//! Pacchetti da importare (skill e plugin): una cartella o un `.zip` copiati
//! in una cartella di lavoro, con limiti di dimensione e senza uscire dalla
//! cartella. Collegamenti e giunzioni non si seguono.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// Quanto può essere grande un pacchetto, e come dirlo all'utente.
pub struct Limits {
    pub files: usize,
    pub bytes: u64,
    pub too_big: &'static str,
}

/// I file di una cartella, relativi a lei, senza collegamenti né giunzioni.
pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            } else if kind.is_dir() {
                walk(root, &path, out);
            } else if kind.is_file()
                && let Ok(relative) = path.strip_prefix(root)
            {
                out.push(relative.to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

/// Una cartella di lavoro nuova dentro `parent`, che comincia con un punto:
/// gli elenchi la saltano e i backup non la salvano.
pub fn staging_dir(parent: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(parent)?;
    let mut n = 0;
    loop {
        let dir = parent.join(format!(".import-{}-{n}", std::process::id()));
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
            return Ok(dir);
        }
        n += 1;
    }
}

/// Copia `source` in `staged`, entro i limiti.
pub fn copy_folder(source: &Path, staged: &Path, limits: &Limits) -> Result<()> {
    let files = files_in(source);
    let total: u64 = files
        .iter()
        .filter_map(|f| std::fs::metadata(source.join(f)).ok())
        .map(|m| m.len())
        .sum();
    if files.len() > limits.files || total > limits.bytes {
        return Err(Error::msg(limits.too_big));
    }
    for file in &files {
        let target = staged.join(file);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source.join(file), target)?;
    }
    Ok(())
}

/// La cartella che fa da radice di un pacchetto: quella che contiene uno dei
/// `markers` (percorsi relativi, per esempio `.claude-plugin/plugin.json`),
/// in cima allo zip o in un'unica cartella principale.
fn zip_root(names: &[PathBuf], markers: &[&str], none: &str, many: &str) -> Result<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in names {
        for marker in markers {
            let marker = Path::new(marker);
            if !path.ends_with(marker) {
                continue;
            }
            let mut root = path.as_path();
            for _ in marker.components() {
                root = root.parent().unwrap_or(Path::new(""));
            }
            if root.components().count() <= 1 && !roots.iter().any(|r| r == root) {
                roots.push(root.to_path_buf());
            }
        }
    }
    roots.sort_by_key(|r| r.components().count());
    match roots.as_slice() {
        [] => Err(Error::msg(none.to_string())),
        [only] => Ok(only.clone()),
        [first, second, ..] if first.as_os_str().is_empty() && !second.as_os_str().is_empty() => {
            Ok(first.clone())
        }
        _ => Err(Error::msg(many.to_string())),
    }
}

/// Estrae in `staged` il pacchetto contenuto nello zip `source`. `none` e
/// `many` sono gli errori quando non c'è nessun pacchetto o ce n'è più d'uno.
pub fn extract_zip(
    source: &Path,
    staged: &Path,
    markers: &[&str],
    limits: &Limits,
    none: &str,
    many: &str,
) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(source)?)
        .map_err(|_| Error::msg("Il file scelto non è un archivio .zip leggibile."))?;
    let names: Vec<PathBuf> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok()?.enclosed_name())
        .collect();
    let root = zip_root(&names, markers, none, many)?;

    let mut count = 0;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| Error::msg(format!("Archivio .zip rovinato: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        // `enclosed_name` rifiuta i percorsi che escono dalla cartella.
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(&root) else {
            continue;
        };
        if !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        {
            continue;
        }
        count += 1;
        if count > limits.files {
            return Err(Error::msg(limits.too_big));
        }
        let target = staged.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        // La dimensione dichiarata dallo zip può mentire: si conta ciò che esce.
        let limit = limits.bytes - total + 1;
        let written = std::io::copy(&mut (&mut entry).take(limit), &mut out)?;
        total += written;
        if total > limits.bytes {
            return Err(Error::msg(limits.too_big));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_radice_dello_zip() {
        let p = |s: &str| PathBuf::from(s);
        let markers = [".claude-plugin/plugin.json", "plugin.json"];
        let root = |names: &[&str]| {
            zip_root(
                &names.iter().map(|n| p(n)).collect::<Vec<_>>(),
                &markers,
                "nessuno",
                "troppi",
            )
        };
        assert_eq!(
            root(&[".claude-plugin/plugin.json", "x.md"]).unwrap(),
            p("")
        );
        assert_eq!(root(&["mio/.claude-plugin/plugin.json"]).unwrap(), p("mio"));
        assert_eq!(root(&["mio/plugin.json"]).unwrap(), p("mio"));
        // Troppo in profondità: non è la radice di un pacchetto.
        assert!(root(&["a/b/plugin.json"]).is_err());
        assert_eq!(
            root(&["a/plugin.json", "b/plugin.json"])
                .unwrap_err()
                .to_string(),
            "troppi"
        );
        assert_eq!(root(&["x.md"]).unwrap_err().to_string(), "nessuno");
    }
}
