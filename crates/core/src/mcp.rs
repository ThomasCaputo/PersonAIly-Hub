//! Client MCP (Model Context Protocol) per gli strumenti esterni (AG-7).
//!
//! Un server MCP è un programma che l'utente installa e configura nelle
//! Impostazioni. Due trasporti (ADR 0009):
//!
//! - **stdio**: l'app avvia il server come processo figlio e gli parla in
//!   JSON-RPC sulle sue entrata e uscita standard, una riga per messaggio;
//! - **HTTP** ("streamable HTTP"): il server gira altrove (su questo computer
//!   o su internet) e riceve ogni messaggio con un `POST`; risponde in JSON o
//!   con un flusso di eventi (SSE).
//!
//! Gli strumenti che il server dichiara vengono offerti al modello accanto a
//! quelli dell'app, passando per gli stessi permessi: tutti in classe
//! "esecuzione", perché un server può fare qualunque cosa e le sue indicazioni
//! ("questo strumento legge soltanto") sono sue, non dell'utente. Se il server
//! offre delle **risorse** (documenti, file, schemi…), il modello le vede con
//! due strumenti in più: elencarle e leggerne una.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::chat::SseParser;
use crate::error::{Error, Result};
use crate::tools::{Risk, ToolDef, ToolOutput};

/// Versione del protocollo che l'app conosce.
const PROTOCOL_VERSION: &str = "2025-06-18";
/// Avvio e presentazione: largo, perché `npx` può dover scaricare il server.
const START_TIMEOUT: Duration = Duration::from_secs(90);
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
/// Una chiamata a uno strumento.
const CALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Caratteri del risultato restituiti al modello.
const RESULT_CHARS: usize = 30_000;
/// Righe degli errori del server conservate per spiegare un guasto.
const STDERR_LINES: usize = 20;
/// Byte conservati di ciascuna di quelle righe.
const STDERR_LINE_BYTES: usize = 2_000;
/// Inizio dei nomi degli strumenti MCP come li vede il modello.
pub const PREFIX: &str = "mcp__";
/// Lunghezza massima dei nomi degli strumenti accettata dai servizi.
const NAME_MAX: usize = 64;
/// Gli strumenti dell'app che danno accesso alle risorse di un server.
const LIST_RESOURCES: &str = "list_resources";
const READ_RESOURCE: &str = "read_resource";
/// Risorse elencate al massimo, per non riempire il contesto del modello.
const MAX_RESOURCES: usize = 200;
/// Nel portachiavi le intestazioni HTTP stanno accanto alle variabili
/// d'ambiente, con questo prefisso nel nome.
const HEADER_KEY: &str = "header:";
/// Dimensione massima di un messaggio di un server (una riga stdio, un corpo
/// HTTP, un evento): un server guasto non deve poter riempire la memoria.
const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
/// Del corpo di una risposta di errore basta l'inizio.
const MAX_ERROR_BYTES: usize = 64 * 1024;

fn too_big_message() -> String {
    format!(
        "ha mandato un messaggio più grande di {} MB.",
        MAX_MESSAGE_BYTES / (1024 * 1024)
    )
}

/// Legge una riga (senza `\n` e `\r` finali) di al massimo
/// [`MAX_MESSAGE_BYTES`] byte; `None` a flusso finito. Una riga più lunga è
/// un errore `InvalidData`: i byte letti fin lì sono scartati, il resto
/// della riga arriva con le letture successive.
///
/// `line` conserva la parte di riga già letta: se il futuro viene abbandonato
/// a metà (tempo scaduto, utente che interrompe) quei byte non si perdono e la
/// lettura successiva riprende da lì, come con `Lines::next_line`.
async fn read_line_limited<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    line: &mut Vec<u8>,
) -> std::io::Result<Option<String>> {
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }
        let (used, done) = match available.iter().position(|&b| b == b'\n') {
            Some(end) => (end + 1, true),
            None => (available.len(), false),
        };
        let room = MAX_MESSAGE_BYTES - line.len();
        let take = used.min(room);
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if done && take == used {
            break;
        }
        if line.len() >= MAX_MESSAGE_BYTES {
            line.clear();
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                too_big_message(),
            ));
        }
    }
    let mut line = std::mem::take(line);
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    Ok(Some(String::from_utf8_lossy(&line).into_owned()))
}

/// Legge il corpo di una risposta fino a `limit` byte. Il secondo valore è
/// `false` se il corpo era più lungo ed è stato tagliato.
async fn read_body(
    response: &mut reqwest::Response,
    limit: usize,
) -> reqwest::Result<(Vec<u8>, bool)> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let room = limit - body.len();
        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            return Ok((body, false));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, true))
}

/// Come l'app parla con il server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// Un programma avviato dall'app.
    #[default]
    Stdio,
    /// Un server già in esecuzione, raggiunto a un indirizzo.
    Http,
}

/// Come l'app si fa riconoscere da un server HTTP.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpAuth {
    /// Con le intestazioni scritte dall'utente (o senza nulla).
    #[default]
    Headers,
    /// Con un accesso dal browser (ADR 0024).
    OAuth,
}

/// Nel portachiavi, il segreto del client OAuth indicato dall'utente.
pub(crate) const CLIENT_SECRET_KEY: &str = "oauth:client_secret";

/// Un server MCP configurato dall'utente.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpServerConfig {
    /// Vuoto per un server non ancora salvato.
    pub id: String,
    pub name: String,
    pub transport: Transport,
    /// Solo stdio: il programma da avviare, per esempio `npx` o `C:\strumenti\server.exe`.
    pub command: String,
    pub args: Vec<String>,
    /// Solo stdio: variabili d'ambiente aggiuntive (`NOME`, `valore`).
    pub env: Vec<(String, String)>,
    /// Solo HTTP: l'indirizzo del server, per esempio `https://esempio.it/mcp`.
    pub url: String,
    /// Solo HTTP: intestazioni aggiunte a ogni richiesta, di solito
    /// `Authorization`. I valori stanno nel portachiavi, come le variabili.
    pub headers: Vec<(String, String)>,
    /// Solo HTTP: intestazioni o accesso OAuth.
    pub auth: McpAuth,
    /// Solo OAuth: il client registrato dall'utente presso il servizio, per i
    /// servizi senza registrazione dinamica. Vuoto: l'app si registra da sola.
    pub client_id: String,
    /// Solo OAuth, facoltativo: il segreto di quel client. Sta nel portachiavi.
    pub client_secret: String,
}

