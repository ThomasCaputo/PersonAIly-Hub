//! Comandi nel terminale, per gli agenti (AG-5).
//!
//! Il comando parte in una cartella concessa, ma **non è confinato** lì: un
//! programma può leggere e scrivere dove vuole l'utente che lo esegue. Per
//! questo la classe di rischio è "esecuzione" e, salvo il livello "Consenti
//! tutto", ogni comando chiede conferma (vedi ADR 0008).
//!
//! Su Windows il comando gira in PowerShell dentro un "job": quando il comando
//! finisce, scade il tempo o l'utente interrompe, il job viene chiuso e il
//! sistema termina anche i processi che il comando aveva avviato.

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::error::{Error, Result};

/// Tempo massimo predefinito di un comando.
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;
/// Nessun comando può durare più di così.
pub const MAX_TIMEOUT_SECS: u64 = 600;
/// Byte conservati per ciascun flusso (uscita ed errori) mentre il comando gira.
const BUFFER_LIMIT: usize = 512 * 1024;
/// Caratteri dell'uscita restituiti al modello: l'inizio e soprattutto la fine,
/// dove di solito stanno gli errori.
const HEAD_CHARS: usize = 4_000;
const TAIL_CHARS: usize = 16_000;

/// Esito di un comando.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandResult {
    /// `None` se il comando è stato terminato (tempo scaduto).
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl CommandResult {
    /// Il testo che torna al modello.
    pub fn report(&self, timeout: Duration) -> String {
        let mut text = if self.timed_out {
            format!(
                "Il comando ha superato il tempo massimo di {} secondi ed è stato terminato.",
                timeout.as_secs()
            )
        } else {
            match self.exit_code {
                Some(code) => format!("Codice di uscita: {code}"),
                None => "Il comando è stato terminato.".to_string(),
            }
        };
        let stdout = clip(self.stdout.trim_end());
        let stderr = clip(self.stderr.trim_end());
        if !stdout.is_empty() {
            text.push_str("\n\n--- uscita ---\n");
            text.push_str(&stdout);
        }
        if !stderr.is_empty() {
            text.push_str("\n\n--- errori ---\n");
            text.push_str(&stderr);
        }
        if stdout.is_empty() && stderr.is_empty() {
            text.push_str("\n(nessuna uscita)");
        }
        text
    }

    pub fn failed(&self) -> bool {
        self.timed_out || self.exit_code != Some(0)
    }
}

/// Tiene l'inizio e la fine di un testo troppo lungo.
fn clip(text: &str) -> String {
    let count = text.chars().count();
    if count <= HEAD_CHARS + TAIL_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(HEAD_CHARS).collect();
    let tail: String = text.chars().skip(count - TAIL_CHARS).collect();
    format!(
        "{head}\n\n[… {} caratteri omessi …]\n\n{tail}",
        count - HEAD_CHARS - TAIL_CHARS
    )
}

/// Il programma che interpreta il comando.
fn shell(command: &str) -> Command {
    #[cfg(windows)]
    {
        use base64::Engine;
        // L'uscita in UTF-8, niente barre di avanzamento (sporcano il testo) e
        // il codice di uscita dei programmi esterni riportato fuori.
        let script = format!(
            "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8\n\
             $OutputEncoding = [System.Text.Encoding]::UTF8\n\
             $ProgressPreference = 'SilentlyContinue'\n\
             {command}\n\
             if ($LASTEXITCODE -is [int] -and $LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}"
        );
        // Codificato, così virgolette e caratteri speciali arrivano intatti.
        let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded,
        ]);
        cmd.creation_flags(crate::local::runtime::CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        cmd
    }
}

/// Raccoglie un flusso in un buffer condiviso, fino a [`BUFFER_LIMIT`] byte.
fn collect(
    mut stream: impl tokio::io::AsyncRead + Unpin + Send + 'static,
) -> (Arc<Mutex<Vec<u8>>>, tokio::task::JoinHandle<()>) {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let sink = buffer.clone();
    let task = tokio::spawn(async move {
        let mut chunk = [0u8; 8192];
        while let Ok(n) = stream.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            let mut buffer = sink.lock().unwrap_or_else(|e| e.into_inner());
            let room = BUFFER_LIMIT.saturating_sub(buffer.len());
            buffer.extend_from_slice(&chunk[..n.min(room)]);
        }
    });
    (buffer, task)
}

fn take_text(buffer: &Mutex<Vec<u8>>) -> String {
    let bytes = std::mem::take(&mut *buffer.lock().unwrap_or_else(|e| e.into_inner()));
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Con `-EncodedCommand` PowerShell scrive errori e avvisi in CLIXML, un XML
/// illeggibile per il modello: se ne ricava il testo semplice.
fn decode_clixml(text: &str) -> String {
    let Some(xml) = text.trim_start().strip_prefix("#< CLIXML") else {
        return text.to_string();
    };
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<S S=\"") {
        rest = &rest[start + 6..];
        let Some((kind, after)) = rest.split_once("\">") else {
            break;
        };
        let Some((body, after)) = after.split_once("</S>") else {
            break;
        };
        rest = after;
        // Le barre di avanzamento e simili non interessano.
        let prefix = match kind {
            "Error" => "",
            "Warning" => "AVVISO: ",
            "Verbose" | "Debug" | "Information" => "",
            _ => continue,
        };
        out.push_str(prefix);
        out.push_str(&unescape_clixml(body));
    }
    out
}

