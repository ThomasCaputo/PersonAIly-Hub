//! Un solo nucleo attivo per utente ([ADR 0004](../../../docs/adr/0004-nucleo-condiviso-api-locale.md)).
//!
//! Chi avvia l'API locale (l'app desktop o l'eseguibile `server`) scrive nella
//! cartella dati il file di blocco `core.lock` con porta e token; gli altri
//! client (l'estensione VS Code) lo leggono per collegarsi.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::error::{Error, Result};

/// Identificatore dell'app: dà il nome alla cartella dati. Deve restare uguale
/// a `identifier` in `apps/desktop/src-tauri/tauri.conf.json`.
pub const APP_IDENTIFIER: &str = "dev.personaily.hub";
pub const LOCK_FILE: &str = "core.lock";

/// Chi ha avviato il nucleo in esecuzione.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum InstanceKind {
    /// L'app desktop.
    App,
    /// L'eseguibile senza interfaccia.
    Server,
}

/// Contenuto del file di blocco.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LockInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub kind: InstanceKind,
    pub version: String,
}

/// La cartella dati predefinita, la stessa che usa l'app desktop:
/// `%APPDATA%\dev.personaily.hub` su Windows.
pub fn default_data_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
    };
    base.map(|base| base.join(APP_IDENTIFIER))
}

pub fn lock_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LOCK_FILE)
}

/// File in cui l'app annota dove è installato `app-server`, così
/// l'estensione VS Code lo trova da sola anche ad app chiusa.
pub const INSTALL_FILE: &str = "install.json";

/// Contenuto di [`INSTALL_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct InstallInfo {
    pub server_path: String,
    pub version: String,
}

/// Annota dove si trova `app-server`. Si scrive solo se cambia: l'app lo
/// chiama a ogni avvio.
pub fn write_install_info(data_dir: &Path, server: &Path) -> Result<()> {
    let info = InstallInfo {
        server_path: server.to_string_lossy().into_owned(),
        version: crate::version().to_string(),
    };
    let path = data_dir.join(INSTALL_FILE);
    let current = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<InstallInfo>(&text).ok());
    if current.as_ref() != Some(&info) {
        std::fs::create_dir_all(data_dir)?;
        std::fs::write(path, serde_json::to_string_pretty(&info)?)?;
    }
    Ok(())
}

