//! Più modelli locali caricati insieme (ADR 0029): un `llama-server` per
//! file GGUF, ognuno nel suo [`Engine`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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
    /// Lavori in corso su questo modello (risposte, sessioni, completamenti).
    users: AtomicUsize,
    /// Cresce a ogni uso che non viene da un'esecuzione pianificata: a lavoro
    /// finito, il modello caricato da una pianificazione si scarica solo se
    /// nessun altro l'ha usato (ADR 0028).
    attended_uses: AtomicU64,
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
            users: AtomicUsize::new(0),
            attended_uses: AtomicU64::new(0),
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

    /// Segna l'inizio di un lavoro su questo modello, finché il segno vive.
    /// `attended`: lo fa qualcuno che non è un'esecuzione pianificata (chat,
    /// completamento in linea, sessione avviata dall'utente).
    pub fn begin_use(self: &Arc<Self>, attended: bool) -> EngineUse {
        if attended {
            self.attended_uses.fetch_add(1, Ordering::Relaxed);
        }
        self.users.fetch_add(1, Ordering::SeqCst);
        EngineUse(self.clone())
    }

    /// Quante volte qualcuno che non è una pianificazione ha usato il modello.
    pub fn attended_uses(&self) -> u64 {
        self.attended_uses.load(Ordering::Relaxed)
    }

    /// Qualcuno lo sta usando adesso.
    pub fn in_use(&self) -> bool {
        self.users.load(Ordering::SeqCst) > 0
    }
}

/// Un lavoro in corso su un modello (vedi [`Engine::begin_use`]).
pub struct EngineUse(Arc<Engine>);

impl Drop for EngineUse {
    fn drop(&mut self) {
        self.0.users.fetch_sub(1, Ordering::SeqCst);
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
    fn gli_usi_si_contano_per_modello() {
        let http = reqwest::Client::new();
        let a = Arc::new(Engine::new(
            "a".into(),
            0,
            0,
            std::env::temp_dir(),
            http.clone(),
        ));
        let b = Arc::new(Engine::new("b".into(), 1, 1, std::env::temp_dir(), http));
        {
            let _chat = a.begin_use(true);
            let _pianificata = a.begin_use(false);
            assert!(a.in_use());
            assert!(!b.in_use());
        }
        // Finiti i lavori, non è più in uso; conta solo chi non è una pianificazione.
        assert!(!a.in_use());
        assert_eq!(a.attended_uses(), 1);
        assert_eq!(b.attended_uses(), 0);
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
