//! Avvio, controllo e arresto di `llama-server`.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::process::{Child, Command};

use super::install::Variant;
use super::{LoadParams, build_args};
use crate::error::{Error, Result};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Tempo massimo di attesa perché il modello sia pronto.
const LOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[cfg(windows)]
const SERVER_EXE: &str = "llama-server.exe";
#[cfg(not(windows))]
const SERVER_EXE: &str = "llama-server";

/// Il modello attualmente caricato.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LoadedModel {
    pub model_path: String,
    pub model_name: String,
    pub params: LoadParams,
    pub port: u16,
    pub pid: u32,
    /// Cartella della versione di llama.cpp che lo esegue (`b12110-cuda`).
    pub engine: String,
    /// Caricato con il suo componente visivo (`mmproj`): legge le immagini.
    pub vision: bool,
    /// Chiave casuale scelta a ogni avvio: senza, qualunque programma (o
    /// pagina web) sul computer potrebbe usare `llama-server` saltando le
    /// protezioni dell'API locale. Non esce mai dal nucleo.
    #[serde(skip)]
    pub api_key: String,
}

/// Cartella in cui l'app tiene le versioni scaricate di llama.cpp.
pub fn bin_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("bin").join("llama.cpp")
}

/// Una versione di llama.cpp installata.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledEngine {
    /// Nome della cartella, per esempio `b12110-cuda`.
    pub dir_name: String,
    /// `None` per una cartella dal nome inatteso (copiata a mano).
    pub variant: Option<Variant>,
    #[serde(skip)]
    pub exe: PathBuf,
}

/// Le versioni installate, in ordine alfabetico (quindi per build).
pub fn installed_engines(data_dir: &Path) -> Vec<InstalledEngine> {
    let mut found: Vec<InstalledEngine> = std::fs::read_dir(bin_dir(data_dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let exe = entry.path().join(SERVER_EXE);
            let dir_name = entry.file_name().to_string_lossy().into_owned();
            // Le cartelle di lavoro dell'installazione (`.tmp`, `_download`) non contano.
            (exe.is_file() && !dir_name.ends_with(".tmp")).then(|| InstalledEngine {
                variant: Variant::from_dir_name(&dir_name),
                dir_name,
                exe,
            })
        })
        .collect();
    found.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    found
}

/// Sceglie `llama-server` tra le versioni installate: la build più recente
/// della variante preferita; se quella non c'è, la più recente delle altre
/// (CUDA, poi Vulkan, poi solo processore). Non serve il motore: si può
/// chiedere anche mentre un modello si carica.
pub fn find_server(data_dir: &Path, preferred: Option<Variant>) -> Result<InstalledEngine> {
    let engines = installed_engines(data_dir);
    let newest = |variant: Option<Variant>| engines.iter().rev().find(|e| e.variant == variant);
    preferred
        .and_then(|v| newest(Some(v)))
        .or_else(|| Variant::ALL.into_iter().find_map(|v| newest(Some(v))))
        .or_else(|| engines.last())
        .cloned()
        .ok_or_else(|| {
            Error::msg(format!(
                "llama.cpp non è installato: manca {SERVER_EXE} in {}",
                bin_dir(data_dir).display()
            ))
        })
}

/// Gestisce il processo `llama-server`. Un solo modello alla volta.
pub struct LocalRuntime {
    data_dir: PathBuf,
    http: reqwest::Client,
    current: Option<(LoadedModel, Child)>,
    #[cfg(windows)]
    job: Option<win32job::Job>,
}

impl LocalRuntime {
    pub fn new(data_dir: PathBuf, http: reqwest::Client) -> Self {
        Self {
            data_dir,
            http,
            current: None,
            #[cfg(windows)]
            job: None,
        }
    }

    /// Cartella in cui l'app tiene le versioni scaricate di llama.cpp.
    pub fn bin_dir(&self) -> PathBuf {
        bin_dir(&self.data_dir)
    }

    pub fn log_path(&self) -> PathBuf {
        self.data_dir.join("logs").join("llama-server.log")
    }

