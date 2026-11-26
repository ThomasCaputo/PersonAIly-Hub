//! Backup e ripristino dei dati dell'utente.
//!
//! Un backup è un file `.zip` con l'archivio (`data.sqlite`, copiato in modo
//! coerente anche mentre l'app lavora), i preset (`presets/`), le skill (`skills/`), i plugin (`plugins/`) e una
//! descrizione (`backup.json`). Chiavi API, token e variabili dei server MCP
//! stanno nel Gestore credenziali di Windows e **non** sono inclusi; nemmeno
//! i modelli e llama.cpp, che si riscaricano.
//!
//! Il ripristino non tocca l'archivio aperto: prepara i file in
//! `restore-pending/` e l'app si riavvia. Al riavvio, prima di aprire
//! l'archivio, [`apply_pending`] mette da parte i dati attuali in
//! `backups/before-restore-<data>/` e mette al loro posto quelli del backup.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store;

const DB_NAME: &str = "data.sqlite";
const MANIFEST_NAME: &str = "backup.json";
const PRESETS_DIR: &str = "presets";
const SKILLS_DIR: &str = crate::skills::SKILLS_DIR;
const PLUGINS_DIR: &str = crate::plugins::PLUGINS_DIR;
const PENDING_DIR: &str = "restore-pending";
/// Quanto può occupare, estratto, un backup da ripristinare: molto più di un
/// archivio vero, ma uno zip costruito apposta non riempie il disco.
const MAX_RESTORE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
/// La descrizione `backup.json` è di poche righe.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// Che cosa contiene un backup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    /// Versione dell'app che l'ha creato.
    pub app_version: String,
    /// Schema dell'archivio salvato.
    pub schema: i64,
    /// Data e ora di creazione (UTC, `20261120-173150`).
    pub created: String,
}

/// Scrive il backup in `dest` (un `.zip`). `copy_db` scrive la copia coerente
/// dell'archivio nel percorso che riceve: è l'unico momento in cui serve
/// l'archivio, che resta così libero mentre lo zip si comprime (secondi, per
/// un archivio grande).
pub fn create(
    copy_db: impl FnOnce(&Path) -> Result<()>,
    data_dir: &Path,
    dest: &Path,
) -> Result<BackupInfo> {
    // Un nome per backup: due backup insieme non si toccano la copia.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let snapshot = data_dir.join(format!("backup-{}-{seq}.sqlite.tmp", std::process::id()));
    copy_db(&snapshot)?;
    let result = write_zip(&snapshot, data_dir, dest);
    let _ = std::fs::remove_file(&snapshot);
    result
}

fn write_zip(snapshot: &Path, data_dir: &Path, dest: &Path) -> Result<BackupInfo> {
    let info = BackupInfo {
        app_version: crate::version().to_string(),
        schema: store::schema_of(snapshot)?,
        created: crate::log::file_stamp(),
    };
    // Si scrive accanto alla destinazione e si rinomina alla fine: un backup
    // interrotto non deve sostituire uno buono con lo stesso nome.
    let partial = dest.with_extension("zip.part");
    let zip_error = |e: zip::result::ZipError| Error::msg(format!("backup non riuscito: {e}"));
    {
        let mut zip = zip::ZipWriter::new(File::create(&partial)?);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(MANIFEST_NAME, options).map_err(zip_error)?;
        zip.write_all(&serde_json::to_vec_pretty(&info)?)?;
        zip.start_file(DB_NAME, options).map_err(zip_error)?;
        std::io::copy(&mut File::open(snapshot)?, &mut zip)?;
        for folder in [PRESETS_DIR, SKILLS_DIR, PLUGINS_DIR] {
            for (name, path) in files_under(&data_dir.join(folder), folder) {
                // Le importazioni di skill a metà non si salvano.
                if name.contains("/.import-") {
                    continue;
                }
                zip.start_file(name, options).map_err(zip_error)?;
                std::io::copy(&mut File::open(path)?, &mut zip)?;
            }
        }
        zip.finish().map_err(zip_error)?;
    }
    std::fs::rename(&partial, dest).inspect_err(|_| {
        let _ = std::fs::remove_file(&partial);
    })?;
    crate::log::info(format!("backup creato, schema {}", info.schema));
    Ok(info)
}

/// I file di una cartella, con il nome che avranno nello zip (`presets/...`).
fn files_under(dir: &Path, prefix: &str) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let zip_name = format!("{prefix}/{name}");
        match entry.file_type() {
            Ok(t) if t.is_dir() => out.extend(files_under(&path, &zip_name)),
            Ok(t) if t.is_file() => out.push((zip_name, path)),
            _ => {}
        }
    }
    out
}

/// Controlla il backup e lo prepara per il prossimo avvio. Rifiuta i file che
/// non sono backup dell'app, gli archivi rovinati e quelli di una versione
/// più recente (questa non saprebbe leggerli).
pub fn stage(data_dir: &Path, source: &Path) -> Result<BackupInfo> {
    let pending = data_dir.join(PENDING_DIR);
    let _ = std::fs::remove_dir_all(&pending);
    let result = extract(source, &pending);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&pending);
    }
    result
}