impl McpServerConfig {
    /// `true` se il server si raggiunge con un accesso OAuth.
    pub fn uses_oauth(&self) -> bool {
        self.transport == Transport::Http && self.auth == McpAuth::OAuth
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::msg("Dai un nome al server MCP."));
        }
        match self.transport {
            Transport::Stdio => {
                if self.command.trim().is_empty() {
                    return Err(Error::msg("Indica il comando che avvia il server MCP."));
                }
            }
            Transport::Http => check_url(self.url.trim())?,
        }
        if self
            .env
            .iter()
            .any(|(name, _)| name.trim().is_empty() || name.contains('='))
        {
            return Err(Error::msg(
                "Ogni variabile d'ambiente ha bisogno di un nome valido.",
            ));
        }
        let header_name = |name: &str| {
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_graphic() && b != b':')
        };
        if self
            .headers
            .iter()
            .any(|(name, _)| !header_name(name.trim()))
        {
            return Err(Error::msg(
                "Ogni intestazione HTTP ha bisogno di un nome valido (lettere, cifre e trattini).",
            ));
        }
        Ok(())
    }

    /// Le coppie nome/valore da tenere nel portachiavi, con il nome usato là.
    fn secrets_mut(&mut self) -> impl Iterator<Item = (String, &mut String)> {
        let env = self
            .env
            .iter_mut()
            .map(|(name, value)| (name.trim().to_string(), value));
        let headers = self
            .headers
            .iter_mut()
            .map(|(name, value)| (format!("{HEADER_KEY}{}", name.trim()), value));
        let secret = std::iter::once((CLIENT_SECRET_KEY.to_string(), &mut self.client_secret));
        env.chain(headers).chain(secret)
    }

    fn secret_names(&self) -> Vec<String> {
        self.env
            .iter()
            .map(|(name, _)| name.trim().to_string())
            .chain(
                self.headers
                    .iter()
                    .map(|(name, _)| format!("{HEADER_KEY}{}", name.trim())),
            )
            .chain(std::iter::once(CLIENT_SECRET_KEY.to_string()))
            .collect()
    }
}

/// Un indirizzo accettabile per un server MCP: cifrato, oppure su questo
/// computer (dove la cifratura non serve).
pub(crate) fn check_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| Error::msg("Indica l'indirizzo completo del server MCP (https://…)."))?;
    let local = matches!(
        parsed.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if local => Ok(()),
        _ => Err(Error::msg(
            "L'indirizzo del server MCP deve iniziare con https:// (http:// solo per un server su questo computer).",
        )),
    }
}

/// Dove stanno i valori delle variabili d'ambiente dei server: il
/// portachiavi del sistema ([`crate::secrets::KeyringVault`]); nei test, la memoria.
pub trait EnvVault {
    fn get(&self, server_id: &str, name: &str) -> Result<Option<String>>;
    fn set(&self, server_id: &str, name: &str, value: &str) -> Result<()>;
    fn delete(&self, server_id: &str, name: &str) -> Result<()>;
}

/// Toglie i valori delle variabili e delle intestazioni da `config` e li
/// mette nella cassaforte; in archivio restano solo i nomi. `previous` è la
/// configurazione salvata prima: i valori che non ci sono più vengono tolti
/// anche dalla cassaforte.
pub fn seal_env(
    vault: &dyn EnvVault,
    config: &McpServerConfig,
    previous: Option<&McpServerConfig>,
) -> Result<McpServerConfig> {
    let mut sealed = config.clone();
    for (name, value) in sealed.secrets_mut() {
        if value.is_empty() {
            vault.delete(&config.id, &name)?;
        } else {
            vault.set(&config.id, &name, value)?;
        }
        value.clear();
    }
    if let Some(previous) = previous {
        let current = config.secret_names();
        for old in previous.secret_names() {
            if !current.contains(&old) {
                vault.delete(&config.id, &old)?;
            }
        }
    }
    Ok(sealed)
}

/// Rimette nella configurazione i valori presi dalla cassaforte. Un valore
/// che non si trova resta vuoto: il server parte lo stesso e, se gli serviva,
/// lo dice nei suoi errori.
pub fn open_env(vault: &dyn EnvVault, config: &mut McpServerConfig) -> Result<()> {
    let id = config.id.clone();
    for (name, value) in config.secrets_mut() {
        if value.is_empty() {
            *value = vault.get(&id, &name)?.unwrap_or_default();
        }
    }
    Ok(())
}

/// Toglie dalla cassaforte tutti i valori di un server eliminato.
pub fn forget_env(vault: &dyn EnvVault, config: &McpServerConfig) -> Result<()> {
    for name in config.secret_names() {
        vault.delete(&config.id, &name)?;
    }
    Ok(())
}

/// Un argomento di un prompt MCP.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptArg {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// Un prompt offerto da un server (ADR 0025): in chat diventa un comando `/`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPrompt {
    /// Posizione del server nell'insieme della conversazione.
    pub server: usize,
    pub server_name: String,
    pub name: String,
    pub description: String,
    pub arguments: Vec<McpPromptArg>,
}

/// Caratteri massimi del testo di un prompt.
const PROMPT_CHARS: usize = 30_000;

/// Uno strumento dichiarato da un server, per l'interfaccia.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

/// Uno strumento come lo dichiara il server.
#[derive(Debug, Clone)]
struct ServerTool {
    name: String,
    description: String,
    schema: Value,
}

/// Il processo di un server stdio.
struct StdioLink {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// La riga dell'uscita letta solo in parte (vedi [`read_line_limited`]).
    partial: Vec<u8>,
    /// Ultime righe scritte dal server sugli errori.
    stderr: Arc<Mutex<Vec<String>>>,
    #[cfg(windows)]
    _job: Option<win32job::Job>,
}

/// Il collegamento con un server HTTP.
struct HttpLink {
    http: reqwest::Client,
    url: String,
    headers: Vec<(String, String)>,
    /// Assegnato dal server alla presentazione, se ne tiene traccia.
    session: Option<String>,
    /// La versione del protocollo concordata, da ripetere a ogni richiesta.
    protocol: Option<String>,
}

enum Link {
    Stdio(Box<StdioLink>),
    Http(HttpLink),
}

/// Il collegamento con un server in esecuzione.
pub struct McpClient {
    name: String,
    link: Link,
    next_id: u64,
    /// Il server offre delle risorse (`resources/list`, `resources/read`).
    resources: bool,
    /// Il server offre dei prompt (`prompts/list`, `prompts/get`).
    prompts: bool,
}