    /// Il modello caricato, se il suo processo è ancora vivo.
    pub fn loaded(&mut self) -> Option<LoadedModel> {
        if let Some((_, child)) = &mut self.current
            && !matches!(child.try_wait(), Ok(None))
        {
            // Il processo è terminato da solo (crash o chiusura esterna).
            self.current = None;
        }
        self.current.as_ref().map(|(model, _)| model.clone())
    }

    /// Carica un modello con la versione di llama.cpp indicata, scaricando
    /// prima quello eventualmente presente. Con `mmproj` il modello legge
    /// anche le immagini.
    pub async fn load(
        &mut self,
        engine: &InstalledEngine,
        model_path: &str,
        model_name: &str,
        params: &LoadParams,
        mmproj: Option<&str>,
    ) -> Result<LoadedModel> {
        self.unload().await;

        let exe = &engine.exe;
        let port = free_port()?;
        let api_key = crate::instance::new_token()?;
        let log_path = self.log_path();
        if let Some(dir) = log_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let log = File::create(&log_path)?;

        let mut cmd = Command::new(exe);
        cmd.args(build_args(model_path, params, port, mmproj))
            // Nell'ambiente e non tra gli argomenti: la riga di comando di un
            // processo si legge facilmente (Gestione attività, `wmic`).
            .env("LLAMA_API_KEY", &api_key)
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);
        self.bind_to_app_lifetime(&child);

        if let Err(e) = self.wait_until_ready(&mut child, port).await {
            let _ = child.kill().await;
            return Err(e);
        }

        let model = LoadedModel {
            model_path: model_path.to_string(),
            model_name: model_name.to_string(),
            params: params.clone(),
            port,
            pid,
            engine: engine.dir_name.clone(),
            vision: mmproj.is_some(),
            api_key,
        };
        self.current = Some((model.clone(), child));
        Ok(model)
    }

    /// Termina il processo: è così che la memoria torna tutta al sistema.
    pub async fn unload(&mut self) {
        if let Some((_, mut child)) = self.current.take() {
            let _ = child.kill().await;
        }
    }

    /// Fa in modo che `llama-server` muoia insieme all'app anche se questa
    /// va in crash o viene terminata a forza: altrimenti resterebbe acceso
    /// con tutta la memoria occupata.
    ///
    /// Su Windows si usa un "job": quando l'ultimo riferimento al job si chiude
    /// (cioè quando il nostro processo finisce, in qualunque modo), il sistema
    /// termina i processi che ne fanno parte.
    #[cfg(windows)]
    fn bind_to_app_lifetime(&mut self, child: &Child) {
        if self.job.is_none() {
            self.job = win32job::Job::create().ok().and_then(|job| {
                let mut info = job.query_extended_limit_info().ok()?;
                info.limit_kill_on_job_close();
                job.set_extended_limit_info(&info).ok()?;
                Some(job)
            });
        }
        if let (Some(job), Some(handle)) = (&self.job, child.raw_handle()) {
            let _ = job.assign_process(handle as _);
        }
    }

    #[cfg(not(windows))]
    fn bind_to_app_lifetime(&mut self, _child: &Child) {}

    async fn wait_until_ready(&self, child: &mut Child, port: u16) -> Result<()> {
        let url = format!("http://127.0.0.1:{port}/health");
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                return Err(Error::msg(format!(
                    "llama-server si è chiuso durante il caricamento ({status}).\n{}",
                    self.log_tail(12)
                )));
            }
            // Mentre carica risponde 503; quando è pronto, 200.
            if let Ok(response) = self.http.get(&url).send().await
                && response.status().is_success()
            {
                return Ok(());
            }
            if start.elapsed() > LOAD_TIMEOUT {
                return Err(Error::msg(
                    "il caricamento del modello ha superato il tempo massimo",
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Ultime righe del registro di `llama-server`, per spiegare un errore.
    pub fn log_tail(&self, lines: usize) -> String {
        let text = std::fs::read(self.log_path())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }
}

/// Chiede al sistema una porta libera su 127.0.0.1.
fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}