/// Il file di blocco, se c'è ed è leggibile.
pub fn read_lock(data_dir: &Path) -> Option<LockInfo> {
    let text = std::fs::read_to_string(lock_path(data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Scrive il file di blocco. Prima un file temporaneo, poi lo si rinomina:
/// chi legge non trova mai un file scritto a metà.
pub fn write_lock(data_dir: &Path, info: &LockInfo) -> Result<()> {
    let path = lock_path(data_dir);
    let temp = path.with_extension("lock.tmp");
    std::fs::write(&temp, serde_json::to_string_pretty(info)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// Prende il file di blocco per questo processo. Se un altro nucleo è già in
/// esecuzione non tocca nulla e lo restituisce. La creazione esclusiva del
/// file evita che due processi avviati insieme credano entrambi di essere soli.
pub fn claim_lock(data_dir: &Path, info: &LockInfo) -> Result<Option<LockInfo>> {
    use std::io::Write;
    let path = lock_path(data_dir);
    for _ in 0..20 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(serde_json::to_string_pretty(info)?.as_bytes())?;
                return Ok(None);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                match read_lock(data_dir) {
                    Some(other) if other.pid == std::process::id() => {
                        write_lock(data_dir, info)?;
                        return Ok(None);
                    }
                    Some(other) if lock_owner_alive(data_dir, &other) => return Ok(Some(other)),
                    Some(_) => {
                        // Lasciato da un processo terminato male.
                        let _ = std::fs::remove_file(&path);
                    }
                    None => {
                        // Illeggibile: forse un altro processo lo sta scrivendo
                        // proprio ora. Se resta così, è un avanzo da togliere.
                        let fresh = std::fs::metadata(&path)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.elapsed().ok())
                            .is_some_and(|age| age < std::time::Duration::from_secs(2));
                        if fresh {
                            std::thread::sleep(std::time::Duration::from_millis(150));
                        } else {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::msg(
        "Il file di blocco del nucleo non si riesce a creare.",
    ))
}

/// Toglie il file di blocco, ma solo se l'ha scritto questo processo.
pub fn remove_lock(data_dir: &Path) {
    if read_lock(data_dir).is_some_and(|lock| lock.pid == std::process::id()) {
        let _ = std::fs::remove_file(lock_path(data_dir));
    }
}

/// `true` se il processo esiste ancora.
pub fn process_alive(pid: u32) -> bool {
    let pid = Pid::from_u32(pid);
    let Ok(mut sys) = System::new() else {
        // Senza poterlo verificare si presume vivo: l'API risponderà o no.
        return true;
    };
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(pid).is_some()
}

/// Il file di blocco di un nucleo ancora in esecuzione; quello lasciato da
/// un processo terminato male viene ignorato.
pub fn running_instance(data_dir: &Path) -> Option<LockInfo> {
    read_lock(data_dir)
        .filter(|lock| lock.pid != std::process::id() && lock_owner_alive(data_dir, lock))
}

/// Il processo che ha scritto il file di blocco è ancora vivo. Non basta che
/// esista un processo con quel numero: Windows ricicla i numeri, e un nucleo
/// chiuso male (o una build di sviluppo fermata) può lasciare il suo file a un
/// programma qualsiasi nato dopo. Vale solo un processo nato prima che il file
/// fosse scritto.
pub fn lock_owner_alive(data_dir: &Path, lock: &LockInfo) -> bool {
    let Some(started) = process_start(lock.pid) else {
        return false;
    };
    let written = std::fs::metadata(lock_path(data_dir))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    // Un secondo di tolleranza: le due misure hanno precisioni diverse.
    written.is_none_or(|written| started <= written + 1)
}

/// Quando è nato il processo, in secondi dal 1970; `None` se non esiste.
fn process_start(pid: u32) -> Option<u64> {
    let pid = Pid::from_u32(pid);
    let mut sys = System::new().ok()?;
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(pid).map(|p| p.start_time())
}

/// Un token nuovo: 32 byte casuali in esadecimale.
pub fn new_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| Error::msg(format!("generatore casuale di sistema: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Confronto che non si ferma al primo carattere diverso: il tempo di
/// risposta non rivela quanta parte del token è giusta.
pub fn tokens_match(given: &str, expected: &str) -> bool {
    let (a, b) = (given.as_bytes(), expected.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un numero di processo riciclato da un programma nato dopo il file di
    /// blocco non fa credere che il nucleo sia ancora aperto.
    #[cfg(windows)]
    #[test]
    fn numero_di_processo_riciclato() {
        let dir = std::env::temp_dir().join(format!("lock-riciclo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut info = LockInfo {
            port: 1,
            token: "t".into(),
            pid: std::process::id(),
            kind: InstanceKind::Server,
            version: "0".into(),
        };
        write_lock(&dir, &info).unwrap();
        // Questo processo è nato prima del file: è il proprietario.
        assert!(lock_owner_alive(&dir, &info));

        // Un processo nato dopo il file, con il numero scritto nel file.
        std::thread::sleep(std::time::Duration::from_millis(2100));
        let mut later = std::process::Command::new("cmd")
            .args(["/c", "ping -n 3 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        info.pid = later.id();
        assert!(!lock_owner_alive(&dir, &info));
        let _ = later.kill();
        let _ = later.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn token_is_random_hex() {
        let a = new_token().unwrap();
        let b = new_token().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn tokens_compare() {
        assert!(tokens_match("abc", "abc"));
        assert!(!tokens_match("abd", "abc"));
        assert!(!tokens_match("ab", "abc"));
        assert!(!tokens_match("", "abc"));
    }

    #[test]
    fn lock_round_trip() {
        let dir = std::env::temp_dir().join(format!("lock-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let info = LockInfo {
            port: 1234,
            token: "t".into(),
            pid: std::process::id(),
            kind: InstanceKind::Server,
            version: "0".into(),
        };
        assert_eq!(claim_lock(&dir, &info).unwrap(), None);
        assert_eq!(read_lock(&dir), Some(info.clone()));
        // Un file lasciato da un processo che non esiste più si sostituisce.
        let stale = LockInfo {
            pid: u32::MAX - 7,
            ..info.clone()
        };
        write_lock(&dir, &stale).unwrap();
        assert_eq!(claim_lock(&dir, &info).unwrap(), None);
        assert_eq!(read_lock(&dir), Some(info.clone()));
        // Il proprio file di blocco non conta come "un altro nucleo".
        assert_eq!(running_instance(&dir), None);

        // Un nucleo di un altro processo ancora vivo non si sostituisce.
        let mut other = if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/c", "ping -n 6 127.0.0.1 >NUL"])
                .spawn()
                .unwrap()
        } else {
            std::process::Command::new("sleep")
                .arg("5")
                .spawn()
                .unwrap()
        };
        let alive = LockInfo {
            pid: other.id(),
            ..info.clone()
        };
        write_lock(&dir, &alive).unwrap();
        assert_eq!(claim_lock(&dir, &info).unwrap(), Some(alive.clone()));
        assert_eq!(running_instance(&dir), Some(alive));
        // Il file di un altro processo non si toglie.
        remove_lock(&dir);
        assert!(read_lock(&dir).is_some());
        let _ = other.kill();
        let _ = other.wait();

        write_lock(&dir, &info).unwrap();
        remove_lock(&dir);
        assert_eq!(read_lock(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
