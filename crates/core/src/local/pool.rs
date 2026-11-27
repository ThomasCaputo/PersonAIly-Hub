//! Più modelli locali caricati insieme (ADR 0029): un `llama-server` per
//! file GGUF, ognuno nel suo [`Engine`].

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::runtime::{LoadedModel, LocalRuntime};

/// Il nome del file di registro di `llama-server` per il motore numero `index`:
/// il primo usa quello di sempre.
pub fn log_name(index: usize) -> String {
    if index == 0 {
        "llama-server.log".into()
    } else {
        format!("llama-server-{}.log", index + 1)
    }
}

/// La chiave con cui si riconosce un modello: il suo percorso, dove su
/// Windows maiuscole e barre non contano.
pub fn key(path: &str) -> String {
    if cfg!(windows) {
        path.trim().replace('/', "\\").to_lowercase()
    } else {
        path.trim().to_string()
    }
}

/// Un modello locale caricato, o in caricamento.
pub struct Engine {
    pub key: String,
    /// Occupato mentre il modello si carica o si scarica: le risposte non lo
    /// prendono, leggono solo porta e chiave.
    pub runtime: tokio::sync::Mutex<LocalRuntime>,
    last_use: Mutex<Instant>,
    /// Ordine di caricamento: il più vecchio viene prima.
    pub seq: u64,
    /// Quale registro usa (vedi [`log_name`]).
    pub log_index: usize,
}

impl Engine {
    pub fn new(
        key: String,
        seq: u64,
        log_index: usize,
        data_dir: PathBuf,
        http: reqwest::Client,
    ) -> Self {
        Self {
            key,
            runtime: tokio::sync::Mutex::new(LocalRuntime::named(
                data_dir,
                http,
                log_name(log_index),
            )),
            last_use: Mutex::new(Instant::now()),
            seq,
            log_index,
        }
    }

    /// Il modello, se è caricato e non si sta caricando o scaricando.
    pub fn loaded(&self) -> Option<LoadedModel> {
        self.runtime.try_lock().ok().and_then(|mut r| r.loaded())
    }

    /// Si sta caricando o scaricando.
    pub fn is_busy(&self) -> bool {
        self.runtime.try_lock().is_err()
    }

    pub fn touch(&self) {
        *self.last_use.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    pub fn idle(&self) -> Duration {
        self.last_use
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed()
    }
}

/// Quanta VRAM deve restare libera caricando un modello accanto ad altri:
/// il 5% del totale, e non meno di 512 MB.
pub fn vram_reserve(total: u64) -> u64 {
    (total / 20).max(512 * 1024 * 1024)
}

/// Quanta RAM deve restare libera caricando un modello accanto ad altri:
/// il 10% del totale, e non meno di 2 GB.
pub fn ram_reserve(total: u64) -> u64 {
    (total / 10).max(2 * 1024 * 1024 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn il_margine_cresce_con_la_memoria() {
        assert_eq!(vram_reserve(8 * GB), 512 * 1024 * 1024);
        assert_eq!(vram_reserve(24 * GB), 24 * GB / 20);
        assert_eq!(ram_reserve(16 * GB), 2 * GB);
        assert_eq!(ram_reserve(64 * GB), 64 * GB / 10);
    }

    #[test]
    fn la_chiave_non_guarda_maiuscole_e_barre_su_windows() {
        if cfg!(windows) {
            assert_eq!(key("C:/Modelli/Qwen.gguf"), key(r"c:\modelli\qwen.gguf"));
        }
        assert_eq!(log_name(0), "llama-server.log");
        assert_eq!(log_name(1), "llama-server-2.log");
    }
}
