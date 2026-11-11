//! Avvio, controllo e arresto di `llama-server`.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::process::{Child, Command};

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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedModel {
    pub model_path: String,
    pub model_name: String,
    pub params: LoadParams,
    pub port: u16,
    pub pid: u32,
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
        self.data_dir.join("bin").join("llama.cpp")
    }

    pub fn log_path(&self) -> PathBuf {
        self.data_dir.join("logs").join("llama-server.log")
    }

    /// Cerca `llama-server` tra le versioni installate; se ce n'è più d'una
    /// prende l'ultima in ordine alfabetico (cioè la build più recente).
    pub fn find_server(&self) -> Result<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(self.bin_dir())
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path().join(SERVER_EXE))
            .filter(|exe| exe.is_file())
            .collect();
        found.sort();
        found.pop().ok_or_else(|| {
            Error::msg(format!(
                "llama.cpp non è installato: manca {SERVER_EXE} in {}",
                self.bin_dir().display()
            ))
        })
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

    /// Carica un modello, scaricando prima quello eventualmente presente.
    pub async fn load(
        &mut self,
        model_path: &str,
        model_name: &str,
        params: &LoadParams,
    ) -> Result<LoadedModel> {
        self.unload().await;

        let exe = self.find_server()?;
        let port = free_port()?;
        let log_path = self.log_path();
        if let Some(dir) = log_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let log = File::create(&log_path)?;

        let mut cmd = Command::new(&exe);
        cmd.args(build_args(model_path, params, port))
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