fn extract(source: &Path, pending: &Path) -> Result<BackupInfo> {
    let not_a_backup = || Error::msg("Il file scelto non è un backup di PersonAIly Hub.");
    let mut zip = zip::ZipArchive::new(File::open(source)?).map_err(|_| not_a_backup())?;
    let info: BackupInfo = {
        let mut manifest = zip.by_name(MANIFEST_NAME).map_err(|_| not_a_backup())?;
        let mut text = String::new();
        (&mut manifest)
            .take(MAX_MANIFEST_BYTES)
            .read_to_string(&mut text)?;
        serde_json::from_str(&text).map_err(|_| not_a_backup())?
    };
    std::fs::create_dir_all(pending)?;
    let too_big = || {
        Error::msg(format!(
            "Il backup è troppo grande da ripristinare (più di {} GB estratto).",
            MAX_RESTORE_BYTES / (1024 * 1024 * 1024)
        ))
    };
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|e| Error::msg(format!("backup rovinato: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let Ok(name) = entry.name().map(|n| n.into_owned()) else {
            continue;
        };
        // Solo i file che un backup contiene, e mai fuori dalla cartella.
        let allowed = name == DB_NAME
            || name.starts_with(&format!("{PRESETS_DIR}/"))
            || name.starts_with(&format!("{SKILLS_DIR}/"))
            || name.starts_with(&format!("{PLUGINS_DIR}/"));
        let relative = PathBuf::from(&name);
        let safe = relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
        if !allowed || !safe {
            continue;
        }
        let target = pending.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Si contano i byte che escono davvero: la dimensione dichiarata
        // dallo zip può mentire.
        let room = MAX_RESTORE_BYTES - total;
        let written = std::io::copy(
            &mut (&mut entry).take(room + 1),
            &mut File::create(&target)?,
        )?;
        total += written;
        if total > MAX_RESTORE_BYTES {
            return Err(too_big());
        }
    }
    let db = pending.join(DB_NAME);
    if !db.is_file() {
        return Err(not_a_backup());
    }
    let schema = store::schema_of(&db)?;
    if schema > store::LATEST_SCHEMA {
        return Err(Error::msg(format!(
            "Il backup viene da una versione più recente dell'app ({}): aggiorna l'app prima di ripristinarlo.",
            info.app_version
        )));
    }
    crate::log::info(format!(
        "ripristino preparato: backup del {} (versione {}, schema {schema})",
        info.created, info.app_version
    ));
    Ok(info)
}