impl McpClient {
    /// Avvia il server (o lo raggiunge) e completa la presentazione (`initialize`).
    pub async fn start(config: &McpServerConfig, cwd: Option<&Path>) -> Result<Self> {
        let link = match config.transport {
            Transport::Stdio => Link::Stdio(Box::new(spawn(config, cwd)?)),
            Transport::Http => {
                check_url(config.url.trim())?;
                Link::Http(HttpLink {
                    http: reqwest::Client::builder()
                        .connect_timeout(Duration::from_secs(20))
                        .build()?,
                    url: config.url.trim().to_string(),
                    headers: config
                        .headers
                        .iter()
                        .map(|(name, value)| (name.trim().to_string(), value.clone()))
                        .collect(),
                    session: None,
                    protocol: None,
                })
            }
        };
        let mut client = Self {
            name: config.name.trim().to_string(),
            link,
            next_id: 1,
            resources: false,
            prompts: false,
        };
        let result = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "personaily-hub", "version": crate::version() }
                }),
                START_TIMEOUT,
            )
            .await?;
        client.resources = result.pointer("/capabilities/resources").is_some();
        client.prompts = result.pointer("/capabilities/prompts").is_some();
        if let Link::Http(http) = &mut client.link {
            http.protocol = Some(
                result
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(PROTOCOL_VERSION)
                    .to_string(),
            );
        }
        client
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(client)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// `false` se il processo di un server stdio si è chiuso. Di un server
    /// HTTP non si sa finché non si prova a parlargli.
    fn is_alive(&mut self) -> bool {
        match &mut self.link {
            Link::Stdio(stdio) => matches!(stdio.child.try_wait(), Ok(None)),
            Link::Http(_) => true,
        }
    }

    /// Spiega un guasto: il messaggio e, se c'è, ciò che il server ha scritto sugli errori.
    fn failure(&mut self, message: &str) -> Error {
        let mut text = format!("Server MCP «{}»: {message}", self.name);
        if let Link::Stdio(stdio) = &mut self.link {
            if matches!(stdio.child.try_wait(), Ok(Some(_))) {
                text.push_str(" Il server si è chiuso.");
            }
            let tail = stdio
                .stderr
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .join("\n");
            if !tail.trim().is_empty() {
                text.push('\n');
                text.push_str(tail.trim());
            }
        }
        Error::msg(text)
    }

    /// Solo stdio: scrive un messaggio sull'entrata del server.
    async fn send_line(&mut self, message: &Value) -> Result<()> {
        let mut line = serde_json::to_string(message)?;
        line.push('\n');
        let Link::Stdio(stdio) = &mut self.link else {
            return Ok(());
        };
        if stdio.stdin.write_all(line.as_bytes()).await.is_err()
            || stdio.stdin.flush().await.is_err()
        {
            return Err(self.failure("non riceve più messaggi."));
        }
        Ok(())
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        match &self.link {
            Link::Stdio(_) => self.send_line(&message).await,
            Link::Http(_) => {
                // Il server risponde 202 senza contenuto: basta che accetti.
                let response = self.post(&message, LIST_TIMEOUT).await?;
                if !response.status().is_success() {
                    return Err(self.http_failure(response).await);
                }
                Ok(())
            }
        }
    }

    /// Manda una richiesta e aspetta la sua risposta. Intanto risponde alle
    /// richieste del server (`ping`) e ignora le sue notifiche.
    async fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        match &self.link {
            Link::Stdio(_) => self.request_stdio(id, method, &message, timeout).await,
            Link::Http(_) => self.request_http(id, method, &message, timeout).await,
        }
    }

    async fn request_stdio(
        &mut self,
        id: u64,
        method: &str,
        message: &Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.send_line(message).await?;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let Link::Stdio(stdio) = &mut self.link else {
                unreachable!("collegamento stdio");
            };
            let stdio = &mut **stdio;
            let reading = read_line_limited(&mut stdio.stdout, &mut stdio.partial);
            let line = match tokio::time::timeout_at(deadline, reading).await {
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::InvalidData => {
                    return Err(self.failure(&too_big_message()));
                }
                Err(_) => {
                    return Err(self.failure(&format!(
                        "nessuna risposta a «{method}» entro {} secondi.",
                        timeout.as_secs()
                    )));
                }
                Ok(Ok(Some(line))) => line,
                Ok(_) => return Err(self.failure("ha chiuso il collegamento.")),
            };
            // Qualche server scrive righe di servizio sull'uscita: si saltano.
            let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if let Some(reply) = reply_to_server(&message) {
                self.send_line(&reply).await?;
                continue;
            }
            if let Some(result) = self.response_for(id, &message) {
                return result;
            }
        }
    }

    /// Se il messaggio è la risposta alla richiesta `id`, il suo risultato.
    fn response_for(&self, id: u64, message: &Value) -> Option<Result<Value>> {
        if message.get("method").is_some() || message.get("id").and_then(Value::as_u64) != Some(id)
        {
            return None;
        }
        if let Some(error) = message.get("error") {
            let text = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("errore sconosciuto");
            return Some(Err(Error::msg(format!(
                "Server MCP «{}»: {text}",
                self.name
            ))));
        }
        Some(Ok(message.get("result").cloned().unwrap_or(Value::Null)))
    }

    /// Solo HTTP: manda un messaggio con un `POST`.
    async fn post(&mut self, message: &Value, timeout: Duration) -> Result<reqwest::Response> {
        let Link::Http(http) = &self.link else {
            unreachable!("collegamento HTTP");
        };
        let mut request = http
            .http
            .post(&http.url)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .timeout(timeout)
            .json(message);
        for (name, value) in &http.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if let Some(session) = &http.session {
            request = request.header("Mcp-Session-Id", session);
        }
        if let Some(protocol) = &http.protocol {
            request = request.header("MCP-Protocol-Version", protocol);
        }
        match request.send().await {
            Ok(response) => Ok(response),
            Err(e) => Err(self.failure(&format!("non raggiungibile ({e})."))),
        }
    }

    async fn http_failure(&mut self, mut response: reqwest::Response) -> Error {
        let status = response.status();
        let (bytes, _) = read_body(&mut response, MAX_ERROR_BYTES)
            .await
            .unwrap_or_default();
        let body = String::from_utf8_lossy(&bytes);
        let detail: String = body.trim().chars().take(300).collect();
        let hint = match status.as_u16() {
            401 | 403 => {
                " Controlla le intestazioni (per esempio il token in Authorization), oppure scegli «Accesso dal browser (OAuth)» e fai l'accesso."
            }
            404 => " Controlla l'indirizzo.",
            _ => "",
        };
        self.failure(&format!("ha risposto {status}.{hint} {detail}"))
    }

    async fn request_http(
        &mut self,
        id: u64,
        method: &str,
        message: &Value,
        timeout: Duration,
    ) -> Result<Value> {
        let mut response = self.post(message, timeout).await?;
        if !response.status().is_success() {
            return Err(self.http_failure(response).await);
        }
        if let Some(session) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            && let Link::Http(http) = &mut self.link
        {
            http.session = Some(session.to_string());
        }
        let is_stream = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("text/event-stream"));
        if !is_stream {
            let bytes = match read_body(&mut response, MAX_MESSAGE_BYTES).await {
                Ok((bytes, true)) => bytes,
                Ok((_, false)) => return Err(self.failure(&too_big_message())),
                Err(e) => return Err(self.failure(&format!("risposta illeggibile ({e})."))),
            };
            let body: Value = serde_json::from_slice(&bytes)
                .map_err(|e| self.failure(&format!("risposta illeggibile ({e}).")))?;
            // Una risposta sola, o un gruppo di messaggi.
            let messages = match body {
                Value::Array(items) => items,
                other => vec![other],
            };
            for message in &messages {
                if let Some(result) = self.response_for(id, message) {
                    return result;
                }
            }
            return Err(self.failure(&format!("nessuna risposta a «{method}».")));
        }

        // Flusso di eventi: arrivano notifiche, richieste del server e, alla
        // fine, la risposta.
        let deadline = tokio::time::Instant::now() + timeout;
        let mut parser = SseParser::default();
        loop {
            let chunk = match tokio::time::timeout_at(deadline, response.chunk()).await {
                Err(_) => {
                    return Err(self.failure(&format!(
                        "nessuna risposta a «{method}» entro {} secondi.",
                        timeout.as_secs()
                    )));
                }
                Ok(Ok(Some(chunk))) => chunk,
                Ok(Ok(None)) => {
                    return Err(self.failure(&format!(
                        "ha chiuso il flusso senza rispondere a «{method}»."
                    )));
                }
                Ok(Err(e)) => return Err(self.failure(&format!("collegamento interrotto ({e})."))),
            };
            let events = parser.push(&chunk);
            // Un evento senza fine resterebbe tutto in memoria.
            if parser.pending() > MAX_MESSAGE_BYTES {
                return Err(self.failure(&too_big_message()));
            }
            for data in events {
                let Ok(message) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };
                if let Some(reply) = reply_to_server(&message) {
                    let _ = self.post(&reply, LIST_TIMEOUT).await;
                    continue;
                }
                if let Some(result) = self.response_for(id, &message) {
                    return result;
                }
            }
        }
    }

    async fn list_tools(&mut self) -> Result<Vec<ServerTool>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        // Un server che pagina all'infinito non deve bloccare l'app.
        for _ in 0..20 {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let result = self.request("tools/list", params, LIST_TIMEOUT).await?;
            for tool in result
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(name) = tool.get("name").and_then(Value::as_str) else {
                    continue;
                };
                tools.push(ServerTool {
                    name: name.to_string(),
                    description: tool
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    schema: object_schema(tool.get("inputSchema")),
                });
            }
            cursor = next_cursor(&result);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    /// Gli strumenti del server, per l'interfaccia; con le risorse, anche i
    /// due strumenti dell'app che le leggono.
    pub async fn tool_infos(&mut self) -> Result<Vec<McpToolInfo>> {
        let mut infos: Vec<McpToolInfo> = self
            .list_tools()
            .await?
            .into_iter()
            .map(|t| McpToolInfo {
                name: t.name,
                description: t.description,
            })
            .collect();
        if self.resources {
            for def in resource_tools() {
                infos.push(McpToolInfo {
                    name: def.name.into_owned(),
                    description: def.description.into_owned(),
                });
            }
        }
        Ok(infos)
    }

    async fn call(&mut self, tool: &str, arguments: &Value) -> Result<ToolOutput> {
        let result = self
            .request(
                "tools/call",
                json!({ "name": tool, "arguments": arguments }),
                CALL_TIMEOUT,
            )
            .await?;
        Ok(tool_output(&result))
    }

    /// Le risorse del server, una per riga: indirizzo, nome e descrizione.
    /// I prompt del server, senza ancora il loro testo.
    async fn list_prompts(&mut self) -> Result<Vec<(String, String, Vec<McpPromptArg>)>> {
        let mut prompts = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..10 {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let result = self.request("prompts/list", params, LIST_TIMEOUT).await?;
            for prompt in result
                .get("prompts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let text = |v: &Value, key: &str| {
                    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
                };
                let name = text(prompt, "name");
                if name.is_empty() {
                    continue;
                }
                let arguments = prompt
                    .get("arguments")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|a| !text(a, "name").is_empty())
                    .map(|a| McpPromptArg {
                        name: text(a, "name"),
                        description: text(a, "description"),
                        required: a.get("required").and_then(Value::as_bool).unwrap_or(false),
                    })
                    .collect();
                let description = match text(prompt, "description") {
                    d if d.is_empty() => text(prompt, "title"),
                    d => d,
                };
                prompts.push((name, description, arguments));
            }
            cursor = next_cursor(&result);
            if cursor.is_none() {
                break;
            }
        }
        Ok(prompts)
    }

    /// Il testo di un prompt con i suoi argomenti: i messaggi uniti, in ordine.
    async fn get_prompt(
        &mut self,
        name: &str,
        arguments: &std::collections::HashMap<String, String>,
    ) -> Result<String> {
        let result = self
            .request(
                "prompts/get",
                json!({ "name": name, "arguments": arguments }),
                CALL_TIMEOUT,
            )
            .await?;
        Ok(prompt_text(&result))
    }

    async fn list_resources(&mut self) -> Result<ToolOutput> {
        let mut lines = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..10 {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let result = self.request("resources/list", params, LIST_TIMEOUT).await?;
            for resource in result
                .get("resources")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let get = |key: &str| resource.get(key).and_then(Value::as_str).unwrap_or("");
                let mut line = format!("- {}", get("uri"));
                for (label, key) in [("", "name"), ("", "description"), ("tipo ", "mimeType")] {
                    if !get(key).is_empty() {
                        line.push_str(&format!(" — {label}{}", get(key)));
                    }
                }
                lines.push(line);
            }
            cursor = next_cursor(&result);
            if cursor.is_none() || lines.len() >= MAX_RESOURCES {
                break;
            }
        }
        if lines.is_empty() {
            return Ok(ToolOutput::ok("Il server non offre risorse."));
        }
        let more = lines.len() > MAX_RESOURCES;
        lines.truncate(MAX_RESOURCES);
        let mut text = lines.join("\n");
        if more {
            text.push_str("\n… e altre non mostrate.");
        }
        Ok(ToolOutput::ok(text))
    }

    async fn read_resource(&mut self, uri: &str) -> Result<ToolOutput> {
        let result = self
            .request("resources/read", json!({ "uri": uri }), CALL_TIMEOUT)
            .await?;
        let mut parts = Vec::new();
        for item in result
            .get("contents")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match item.get("text").and_then(Value::as_str) {
                Some(text) => parts.push(text.to_string()),
                None => parts.push(format!(
                    "[Contenuto binario di {} ({}): non mostrato al modello]",
                    item.get("uri").and_then(Value::as_str).unwrap_or(uri),
                    item.get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("tipo sconosciuto")
                )),
            }
        }
        let mut content = parts.join("\n\n");
        if content.chars().count() > RESULT_CHARS {
            content = content.chars().take(RESULT_CHARS).collect();
            content.push_str("\n\n[… risorsa troncata …]");
        }
        if content.is_empty() {
            content = "(risorsa vuota)".to_string();
        }
        Ok(ToolOutput::ok(content))
    }
}

