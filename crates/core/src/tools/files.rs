//! Strumenti sui file: sempre e solo dentro le cartelle concesse dall'utente.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Oltre questa dimensione un file non viene letto.
const MAX_READ_BYTES: u64 = 20 * 1024 * 1024;
/// Quanti caratteri di un file tornano al modello, per non riempirne il contesto.
const MAX_READ_CHARS: usize = 60_000;
const MAX_LIST_ENTRIES: usize = 300;

/// Trasforma il percorso chiesto dal modello in un percorso reale e controlla
/// che stia dentro una cartella concessa.
///
/// Il confronto si fa sui percorsi **risolti** (senza `..`, collegamenti e
/// giunzioni seguiti fino in fondo): è l'unico modo per non farsi portare
/// fuori da un `..\..` o da un collegamento che punta altrove.
pub fn resolve(path: &str, folders: &[PathBuf]) -> Result<PathBuf> {
    let roots: Vec<PathBuf> = folders
        .iter()
        .filter_map(|f| f.canonicalize().ok())
        .collect();
    let Some(first) = roots.first() else {
        return Err(Error::msg(
            "Nessuna cartella concessa: l'utente deve aggiungerne una nelle Impostazioni.",
        ));
    };

    let raw = Path::new(path.trim());
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        first.join(raw)
    };

    // Un file da creare non esiste ancora: si risolve la parte che esiste e
    // si riattacca il resto, che non può contenere `..`.
    let mut existing = joined.as_path();
    let mut rest: Vec<&OsStr> = Vec::new();
    let invalid = || Error::msg(format!("Percorso non valido: {path}"));
    let mut full = loop {
        match existing.canonicalize() {
            Ok(real) => break real,
            Err(_) => {
                rest.push(existing.file_name().ok_or_else(invalid)?);
                existing = existing.parent().ok_or_else(invalid)?;
            }
        }
    };
    full.extend(rest.iter().rev());

    if roots.iter().any(|root| full.starts_with(root)) {
        Ok(full)
    } else {
        Err(Error::msg(format!(
            "{path} è fuori dalle cartelle concesse dall'utente: accesso negato."
        )))
    }
}

/// Percorso da mostrare: senza il prefisso `\\?\` dei percorsi risolti di Windows.
pub fn display(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

pub fn list_folder(path: &str, folders: &[PathBuf]) -> Result<String> {
    let dir = resolve(path, folders)?;
    let mut entries: Vec<(bool, String, u64)> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|entry| {
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
            let size = meta.map(|m| m.len()).unwrap_or(0);
            (
                is_dir,
                entry.file_name().to_string_lossy().into_owned(),
                size,
            )
        })
        .collect();
    // Prima le cartelle, poi i file, in ordine alfabetico.
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });

    let mut out = format!("Contenuto di {}:\n", display(&dir));
    if entries.is_empty() {
        out.push_str("(cartella vuota)\n");
    }
    for (is_dir, name, size) in entries.iter().take(MAX_LIST_ENTRIES) {
        if *is_dir {
            out.push_str(&format!("[cartella] {name}\n"));
        } else {
            out.push_str(&format!("{name} ({size} byte)\n"));
        }
    }
    if entries.len() > MAX_LIST_ENTRIES {
        out.push_str(&format!(
            "… e altri {} elementi non mostrati.\n",
            entries.len() - MAX_LIST_ENTRIES
        ));
    }
    Ok(out)
}

pub fn read_file(path: &str, folders: &[PathBuf]) -> Result<String> {
    let file = resolve(path, folders)?;
    let text = read_text(&file)?;
    Ok(truncate(text, MAX_READ_CHARS))
}