/// Se c'è un ripristino preparato, lo applica. Va chiamato all'avvio, prima
/// di aprire l'archivio. I dati sostituiti restano in `backups/`.
pub fn apply_pending(data_dir: &Path) -> Result<bool> {
    let pending = data_dir.join(PENDING_DIR);
    let db = pending.join(DB_NAME);
    if !db.is_file() {
        let _ = std::fs::remove_dir_all(&pending);
        return Ok(false);
    }
    let aside = data_dir
        .join("backups")
        .join(format!("before-restore-{}", crate::log::file_stamp()));
    std::fs::create_dir_all(&aside)?;

    // Gli spostamenti fatti, per tornare indietro se uno fallisce a metà:
    // altrimenti l'app ripartirebbe con un archivio vuoto (i dati veri sarebbero
    // in `backups/`, ma l'utente li vedrebbe spariti).
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| -> Result<()> {
        let mut step = |from: PathBuf, to: PathBuf| -> Result<()> {
            if from.exists() {
                std::fs::rename(&from, &to)?;
                done.push((from, to));
            }
            Ok(())
        };
        for name in [DB_NAME, "data.sqlite-wal", "data.sqlite-shm"] {
            step(data_dir.join(name), aside.join(name))?;
        }
        step(data_dir.join(PRESETS_DIR), aside.join(PRESETS_DIR))?;
        step(db.clone(), data_dir.join(DB_NAME))?;
        step(pending.join(PRESETS_DIR), data_dir.join(PRESETS_DIR))?;
        // Skill e plugin si sostituiscono solo se il backup ne ha: uno fatto
        // prima che esistessero non deve far sparire quelli installati dopo.
        for folder in [SKILLS_DIR, PLUGINS_DIR] {
            if pending.join(folder).exists() {
                step(data_dir.join(folder), aside.join(folder))?;
                step(pending.join(folder), data_dir.join(folder))?;
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        // Si rimette tutto com'era, in ordine inverso; il ripristino resta in
        // sospeso e si riprova al prossimo avvio.
        for (from, to) in done.iter().rev() {
            if let Err(undo) = std::fs::rename(to, from) {
                crate::log::error(format!(
                    "ripristino annullato a metà: {} non è tornato in {} ({undo})",
                    to.display(),
                    from.display()
                ));
            }
        }
        let _ = std::fs::remove_dir(&aside);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&pending);
    crate::log::info(format!(
        "backup ripristinato; i dati di prima sono in {}",
        aside.display()
    ));
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pa-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn backup_e_ripristino_tornano_allo_stato_salvato() {
        let dir = temp_dir("roundtrip");
        let store = Store::open(&dir.join(DB_NAME)).unwrap();
        store.create_conversation("Prima del backup").unwrap();
        std::fs::create_dir_all(dir.join("presets/named")).unwrap();
        std::fs::write(dir.join("presets/named/a.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("skills/verbale")).unwrap();
        std::fs::write(dir.join("skills/verbale/SKILL.md"), "prima").unwrap();
        std::fs::create_dir_all(dir.join("skills/.import-1-0")).unwrap();
        std::fs::write(dir.join("skills/.import-1-0/SKILL.md"), "a metà").unwrap();
        std::fs::create_dir_all(dir.join("plugins/revisione/.claude-plugin")).unwrap();
        std::fs::write(
            dir.join("plugins/revisione/.claude-plugin/plugin.json"),
            "{}",
        )
        .unwrap();

        let zip = dir.join("copia.zip");
        let info = create(|copy| store.vacuum_into(copy), &dir, &zip).unwrap();
        assert_eq!(info.schema, store::LATEST_SCHEMA);
        assert!(!dir.join("copia.zip.part").exists());

        // Dopo il backup cambia qualcosa: il ripristino deve riportarlo indietro.
        store.create_conversation("Dopo il backup").unwrap();
        std::fs::remove_file(dir.join("presets/named/a.json")).unwrap();
        std::fs::write(dir.join("skills/verbale/SKILL.md"), "dopo").unwrap();
        drop(store);

        assert_eq!(stage(&dir, &zip).unwrap(), info);
        assert!(apply_pending(&dir).unwrap());
        assert!(!dir.join(PENDING_DIR).exists());
        assert!(dir.join("presets/named/a.json").is_file());
        assert_eq!(
            std::fs::read_to_string(dir.join("skills/verbale/SKILL.md")).unwrap(),
            "prima"
        );
        assert!(!dir.join("skills/.import-1-0").exists());
        assert!(
            dir.join("plugins/revisione/.claude-plugin/plugin.json")
                .is_file()
        );

        let restored = Store::open(&dir.join(DB_NAME)).unwrap();
        let titles: Vec<String> = restored
            .list_conversations("")
            .unwrap()
            .into_iter()
            .map(|c| c.title)
            .collect();
        assert_eq!(titles, ["Prima del backup"]);
        // I dati sostituiti restano da parte.
        let aside = std::fs::read_dir(dir.join("backups")).unwrap().count();
        assert_eq!(aside, 1);
        // Senza nulla da ripristinare l'avvio non cambia niente.
        assert!(!apply_pending(&dir).unwrap());
        drop(restored);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Se un passaggio del ripristino fallisce (qui: un file dei plugin
    /// aperto da un altro programma), tutto torna com'era e il ripristino
    /// resta in sospeso per il prossimo avvio.
    #[cfg(windows)]
    #[test]
    fn un_ripristino_interrotto_rimette_tutto_a_posto() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp_dir("rollback");
        let store = Store::open(&dir.join(DB_NAME)).unwrap();
        store.create_conversation("Nel backup").unwrap();
        std::fs::create_dir_all(dir.join("presets")).unwrap();
        std::fs::write(dir.join("presets/a.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("plugins/p")).unwrap();
        std::fs::write(dir.join("plugins/p/plugin.json"), "{}").unwrap();
        let zip = dir.join("copia.zip");
        create(|copy| store.vacuum_into(copy), &dir, &zip).unwrap();
        store.create_conversation("Dopo il backup").unwrap();
        drop(store);
        stage(&dir, &zip).unwrap();

        // Aperto senza condivisione: la cartella dei plugin non si può spostare.
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(dir.join("plugins/p/plugin.json"))
            .unwrap();
        assert!(apply_pending(&dir).is_err());
        drop(lock);

        assert!(dir.join("presets/a.json").is_file());
        assert!(dir.join(PENDING_DIR).join(DB_NAME).is_file());
        let current = Store::open(&dir.join(DB_NAME)).unwrap();
        assert_eq!(current.list_conversations("").unwrap().len(), 2);
        drop(current);

        // Al prossimo avvio il ripristino riesce.
        assert!(apply_pending(&dir).unwrap());
        let restored = Store::open(&dir.join(DB_NAME)).unwrap();
        assert_eq!(restored.list_conversations("").unwrap().len(), 1);
        drop(restored);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn altri_file_sono_rifiutati() {
        let dir = temp_dir("refused");
        let not_zip = dir.join("testo.zip");
        std::fs::write(&not_zip, "non sono uno zip").unwrap();
        assert!(stage(&dir, &not_zip).is_err());

        // Uno zip senza descrizione non è un backup dell'app.
        let zip_path = dir.join("altro.zip");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        zip.start_file("data.sqlite", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        assert!(stage(&dir, &zip_path).is_err());
        assert!(!dir.join(PENDING_DIR).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