/// Avvia il processo di un server stdio.
fn spawn(config: &McpServerConfig, cwd: Option<&Path>) -> Result<StdioLink> {
    let program = resolve_program(config.command.trim());
    let mut cmd = Command::new(&program);
    cmd.args(&config.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (name, value) in &config.env {
        cmd.env(name.trim(), value);
    }
    if let Some(dir) = cwd.filter(|d| d.is_dir()) {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    cmd.creation_flags(crate::local::runtime::CREATE_NO_WINDOW);

    let mut child = cmd.spawn().map_err(|e| {
        Error::msg(format!(
            "Impossibile avviare il server MCP «{}» ({}): {e}",
            config.name,
            program.display()
        ))
    })?;
    #[cfg(windows)]
    let job = job_for(&child);

    let stdin = child.stdin.take().expect("entrata collegata");
    let stdout = BufReader::new(child.stdout.take().expect("uscita collegata"));
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let sink = stderr.clone();
    let mut err_reader = BufReader::new(child.stderr.take().expect("errori collegati"));
    tokio::spawn(async move {
        // Il flusso va svuotato fino in fondo, anche se una riga è enorme:
        // altrimenti il server si blocca quando il canale è pieno.
        let mut partial = Vec::new();
        loop {
            let mut line = match read_line_limited(&mut err_reader, &mut partial).await {
                Ok(Some(line)) => line,
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => continue,
                _ => break,
            };
            // Servono solo a spiegare un guasto: di ognuna basta l'inizio.
            if line.len() > STDERR_LINE_BYTES {
                let mut cut = STDERR_LINE_BYTES;
                while !line.is_char_boundary(cut) {
                    cut -= 1;
                }
                line.truncate(cut);
                line.push('…');
            }
            let mut lines = sink.lock().unwrap_or_else(|e| e.into_inner());
            lines.push(line);
            if lines.len() > STDERR_LINES {
                lines.remove(0);
            }
        }
    });
    Ok(StdioLink {
        child,
        stdin,
        stdout,
        partial: Vec::new(),
        stderr,
        #[cfg(windows)]
        _job: job,
    })
}

/// La risposta a una richiesta del server (`ping`); `None` se il messaggio
/// non è una richiesta.
fn reply_to_server(message: &Value) -> Option<Value> {
    let request_id = message.get("id")?;
    let method = message.get("method")?;
    Some(if method == "ping" {
        json!({ "jsonrpc": "2.0", "id": request_id, "result": {} })
    } else {
        json!({ "jsonrpc": "2.0", "id": request_id,
                "error": { "code": -32601, "message": "Metodo non supportato" } })
    })
}

/// Il testo dei messaggi di un prompt. Quelli dell'assistente sono segnati,
/// immagini e audio solo nominati.
fn prompt_text(result: &Value) -> String {
    let mut parts = Vec::new();
    for message in result
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let content = message.get("content").cloned().unwrap_or(Value::Null);
        let text = match content.get("type").and_then(Value::as_str) {
            Some("text") => content
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            Some("resource") => content
                .pointer("/resource/text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "[risorsa {}]",
                        content
                            .pointer("/resource/uri")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                    )
                }),
            Some("image") => "[immagine]".to_string(),
            Some("audio") => "[audio]".to_string(),
            _ => String::new(),
        };
        if text.trim().is_empty() {
            continue;
        }
        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            parts.push(format!("[assistente] {text}"));
        } else {
            parts.push(text);
        }
    }
    let mut text = parts.join("\n\n");
    if text.chars().count() > PROMPT_CHARS {
        text = text.chars().take(PROMPT_CHARS).collect();
    }
    text
}