/// Legge un file come testo: i PDF vengono convertiti, i file binari rifiutati.
pub fn read_text(file: &Path) -> Result<String> {
    let size = std::fs::metadata(file)?.len();
    if size > MAX_READ_BYTES {
        return Err(Error::msg(format!(
            "{} è troppo grande per essere letto ({} MB).",
            display(file),
            size / (1024 * 1024)
        )));
    }
    let bytes = std::fs::read(file)?;
    let is_pdf = file
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));
    if is_pdf {
        return pdf_text(&bytes).map_err(|e| {
            Error::msg(format!(
                "Non riesco a leggere il testo di {}: {e}",
                display(file)
            ))
        });
    }
    // Il segno invisibile che alcuni programmi di Windows mettono in testa ai file.
    let bytes = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(rest) => rest.to_vec(),
        None => bytes,
    };
    String::from_utf8(bytes).map_err(|_| {
        Error::msg(format!(
            "{} non è un file di testo (o non è in UTF-8).",
            display(file)
        ))
    })
}

/// Il testo di un PDF. `pdf-extract` va in panic con molti PDF insoliti o
/// rovinati: il panic si ferma qui e diventa un errore, invece di chiudere
/// l'app (per questo il profilo di rilascio usa `panic = "unwind"`).
fn pdf_text(bytes: &[u8]) -> std::result::Result<String, String> {
    match std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes)) {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("il PDF ha una struttura che non so interpretare".into()),
    }
}

/// Accorcia un testo troppo lungo, dicendolo chiaramente a chi lo legge.
pub fn truncate(text: String, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text;
    }
    let cut: String = text.chars().take(max_chars).collect();
    format!("{cut}\n\n[Testo troncato: mostrati {max_chars} caratteri su {total}.]")
}

pub fn write_file(path: &str, content: &str, folders: &[PathBuf]) -> Result<String> {
    let file = resolve(path, folders)?;
    if file.is_dir() {
        return Err(Error::msg(format!(
            "{} è una cartella, non un file.",
            display(&file)
        )));
    }
    let existed = file.exists();
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomic(&file, content.as_bytes())?;
    Ok(format!(
        "{} {} ({} caratteri).",
        if existed { "Sostituito" } else { "Creato" },
        display(&file),
        content.chars().count()
    ))
}

/// Scrive prima un file temporaneo accanto a quello vero, poi lo rinomina:
/// se la scrittura si interrompe a metà, il file originale resta intatto.
fn write_atomic(file: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let name = file
        .file_name()
        .ok_or_else(|| Error::msg("Percorso senza nome di file."))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(".{}.tmp", std::process::id()));
    let temp = file.with_file_name(temp_name);
    let written = std::fs::File::create(&temp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    if std::fs::rename(&temp, file).is_err() {
        // Su Windows la rinomina fallisce se un altro programma tiene aperto
        // il file senza permetterlo: si scrive allora direttamente.
        let _ = std::fs::remove_file(&temp);
        std::fs::write(file, bytes)?;
    }
    Ok(())
}