/// Toglie le sequenze `_xHHHH_` e le entità XML.
fn unescape_clixml(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("_x") {
        out.push_str(&rest[..start]);
        let candidate = &rest[start..];
        let decoded = candidate
            .get(2..6)
            .filter(|_| candidate.as_bytes().get(6) == Some(&b'_'))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .and_then(char::from_u32);
        match decoded {
            Some(c) => {
                if c != '\r' {
                    out.push(c);
                }
                rest = &candidate[7..];
            }
            None => {
                out.push_str("_x");
                rest = &candidate[2..];
            }
        }
    }
    out.push_str(rest);
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Esegue `command` nella cartella `cwd`. Se il futuro viene abbandonato
/// (l'utente interrompe), il comando e i processi che ha avviato vengono terminati.
pub async fn run(command: &str, cwd: &Path, timeout: Duration) -> Result<CommandResult> {
    if !cwd.is_dir() {
        return Err(Error::msg(format!(
            "La cartella {} non esiste.",
            cwd.display()
        )));
    }
    let mut cmd = shell(command);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::msg(format!("Impossibile avviare il comando: {e}")))?;
    // Finché `_job` vive, i processi del comando gli appartengono; quando
    // viene chiuso (fine della funzione o interruzione) il sistema li termina.
    let _job = job_for(&child);

    let (stdout, out_task) = collect(child.stdout.take().expect("uscita collegata"));
    let (stderr, err_task) = collect(child.stderr.take().expect("errori collegati"));

    let (exit_code, timed_out) = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => (status?.code(), false),
        Err(_) => {
            let _ = child.kill().await;
            (None, true)
        }
    };
    // Un processo avviato dal comando potrebbe tenere aperti i flussi: si
    // aspetta poco, poi si prende ciò che è arrivato.
    let drain = Duration::from_secs(2);
    let _ = tokio::time::timeout(drain, out_task).await;
    let _ = tokio::time::timeout(drain, err_task).await;

    Ok(CommandResult {
        exit_code,
        stdout: take_text(&stdout),
        stderr: decode_clixml(&take_text(&stderr)),
        timed_out,
    })
}

#[cfg(windows)]
fn job_for(child: &tokio::process::Child) -> Option<win32job::Job> {
    let job = win32job::Job::create().ok()?;
    let mut info = job.query_extended_limit_info().ok()?;
    info.limit_kill_on_job_close();
    job.set_extended_limit_info(&info).ok()?;
    job.assign_process(child.raw_handle()? as _).ok()?;
    Some(job)
}

#[cfg(not(windows))]
fn job_for(_child: &tokio::process::Child) -> Option<()> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("core-term-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn un_comando_restituisce_uscita_e_codice() {
        let dir = temp_dir("ok");
        let result = run("echo 'ciao àè'", &dir, Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("ciao àè"), "{result:?}");
        assert!(!result.failed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn il_comando_parte_nella_cartella_indicata() {
        let dir = temp_dir("cwd");
        std::fs::write(dir.join("segnaposto.txt"), "x").unwrap();
        let listing = if cfg!(windows) {
            "Get-ChildItem -Name"
        } else {
            "ls"
        };
        let result = run(listing, &dir, Duration::from_secs(60)).await.unwrap();
        assert!(result.stdout.contains("segnaposto.txt"), "{result:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn il_codice_di_uscita_di_un_programma_esterno_arriva() {
        let dir = temp_dir("exit");
        let command = if cfg!(windows) {
            "cmd /c exit 3"
        } else {
            "exit 3"
        };
        let result = run(command, &dir, Duration::from_secs(60)).await.unwrap();
        assert_eq!(result.exit_code, Some(3));
        assert!(result.failed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn un_comando_troppo_lungo_viene_terminato() {
        let dir = temp_dir("timeout");
        let command = if cfg!(windows) {
            "Start-Sleep -Seconds 30"
        } else {
            "sleep 30"
        };
        let start = std::time::Instant::now();
        let result = run(command, &dir, Duration::from_secs(2)).await.unwrap();
        assert!(result.timed_out);
        assert!(start.elapsed() < Duration::from_secs(15));
        assert!(
            result
                .report(Duration::from_secs(2))
                .contains("tempo massimo")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn gli_errori_di_powershell_arrivano_come_testo() {
        let dir = temp_dir("errori");
        let command = if cfg!(windows) {
            "Get-Content non-esiste.txt"
        } else {
            "cat non-esiste.txt"
        };
        let result = run(command, &dir, Duration::from_secs(60)).await.unwrap();
        assert!(result.failed());
        assert!(!result.stderr.contains("CLIXML"), "{}", result.stderr);
        assert!(!result.stderr.contains("_x000D_"), "{}", result.stderr);
        assert!(
            result.stderr.contains("non-esiste.txt"),
            "{}",
            result.stderr
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn il_clixml_diventa_testo() {
        let xml = "#< CLIXML\r\n<Objs Version=\"1.1.0.1\"><Obj S=\"progress\" RefId=\"0\"></Obj>\
                   <S S=\"Error\">Errore &lt;uno&gt;_x000D__x000A_</S>\
                   <S S=\"Warning\">attento_x000A_</S></Objs>";
        assert_eq!(decode_clixml(xml), "Errore <uno>\nAVVISO: attento\n");
        assert_eq!(decode_clixml("testo normale"), "testo normale");
        assert_eq!(unescape_clixml("a_xZZ_b"), "a_xZZ_b");
    }

    #[test]
    fn un_uscita_lunghissima_viene_accorciata_nel_mezzo() {
        let long = format!("INIZIO{}FINE", "x".repeat(50_000));
        let clipped = clip(&long);
        assert!(clipped.starts_with("INIZIO"));
        assert!(clipped.ends_with("FINE"));
        assert!(clipped.contains("caratteri omessi"));
        assert!(clipped.chars().count() < 21_000);
    }
}