fn next_cursor(result: &Value) -> Option<String> {
    result
        .get("nextCursor")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
}

/// I due strumenti con cui il modello usa le risorse di un server.
fn resource_tools() -> [ToolDef; 2] {
    [
        ToolDef {
            name: LIST_RESOURCES.into(),
            description: "Elenca le risorse (documenti, file, dati) che il server mette a \
                          disposizione, con il loro indirizzo."
                .into(),
            schema: json!({ "type": "object", "properties": {} }),
            risk: Risk::Execute,
        },
        ToolDef {
            name: READ_RESOURCE.into(),
            description: "Legge una risorsa del server dato il suo indirizzo (uri), preso da \
                          list_resources."
                .into(),
            schema: json!({
                "type": "object",
                "properties": {
                    "uri": { "type": "string", "description": "L'indirizzo della risorsa." }
                },
                "required": ["uri"]
            }),
            risk: Risk::Execute,
        },
    ]
}

/// Lo schema dei parametri, sempre come oggetto: i servizi non ne accettano altri.
fn object_schema(schema: Option<&Value>) -> Value {
    match schema {
        Some(Value::Object(map)) if map.get("type").and_then(Value::as_str) == Some("object") => {
            let mut schema = Value::Object(map.clone());
            if schema.get("properties").is_none() {
                schema["properties"] = json!({});
            }
            schema
        }
        _ => json!({ "type": "object", "properties": {} }),
    }
}

/// Il risultato di `tools/call` come testo per il modello.
fn tool_output(result: &Value) -> ToolOutput {
    let mut parts = Vec::new();
    for item in result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let text = match item.get("type").and_then(Value::as_str) {
            Some("text") => item.get("text").and_then(Value::as_str).map(str::to_string),
            Some("image") => {
                Some("[Immagine restituita dal server: non mostrata al modello]".into())
            }
            Some("audio") => Some("[Audio restituito dal server: non mostrato al modello]".into()),
            Some("resource") => item.get("resource").map(|r| {
                r.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        format!(
                            "[Risorsa {}]",
                            r.get("uri").and_then(Value::as_str).unwrap_or("?")
                        )
                    })
            }),
            Some("resource_link") => Some(format!(
                "[Collegamento a {}]",
                item.get("uri").and_then(Value::as_str).unwrap_or("?")
            )),
            _ => None,
        };
        parts.extend(text);
    }
    if parts.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        parts.push(structured.to_string());
    }
    let mut content = parts.join("\n\n");
    if content.chars().count() > RESULT_CHARS {
        content = content.chars().take(RESULT_CHARS).collect();
        content.push_str("\n\n[… risultato troncato …]");
    }
    if content.is_empty() {
        content = "(nessun risultato)".to_string();
    }
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        ToolOutput::error(content)
    } else {
        ToolOutput::ok(content)
    }
}