pub fn edit_file(
    path: &str,
    old_text: &str,
    new_text: &str,
    folders: &[PathBuf],
) -> Result<String> {
    let file = resolve(path, folders)?;
    if std::fs::metadata(&file)?.len() > MAX_READ_BYTES {
        return Err(Error::msg(format!(
            "{} è troppo grande per essere modificato.",
            display(&file)
        )));
    }
    let text = String::from_utf8(std::fs::read(&file)?)
        .map_err(|_| Error::msg("Il file non è di testo: non si può modificare."))?;
    if old_text.is_empty() {
        return Err(Error::msg(
            "old_text è vuoto: indica il testo da sostituire.",
        ));
    }
    match text.matches(old_text).count() {
        0 => Err(Error::msg(
            "old_text non compare nel file: rileggilo e copia il testo esatto.",
        )),
        1 => {
            write_atomic(&file, text.replacen(old_text, new_text, 1).as_bytes())?;
            Ok(format!("Modificato {}.", display(&file)))
        }
        n => Err(Error::msg(format!(
            "old_text compare {n} volte nel file: aggiungi righe vicine per renderlo unico."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Due cartelle sorelle: una concessa e una no.
    fn sandbox(tag: &str) -> (PathBuf, PathBuf, Vec<PathBuf>) {
        let base = std::env::temp_dir().join(format!("strumenti-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let granted = base.join("concessa");
        let secret = base.join("segreta");
        std::fs::create_dir_all(&granted).unwrap();
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(secret.join("password.txt"), "segreto").unwrap();
        (base, secret, vec![granted])
    }

    #[test]
    fn non_si_esce_dalle_cartelle_concesse() {
        let (base, secret, folders) = sandbox("confine");

        // Percorso assoluto altrove, risalita con `..`, anche verso file da creare.
        let outside = secret.join("password.txt");
        assert!(read_file(outside.to_str().unwrap(), &folders).is_err());
        assert!(read_file(r"..\segreta\password.txt", &folders).is_err());
        assert!(write_file(r"..\segreta\nuovo.txt", "x", &folders).is_err());
        assert!(write_file(r"nuova\..\..\segreta\nuovo.txt", "x", &folders).is_err());
        assert!(list_folder("..", &folders).is_err());
        assert!(!secret.join("nuovo.txt").exists());

        // Senza cartelle concesse non si tocca nulla.
        assert!(read_file(outside.to_str().unwrap(), &[]).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(windows)]
    #[test]
    fn una_giunzione_non_porta_fuori() {
        let (base, secret, folders) = sandbox("giunzione");
        let link = folders[0].join("scorciatoia");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&secret)
            .output()
            .is_ok_and(|o| o.status.success());
        if made {
            assert!(read_file(r"scorciatoia\password.txt", &folders).is_err());
            assert!(write_file(r"scorciatoia\nuovo.txt", "x", &folders).is_err());
            let _ = std::fs::remove_dir(&link);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn crea_legge_modifica_ed_elenca() {
        let (base, _, folders) = sandbox("uso");

        // Percorso relativo: parte dalla prima cartella concessa; le sottocartelle si creano.
        let created = write_file(r"note\spesa.txt", "pane\nlatte\n", &folders).unwrap();
        assert!(created.starts_with("Creato"));
        assert_eq!(
            read_file(r"note\spesa.txt", &folders).unwrap(),
            "pane\nlatte\n"
        );

        assert!(edit_file(r"note\spesa.txt", "latte", "latte di avena", &folders).is_ok());
        assert!(edit_file(r"note\spesa.txt", "uova", "x", &folders).is_err());
        let absolute = folders[0].join("note").join("spesa.txt");
        assert_eq!(
            read_file(absolute.to_str().unwrap(), &folders).unwrap(),
            "pane\nlatte di avena\n"
        );

        // Testo ripetuto: la modifica è ambigua e viene rifiutata.
        write_file("doppio.txt", "a a", &folders).unwrap();
        assert!(edit_file("doppio.txt", "a", "b", &folders).is_err());
        assert!(
            write_file("doppio.txt", "b", &folders)
                .unwrap()
                .starts_with("Sostituito")
        );

        let listing = list_folder(".", &folders).unwrap();
        assert!(listing.contains("[cartella] note"));
        assert!(listing.contains("doppio.txt (1 byte)"));
        assert!(!listing.contains(r"\\?\"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn i_file_binari_e_quelli_lunghi_sono_gestiti() {
        let (base, _, folders) = sandbox("limiti");
        std::fs::write(folders[0].join("immagine.bin"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
        assert!(read_file("immagine.bin", &folders).is_err());

        // Un PDF rovinato è un errore, non un crash.
        std::fs::write(
            folders[0].join("rotto.pdf"),
            b"%PDF-1.7
1 0 obj << /Type /Catalog",
        )
        .unwrap();
        assert!(read_file("rotto.pdf", &folders).is_err());

        let long = truncate("à".repeat(50), 10);
        assert!(long.starts_with(&"à".repeat(10)));
        assert!(long.contains("10 caratteri su 50"));
        assert_eq!(truncate("corto".into(), 10), "corto");
        let _ = std::fs::remove_dir_all(&base);
    }
}
