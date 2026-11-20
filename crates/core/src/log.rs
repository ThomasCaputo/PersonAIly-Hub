//! Registro diagnostico del nucleo: `logs/core.log` nella cartella dati.
//!
//! Serve a capire che cosa è andato storto quando un utente segnala un
//! problema: avvio, caricamento dei modelli, errori, panic. Resta sul
//! computer (GE-3): esce solo se l'utente copia le informazioni diagnostiche.
//!
//! Non vi si scrivono mai chiavi API, token né il contenuto delle
//! conversazioni. Quando il file supera [`MAX_BYTES`] diventa `core.1.log`
//! e se ne comincia uno nuovo, così occupa al massimo il doppio.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Dimensione oltre la quale il registro ricomincia da un file nuovo.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
const FILE_NAME: &str = "core.log";
const OLD_FILE_NAME: &str = "core.1.log";

struct Writer {
    path: PathBuf,
    file: File,
    written: u64,
}

static LOG: OnceLock<Mutex<Option<Writer>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Writer>> {
    LOG.get_or_init(|| Mutex::new(None))
}

/// Cartella dei registri (anche quelli di `llama-server`).
pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

/// Il file del registro del nucleo.
pub fn path(data_dir: &Path) -> PathBuf {
    dir(data_dir).join(FILE_NAME)
}

/// Apre il registro in `data_dir` e scrive la riga d'avvio. `who` dice quale
/// programma lo usa (`app`, `server`). Chiamarlo di nuovo cambia file.
///
/// Se il registro non si può aprire l'app funziona lo stesso, senza registro.
pub fn init(data_dir: &Path, who: &str) {
    let path = path(data_dir);
    let opened = std::fs::create_dir_all(dir(data_dir))
        .and_then(|()| open_append(&path))
        .map(|(file, written)| Writer {
            path,
            file,
            written,
        });
    *slot().lock().unwrap_or_else(|e| e.into_inner()) = opened.ok();
    info(format!(
        "avvio di {who} {} su {} ({})",
        crate::version(),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
}

fn open_append(path: &Path) -> std::io::Result<(File, u64)> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let written = file.metadata()?.len();
    Ok((file, written))
}

pub fn info(message: impl AsRef<str>) {
    write("INFO", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    write("WARN", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    write("ERROR", message.as_ref());
}

fn write(level: &str, message: &str) {
    let mut guard = slot().lock().unwrap_or_else(|e| e.into_inner());
    let Some(writer) = guard.as_mut() else {
        return;
    };
    // Una riga per voce: gli a capo di un messaggio diventano rientri.
    let line = format!(
        "{} {level:<5} {}\n",
        timestamp(SystemTime::now()),
        message.trim_end().replace('\n', "\n    ")
    );
    if writer.written + line.len() as u64 > MAX_BYTES {
        let old = writer.path.with_file_name(OLD_FILE_NAME);
        let _ = std::fs::remove_file(&old);
        if std::fs::rename(&writer.path, &old).is_ok()
            && let Ok((file, written)) = open_append(&writer.path)
        {
            writer.file = file;
            writer.written = written;
        }
    }
    if writer.file.write_all(line.as_bytes()).is_ok() {
        writer.written += line.len() as u64;
    }
}

/// Scrive nel registro anche i panic, che altrimenti finirebbero solo su
/// una console che nessuno vede. Il comportamento precedente resta.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!(" ({}:{})", l.file(), l.line()))
            .unwrap_or_default();
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string());
        error(format!("panic: {payload}{location}"));
        previous(info);
    }));
}

/// Le ultime `lines` righe del registro, del file vecchio se quello nuovo
/// ne ha meno.
pub fn tail(data_dir: &Path, lines: usize) -> String {
    let read = |p: PathBuf| std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned());
    let current = read(path(data_dir)).unwrap_or_default();
    let mut all: Vec<&str> = current.lines().collect();
    let old;
    if all.len() < lines {
        old = read(dir(data_dir).join(OLD_FILE_NAME)).unwrap_or_default();
        let mut previous: Vec<&str> = old.lines().collect();
        previous.extend(all);
        all = previous;
    }
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

/// Toglie da un testo che l'utente sta per condividere ciò che identifica il
/// suo computer: la cartella utente e il nome utente.
pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    let profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    if profile.len() >= 3 {
        out = replace_ignore_case(&out, &profile, "%USERPROFILE%");
        out = replace_ignore_case(&out, &profile.replace('\\', "/"), "%USERPROFILE%");
    }
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    if user.len() >= 3 {
        out = replace_ignore_case(&out, &user, "<utente>");
    }
    out
}

fn replace_ignore_case(text: &str, needle: &str, with: &str) -> String {
    let lower_text = text.to_lowercase();
    let lower_needle = needle.to_lowercase();
    // Il confronto senza maiuscole vale solo se le lunghezze non cambiano.
    if lower_text.len() != text.len() || lower_needle.len() != needle.len() {
        return text.replace(needle, with);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    for (start, _) in lower_text.match_indices(&lower_needle) {
        if start < rest {
            continue;
        }
        out.push_str(&text[rest..start]);
        out.push_str(with);
        rest = start + needle.len();
    }
    out.push_str(&text[rest..]);
    out
}

/// Data e ora attuali per un nome di file, `20261120-173150` (UTC).
pub fn file_stamp() -> String {
    timestamp(SystemTime::now())
        .replace(['-', ':', 'Z'], "")
        .replace(' ', "-")
}

/// Data e ora in UTC, `2026-11-20 17:31:50Z`.
fn timestamp(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Giorni dal 1970-01-01 → anno, mese, giorno (algoritmo di H. Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn data_e_ora_in_utc() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01 00:00:00Z");
        // 2026-11-20 17:31:50 UTC
        let t = UNIX_EPOCH + Duration::from_secs(1_795_195_910);
        assert_eq!(timestamp(t), "2026-11-20 17:31:50Z");
        // 29 febbraio di un anno bisestile
        let t = UNIX_EPOCH + Duration::from_secs(1_709_164_800);
        assert_eq!(timestamp(t), "2024-02-29 00:00:00Z");
    }

    #[test]
    fn mascheramento_senza_maiuscole() {
        assert_eq!(
            replace_ignore_case(
                r"C:\Users\Mario\x e c:\users\mario\y",
                r"C:\Users\Mario",
                "%U%"
            ),
            r"%U%\x e %U%\y"
        );
    }
}