/// Su Windows `npx` è in realtà `npx.cmd`: si cerca nel `PATH` il file vero,
/// con le estensioni di `PATHEXT`, così anche i comandi `.cmd` partono.
fn resolve_program(command: &str) -> PathBuf {
    let path = Path::new(command);
    if !cfg!(windows) || path.extension().is_some() || path.components().count() > 1 {
        return path.to_path_buf();
    }
    let extensions: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(str::to_lowercase)
        .collect();
    let dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    for dir in dirs {
        for ext in &extensions {
            let candidate = dir.join(format!("{command}{ext}"));
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    path.to_path_buf()
}

#[cfg(windows)]
fn job_for(child: &Child) -> Option<win32job::Job> {
    let job = win32job::Job::create().ok()?;
    let mut info = job.query_extended_limit_info().ok()?;
    info.limit_kill_on_job_close();
    job.set_extended_limit_info(&info).ok()?;
    job.assign_process(child.raw_handle()? as _).ok()?;
    Some(job)
}

/// Il nome con cui il modello vede uno strumento: `mcp__server__strumento`,
/// solo con i caratteri ammessi dai servizi.
fn exposed_name(server: &str, tool: &str) -> String {
    let clean = |text: &str| -> String {
        text.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let mut name = format!("{PREFIX}{}__{}", clean(server), clean(tool));
    name.truncate(NAME_MAX);
    name
}

/// Che cosa fa uno strumento offerto al modello.
enum Action {
    /// Uno strumento del server, con il suo nome.
    Tool(String),
    ListResources,
    ReadResource,
}

/// Uno strumento offerto al modello, con il server che lo esegue.
struct Entry {
    exposed: String,
    client: usize,
    server: String,
    action: Action,
}

/// I server MCP di una sessione, avviati insieme e chiusi insieme.
#[derive(Default)]
pub struct McpSet {
    clients: Vec<tokio::sync::Mutex<McpClient>>,
    entries: Vec<Entry>,
    defs: Vec<ToolDef>,
}

impl McpSet {
    /// Avvia i server e raccoglie i loro strumenti. Se uno non parte si
    /// ferma tutto: meglio un errore chiaro che un agente senza i suoi strumenti.
    pub async fn start(configs: &[McpServerConfig], cwd: Option<&Path>) -> Result<Self> {
        let mut set = Self::default();
        for config in configs {
            let mut client = McpClient::start(config, cwd).await?;
            let index = set.clients.len();
            let mut offered: Vec<(ServerTool, Action)> = client
                .list_tools()
                .await?
                .into_iter()
                .map(|tool| {
                    let action = Action::Tool(tool.name.clone());
                    (tool, action)
                })
                .collect();
            if client.resources {
                let [list, read] = resource_tools();
                for (def, action) in [(list, Action::ListResources), (read, Action::ReadResource)] {
                    offered.push((
                        ServerTool {
                            name: def.name.into_owned(),
                            description: def.description.into_owned(),
                            schema: def.schema,
                        },
                        action,
                    ));
                }
            }
            for (tool, action) in offered {
                let mut exposed = exposed_name(&config.name, &tool.name);
                // Due strumenti con lo stesso nome ripulito: il secondo prende un numero.
                let mut n = 2;
                while set.entries.iter().any(|e| e.exposed == exposed) {
                    exposed = format!("{}_{n}", exposed_name(&config.name, &tool.name));
                    n += 1;
                }
                let description = if tool.description.is_empty() {
                    format!("Strumento del server MCP «{}».", client.name())
                } else {
                    format!("{} (server MCP «{}»)", tool.description, client.name())
                };
                set.defs.push(ToolDef {
                    name: Cow::Owned(exposed.clone()),
                    description: Cow::Owned(description),
                    schema: tool.schema,
                    risk: Risk::Execute,
                });
                set.entries.push(Entry {
                    exposed,
                    client: index,
                    server: client.name().to_string(),
                    action,
                });
            }
            set.clients.push(tokio::sync::Mutex::new(client));
        }
        Ok(set)
    }

    pub fn defs(&self) -> &[ToolDef] {
        &self.defs
    }

    /// I prompt di tutti i server dell'insieme. Un server che non li sa
    /// elencare viene saltato: gli altri restano utilizzabili.
    pub async fn prompts(&self) -> Vec<McpPrompt> {
        let mut out = Vec::new();
        for (index, client) in self.clients.iter().enumerate() {
            let mut client = client.lock().await;
            if !client.prompts {
                continue;
            }
            match client.list_prompts().await {
                Ok(list) => {
                    out.extend(
                        list.into_iter()
                            .map(|(name, description, arguments)| McpPrompt {
                                server: index,
                                server_name: client.name().to_string(),
                                name,
                                description,
                                arguments,
                            }),
                    )
                }
                Err(e) => crate::log::error(format!("prompt MCP non elencati: {e}")),
            }
        }
        out
    }

    /// Il testo di un prompt di uno dei server.
    pub async fn get_prompt(
        &self,
        server: usize,
        name: &str,
        arguments: &std::collections::HashMap<String, String>,
    ) -> Result<String> {
        let client = self
            .clients
            .get(server)
            .ok_or_else(|| Error::msg("Il server di questo prompt non è più attivo."))?;
        client.lock().await.get_prompt(name, arguments).await
    }

    /// `true` se tutti i server stdio sono ancora in esecuzione. Un server al
    /// lavoro in questo momento conta come vivo.
    pub fn is_alive(&self) -> bool {
        self.clients
            .iter()
            .all(|client| client.try_lock().map_or(true, |mut c| c.is_alive()))
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.exposed == name)
    }

    /// Descrizione breve della chiamata, per la conferma e il registro.
    pub fn summary(&self, name: &str) -> Option<String> {
        let entry = self.entries.iter().find(|e| e.exposed == name)?;
        Some(match &entry.action {
            Action::Tool(tool) => format!("Usare «{tool}» del server MCP «{}»", entry.server),
            Action::ListResources => {
                format!("Elencare le risorse del server MCP «{}»", entry.server)
            }
            Action::ReadResource => {
                format!("Leggere una risorsa del server MCP «{}»", entry.server)
            }
        })
    }

    pub async fn call(&self, name: &str, arguments: &Value) -> ToolOutput {
        let Some(entry) = self.entries.iter().find(|e| e.exposed == name) else {
            return ToolOutput::error(format!("Lo strumento \"{name}\" non è disponibile."));
        };
        let mut client = self.clients[entry.client].lock().await;
        let result = match &entry.action {
            Action::Tool(tool) => client.call(tool, arguments).await,
            Action::ListResources => client.list_resources().await,
            Action::ReadResource => match arguments.get("uri").and_then(Value::as_str) {
                Some(uri) if !uri.trim().is_empty() => client.read_resource(uri.trim()).await,
                _ => Ok(ToolOutput::error("Manca il parametro \"uri\".")),
            },
        };
        match result {
            Ok(output) => output,
            Err(error) => ToolOutput::error(error.to_string()),
        }
    }
}

/// Avvia un server, ne elenca gli strumenti e lo chiude: per il pulsante "Prova".
pub async fn probe(config: &McpServerConfig) -> Result<Vec<McpToolInfo>> {
    config.validate()?;
    let mut client = McpClient::start(config, None).await?;
    client.tool_infos().await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[tokio::test]
    async fn le_righe_troppo_lunghe_si_rifiutano() {
        let text = b"{\"a\":1}\r\nseconda\nultima senza a capo".to_vec();
        let mut reader = BufReader::new(&text[..]);
        let mut partial = Vec::new();
        let mut lines = Vec::new();
        while let Some(line) = read_line_limited(&mut reader, &mut partial).await.unwrap() {
            lines.push(line);
        }
        assert_eq!(lines, ["{\"a\":1}", "seconda", "ultima senza a capo"]);

        // Oltre il limite è un errore, e la riga dopo si legge ancora.
        let mut huge = vec![b'x'; MAX_MESSAGE_BYTES + 10];
        huge.extend_from_slice(b"\ndopo\n");
        let mut reader = BufReader::new(&huge[..]);
        let error = read_line_limited(&mut reader, &mut partial)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        // Ciò che era stato letto non resta in memoria.
        assert!(partial.is_empty());
        // Il resto della riga lunga, poi quella dopo.
        let mut next = async || read_line_limited(&mut reader, &mut partial).await.unwrap();
        assert_eq!(next().await.unwrap(), "x".repeat(10));
        assert_eq!(next().await.unwrap(), "dopo");
        assert!(next().await.is_none());
    }

    /// Una lettura abbandonata a metà riga (tempo scaduto) non perde i byte:
    /// la successiva restituisce la riga intera.
    #[tokio::test]
    async fn una_lettura_interrotta_non_perde_la_riga() {
        let (mut server, client) = tokio::io::duplex(64);
        let mut reader = BufReader::new(client);
        let mut partial = Vec::new();
        server.write_all(b"{\"id\":1,").await.unwrap();
        let first = tokio::time::timeout(
            Duration::from_millis(100),
            read_line_limited(&mut reader, &mut partial),
        )
        .await;
        assert!(first.is_err(), "doveva scadere il tempo");
        server.write_all(b"\"result\":{}}\n").await.unwrap();
        assert_eq!(
            read_line_limited(&mut reader, &mut partial)
                .await
                .unwrap()
                .unwrap(),
            "{\"id\":1,\"result\":{}}"
        );
    }

    #[test]
    fn i_nomi_esposti_sono_validi_per_i_servizi() {
        assert_eq!(
            exposed_name("File system", "read.file"),
            "mcp__File_system__read_file"
        );
        let long = exposed_name(&"s".repeat(50), &"t".repeat(50));
        assert_eq!(long.len(), NAME_MAX);
        assert!(
            long.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        );
    }

    #[test]
    fn lo_schema_e_sempre_un_oggetto() {
        assert_eq!(
            object_schema(None),
            json!({"type": "object", "properties": {}})
        );
        assert_eq!(
            object_schema(Some(&json!({"type": "object"}))),
            json!({"type": "object", "properties": {}})
        );
        let full =
            json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]});
        assert_eq!(object_schema(Some(&full)), full);
    }

    #[test]
    fn il_risultato_diventa_testo() {
        let ok = tool_output(&json!({"content": [
            {"type": "text", "text": "uno"},
            {"type": "image", "data": "AAAA", "mimeType": "image/png"},
            {"type": "resource", "resource": {"uri": "file:///a", "text": "due"}}
        ]}));
        assert!(!ok.is_error);
        assert!(ok.content.starts_with("uno"));
        assert!(ok.content.contains("Immagine"));
        assert!(ok.content.ends_with("due"));

        let failed =
            tool_output(&json!({"content": [{"type": "text", "text": "no"}], "isError": true}));
        assert!(failed.is_error);
        let structured = tool_output(&json!({"content": [], "structuredContent": {"n": 1}}));
        assert_eq!(structured.content, "{\"n\":1}");
    }

    /// Cassaforte in memoria, al posto del portachiavi.
    #[derive(Default)]
    pub struct MemoryVault(pub Mutex<std::collections::HashMap<String, String>>);

    impl EnvVault for MemoryVault {
        fn get(&self, server_id: &str, name: &str) -> Result<Option<String>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&format!("{server_id}:{name}"))
                .cloned())
        }
        fn set(&self, server_id: &str, name: &str, value: &str) -> Result<()> {
            self.0
                .lock()
                .unwrap()
                .insert(format!("{server_id}:{name}"), value.to_string());
            Ok(())
        }
        fn delete(&self, server_id: &str, name: &str) -> Result<()> {
            self.0
                .lock()
                .unwrap()
                .remove(&format!("{server_id}:{name}"));
            Ok(())
        }
    }

    #[test]
    fn le_variabili_stanno_nella_cassaforte_e_non_in_archivio() {
        let vault = MemoryVault::default();
        let config = McpServerConfig {
            id: "srv".into(),
            name: "Prova".into(),
            command: "npx".into(),
            env: vec![
                ("TOKEN".into(), "segreto".into()),
                ("VUOTA".into(), "".into()),
            ],
            ..Default::default()
        };
        let sealed = seal_env(&vault, &config, None).unwrap();
        assert_eq!(
            sealed.env,
            [("TOKEN".into(), "".into()), ("VUOTA".into(), "".into())]
        );
        assert!(!serde_json::to_string(&sealed).unwrap().contains("segreto"));
        assert_eq!(vault.0.lock().unwrap().len(), 1);

        let mut opened = sealed.clone();
        open_env(&vault, &mut opened).unwrap();
        assert_eq!(opened.env, config.env);

        // Tolta una variabile, sparisce anche dalla cassaforte.
        let mut changed = config.clone();
        changed.env = vec![("ALTRA".into(), "x".into())];
        seal_env(&vault, &changed, Some(&sealed)).unwrap();
        assert_eq!(vault.get("srv", "TOKEN").unwrap(), None);
        assert_eq!(vault.get("srv", "ALTRA").unwrap().as_deref(), Some("x"));

        forget_env(&vault, &changed).unwrap();
        assert!(vault.0.lock().unwrap().is_empty());
    }

    #[test]
    fn la_configurazione_va_compilata() {
        let mut config = McpServerConfig {
            name: "Prova".into(),
            command: "npx".into(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
        config.env = vec![("".into(), "x".into())];
        assert!(config.validate().is_err());
        config.env.clear();
        config.command = " ".into();
        assert!(config.validate().is_err());
    }

    /// Un server MCP finto, scritto in PowerShell: risponde a `initialize`,
    /// `tools/list` e `tools/call` (strumento `echo`), e manda un `ping`.
    #[cfg(windows)]
    pub(crate) fn fake_server(dir: &Path) -> McpServerConfig {
        let script = r#"
$ErrorActionPreference = 'Stop'
$stdin = [Console]::In
while ($true) {
  $line = $stdin.ReadLine()
  if ($null -eq $line) { break }
  $msg = $line | ConvertFrom-Json
  if ($null -eq $msg.id) { continue }
  switch ($msg.method) {
    'initialize' {
      [Console]::Out.WriteLine('rumore che non e JSON')
      $r = @{ jsonrpc = '2.0'; id = $msg.id; result = @{ protocolVersion = '2025-06-18'; capabilities = @{ tools = @{} }; serverInfo = @{ name = 'finto'; version = '1' } } }
    }
    'tools/list' {
      [Console]::Out.WriteLine((@{ jsonrpc = '2.0'; id = 'p1'; method = 'ping' } | ConvertTo-Json -Compress))
      $r = @{ jsonrpc = '2.0'; id = $msg.id; result = @{ tools = @(@{ name = 'echo'; description = 'Ripete il testo'; inputSchema = @{ type = 'object'; properties = @{ text = @{ type = 'string' } }; required = @('text') } }) } }
    }
    'tools/call' {
      $r = @{ jsonrpc = '2.0'; id = $msg.id; result = @{ content = @(@{ type = 'text'; text = ('eco: ' + $msg.params.arguments.text) }) } }
    }
    default { $r = @{ jsonrpc = '2.0'; id = $msg.id; result = @{} } }
  }
  [Console]::Out.WriteLine(($r | ConvertTo-Json -Compress -Depth 10))
  [Console]::Out.Flush()
}
"#;
        let path = dir.join("server.ps1");
        std::fs::write(&path, script).unwrap();
        McpServerConfig {
            name: "Finto".into(),
            command: "powershell.exe".into(),
            args: vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                path.to_string_lossy().into_owned(),
            ],
            ..Default::default()
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn parla_con_un_server_vero() {
        let dir = std::env::temp_dir().join(format!("core-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = fake_server(&dir);

        let tools = probe(&config).await.unwrap();
        assert_eq!(
            tools,
            [McpToolInfo {
                name: "echo".into(),
                description: "Ripete il testo".into()
            }]
        );

        let set = McpSet::start(std::slice::from_ref(&config), Some(&dir))
            .await
            .unwrap();
        assert_eq!(set.defs().len(), 1);
        assert_eq!(set.defs()[0].name, "mcp__Finto__echo");
        assert_eq!(set.defs()[0].risk, Risk::Execute);
        assert!(set.has("mcp__Finto__echo"));
        assert!(set.summary("mcp__Finto__echo").unwrap().contains("«echo»"));
        let output = set.call("mcp__Finto__echo", &json!({"text": "ciao"})).await;
        assert_eq!(output, ToolOutput::ok("eco: ciao"));
        assert!(set.call("mcp__Finto__altro", &json!({})).await.is_error);
        drop(set);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn un_server_inesistente_da_un_errore_chiaro() {
        let config = McpServerConfig {
            name: "Fantasma".into(),
            command: "programma-che-non-esiste-davvero".into(),
            ..Default::default()
        };
        let error = probe(&config).await.err().unwrap().to_string();
        assert!(error.contains("Fantasma"), "{error}");
    }

    #[test]
    fn le_intestazioni_http_stanno_nella_cassaforte() {
        let vault = MemoryVault::default();
        let config = McpServerConfig {
            id: "web".into(),
            name: "Remoto".into(),
            transport: Transport::Http,
            url: "https://esempio.it/mcp".into(),
            headers: vec![("Authorization".into(), "Bearer segreto".into())],
            ..Default::default()
        };
        assert!(config.validate().is_ok());
        let sealed = seal_env(&vault, &config, None).unwrap();
        assert!(!serde_json::to_string(&sealed).unwrap().contains("segreto"));
        assert_eq!(
            vault.get("web", "header:Authorization").unwrap().as_deref(),
            Some("Bearer segreto")
        );
        let mut opened = sealed.clone();
        open_env(&vault, &mut opened).unwrap();
        assert_eq!(opened.headers, config.headers);
        forget_env(&vault, &sealed).unwrap();
        assert!(vault.0.lock().unwrap().is_empty());

        // Indirizzi non cifrati solo su questo computer.
        let with_url = |url: &str| McpServerConfig {
            url: url.into(),
            ..config.clone()
        };
        assert!(with_url("http://esempio.it/mcp").validate().is_err());
        assert!(with_url("http://127.0.0.1:8080/mcp").validate().is_ok());
        assert!(with_url("esempio.it").validate().is_err());
        let bad_header = McpServerConfig {
            headers: vec![("Nome: strano".into(), "x".into())],
            ..config.clone()
        };
        assert!(bad_header.validate().is_err());
    }

    /// Un server MCP HTTP finto: risponde in JSON alla presentazione, con un
    /// flusso di eventi a `tools/list` (con un `ping` in mezzo) e offre una
    /// risorsa. Restituisce l'indirizzo e le intestazioni ricevute.
    pub(crate) fn fake_http_server() -> (String, Arc<Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                let (head, body) = loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    request.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().to_string())
                            })
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length || n == 0 {
                            break (text[..end].to_string(), text[end + 4..].to_string());
                        }
                    }
                    if n == 0 {
                        break (text, String::new());
                    }
                };
                log.lock().unwrap().push(head.to_lowercase());
                let message: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let method = message.get("method").and_then(Value::as_str).unwrap_or("");
                let json_reply = |result: Value| {
                    let body = json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nMcp-Session-Id: sessione-1\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                let reply = match (method, message.get("result").is_some()) {
                    // La risposta dell'app al ping del server.
                    (_, true) => {
                        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_string()
                    }
                    ("initialize", _) => json_reply(json!({
                        "protocolVersion": "2025-06-18",
                        "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                        "serverInfo": {"name": "finto", "version": "1"}
                    })),
                    ("notifications/initialized", _) => {
                        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_string()
                    }
                    ("tools/list", _) => {
                        let ping = json!({"jsonrpc": "2.0", "id": "p1", "method": "ping"});
                        let result = json!({"jsonrpc": "2.0", "id": id, "result": {"tools": [
                            {"name": "somma", "description": "Somma", "inputSchema": {"type": "object"}}
                        ]}});
                        let body = format!(
                            "event: message\ndata: {ping}\n\nevent: message\ndata: {result}\n\n"
                        );
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    ("tools/call", _) => {
                        json_reply(json!({"content": [{"type": "text", "text": "3"}]}))
                    }
                    ("resources/list", _) => json_reply(json!({"resources": [
                        {"uri": "doc://guida", "name": "Guida", "mimeType": "text/markdown"}
                    ]})),
                    ("resources/read", _) => json_reply(json!({"contents": [
                        {"uri": "doc://guida", "text": "Contenuto della guida"}
                    ]})),
                    ("prompts/list", _) => json_reply(json!({"prompts": [
                        {"name": "revisione", "description": "Rivedi un testo",
                         "arguments": [{"name": "tono", "description": "Che tono usare", "required": true}]},
                        {"name": "senza-descrizione", "title": "Titolo al posto della descrizione"}
                    ]})),
                    ("prompts/get", _) => {
                        let tono = message
                            .pointer("/params/arguments/tono")
                            .and_then(Value::as_str)
                            .unwrap_or("?");
                        json_reply(json!({"messages": [
                            {"role": "user", "content": {"type": "text", "text": format!("Rivedi con tono {tono}.")}},
                            {"role": "assistant", "content": {"type": "text", "text": "Certo."}},
                            {"role": "user", "content": {"type": "resource", "resource": {"uri": "doc://regole", "text": "Regole di stile"}}},
                            {"role": "user", "content": {"type": "image", "data": "AA==", "mimeType": "image/png"}}
                        ]}))
                    }
                    _ => json_reply(json!({})),
                };
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (format!("http://127.0.0.1:{port}/mcp"), seen)
    }

    #[tokio::test]
    async fn parla_con_un_server_http_e_ne_legge_le_risorse() {
        let (url, seen) = fake_http_server();
        let config = McpServerConfig {
            name: "Remoto".into(),
            transport: Transport::Http,
            url,
            headers: vec![("Authorization".into(), "Bearer abc".into())],
            ..Default::default()
        };
        let tools = probe(&config).await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["somma", LIST_RESOURCES, READ_RESOURCE]);

        let set = McpSet::start(std::slice::from_ref(&config), None)
            .await
            .unwrap();
        assert_eq!(
            set.call("mcp__Remoto__somma", &json!({"a": 1})).await,
            ToolOutput::ok("3")
        );
        let listed = set.call("mcp__Remoto__list_resources", &json!({})).await;
        assert!(
            listed
                .content
                .contains("doc://guida — Guida — tipo text/markdown"),
            "{}",
            listed.content
        );
        let read = set
            .call("mcp__Remoto__read_resource", &json!({"uri": "doc://guida"}))
            .await;
        assert_eq!(read, ToolOutput::ok("Contenuto della guida"));
        assert!(
            set.call("mcp__Remoto__read_resource", &json!({}))
                .await
                .is_error
        );
        assert!(
            set.summary("mcp__Remoto__read_resource")
                .unwrap()
                .contains("risorsa")
        );

        // Intestazione dell'utente, sessione e versione del protocollo.
        let seen = seen.lock().unwrap();
        assert!(seen.iter().all(|h| h.contains("authorization: bearer abc")));
        assert!(
            seen.iter()
                .any(|h| h.contains("mcp-session-id: sessione-1"))
        );
        assert!(
            seen.iter()
                .any(|h| h.contains("mcp-protocol-version: 2025-06-18"))
        );
    }

    #[tokio::test]
    async fn i_prompt_del_server_si_elencano_e_si_compilano() {
        let (url, _) = fake_http_server();
        let config = McpServerConfig {
            name: "Remoto".into(),
            transport: Transport::Http,
            url,
            ..Default::default()
        };
        let set = McpSet::start(std::slice::from_ref(&config), None)
            .await
            .unwrap();
        let prompts = set.prompts().await;
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0].server_name, "Remoto");
        assert_eq!(prompts[0].name, "revisione");
        assert_eq!(
            prompts[0].arguments,
            [McpPromptArg {
                name: "tono".into(),
                description: "Che tono usare".into(),
                required: true
            }]
        );
        assert_eq!(prompts[1].description, "Titolo al posto della descrizione");

        let arguments = [("tono".to_string(), "formale".to_string())].into();
        let text = set.get_prompt(0, "revisione", &arguments).await.unwrap();
        assert_eq!(
            text,
            "Rivedi con tono formale.\n\n[assistente] Certo.\n\nRegole di stile\n\n[immagine]"
        );
        assert!(set.get_prompt(5, "revisione", &arguments).await.is_err());
    }

    #[tokio::test]
    async fn un_server_http_che_rifiuta_spiega_perche() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Some(Ok(mut stream)) = listener.incoming().next() {
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope",
                );
            }
        });
        let config = McpServerConfig {
            name: "Chiuso".into(),
            transport: Transport::Http,
            url: format!("http://127.0.0.1:{port}/mcp"),
            ..Default::default()
        };
        let error = probe(&config).await.err().unwrap().to_string();
        assert!(
            error.contains("401") && error.contains("Authorization"),
            "{error}"
        );
    }
}
