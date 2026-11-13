//! Client MCP (Model Context Protocol) per gli strumenti esterni (AG-7).
//!
//! Un server MCP è un programma che l'utente installa e configura nelle
//! Impostazioni. L'app lo avvia come processo figlio e gli parla in JSON-RPC
//! sulle sue entrata e uscita standard, una riga per messaggio (trasporto
//! "stdio"). Gli strumenti che il server dichiara vengono offerti al modello
//! accanto a quelli dell'app, passando per gli stessi permessi: tutti in
//! classe "esecuzione", perché un server può fare qualunque cosa e le sue
//! indicazioni ("questo strumento legge soltanto") sono sue, non dell'utente
//! (vedi ADR 0009).

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

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
/// Inizio dei nomi degli strumenti MCP come li vede il modello.
pub const PREFIX: &str = "mcp__";
/// Lunghezza massima dei nomi degli strumenti accettata dai servizi.
const NAME_MAX: usize = 64;

/// Un server MCP configurato dall'utente.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpServerConfig {
    /// Vuoto per un server non ancora salvato.
    pub id: String,
    pub name: String,
    /// Il programma da avviare, per esempio `npx` o `C:\strumenti\server.exe`.
    pub command: String,
    pub args: Vec<String>,
    /// Variabili d'ambiente aggiuntive (`NOME`, `valore`).
    pub env: Vec<(String, String)>,
}

impl McpServerConfig {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::msg("Dai un nome al server MCP."));
        }
        if self.command.trim().is_empty() {
            return Err(Error::msg("Indica il comando che avvia il server MCP."));
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
        Ok(())
    }
}

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

/// Il collegamento con un server in esecuzione.
pub struct McpClient {
    name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
    /// Ultime righe scritte dal server sugli errori.
    stderr: Arc<Mutex<Vec<String>>>,
    #[cfg(windows)]
    _job: Option<win32job::Job>,
}

impl McpClient {
    /// Avvia il server e completa la presentazione (`initialize`).
    pub async fn start(config: &McpServerConfig, cwd: Option<&Path>) -> Result<Self> {
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
        let stdout = BufReader::new(child.stdout.take().expect("uscita collegata")).lines();
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let sink = stderr.clone();
        let mut err_lines = BufReader::new(child.stderr.take().expect("errori collegati")).lines();
        tokio::spawn(async move {
            while let Ok(Some(line)) = err_lines.next_line().await {
                let mut lines = sink.lock().unwrap_or_else(|e| e.into_inner());
                lines.push(line);
                if lines.len() > STDERR_LINES {
                    lines.remove(0);
                }
            }
        });

        let mut client = Self {
            name: config.name.trim().to_string(),
            child,
            stdin,
            stdout,
            next_id: 1,
            stderr,
            #[cfg(windows)]
            _job: job,
        };
        client
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "progetto-ai", "version": crate::version() }
                }),
                START_TIMEOUT,
            )
            .await?;
        client
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(client)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn stderr_tail(&self) -> String {
        self.stderr
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .join("\n")
    }

    /// Spiega un guasto: il messaggio e, se c'è, ciò che il server ha scritto sugli errori.
    fn failure(&mut self, message: &str) -> Error {
        let exited = matches!(self.child.try_wait(), Ok(Some(_)));
        let mut text = format!("Server MCP «{}»: {message}", self.name);
        if exited {
            text.push_str(" Il server si è chiuso.");
        }
        let tail = self.stderr_tail();
        if !tail.trim().is_empty() {
            text.push('\n');
            text.push_str(tail.trim());
        }
        Error::msg(text)
    }

    async fn send(&mut self, message: &Value) -> Result<()> {
        let mut line = serde_json::to_string(message)?;
        line.push('\n');
        if self.stdin.write_all(line.as_bytes()).await.is_err() || self.stdin.flush().await.is_err()
        {
            return Err(self.failure("non riceve più messaggi."));
        }
        Ok(())
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    /// Manda una richiesta e aspetta la sua risposta. Intanto risponde alle
    /// richieste del server (`ping`) e ignora le sue notifiche.
    async fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let line = match tokio::time::timeout_at(deadline, self.stdout.next_line()).await {
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
            let has_method = message.get("method").is_some();
            match (message.get("id"), has_method) {
                // Una richiesta del server: si risponde subito.
                (Some(request_id), true) => {
                    let reply = if message["method"] == "ping" {
                        json!({ "jsonrpc": "2.0", "id": request_id, "result": {} })
                    } else {
                        json!({ "jsonrpc": "2.0", "id": request_id,
                                "error": { "code": -32601, "message": "Metodo non supportato" } })
                    };
                    self.send(&reply).await?;
                }
                (Some(response_id), false) if response_id.as_u64() == Some(id) => {
                    if let Some(error) = message.get("error") {
                        let text = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("errore sconosciuto");
                        return Err(Error::msg(format!("Server MCP «{}»: {text}", self.name)));
                    }
                    return Ok(message.get("result").cloned().unwrap_or(Value::Null));
                }
                // Notifiche e risposte vecchie.
                _ => {}
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
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    /// Gli strumenti del server, per l'interfaccia.
    pub async fn tool_infos(&mut self) -> Result<Vec<McpToolInfo>> {
        Ok(self
            .list_tools()
            .await?
            .into_iter()
            .map(|t| McpToolInfo {
                name: t.name,
                description: t.description,
            })
            .collect())
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

/// Uno strumento offerto al modello, con il server che lo esegue.
struct Entry {
    exposed: String,
    client: usize,
    server: String,
    tool: String,
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
            let tools = client.list_tools().await?;
            let index = set.clients.len();
            for tool in tools {
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
                    tool: tool.name,
                });
            }
            set.clients.push(tokio::sync::Mutex::new(client));
        }
        Ok(set)
    }

    pub fn defs(&self) -> &[ToolDef] {
        &self.defs
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.exposed == name)
    }

    /// Descrizione breve della chiamata, per la conferma e il registro.
    pub fn summary(&self, name: &str) -> Option<String> {
        let entry = self.entries.iter().find(|e| e.exposed == name)?;
        Some(format!(
            "Usare «{}» del server MCP «{}»",
            entry.tool, entry.server
        ))
    }

    pub async fn call(&self, name: &str, arguments: &Value) -> ToolOutput {
        let Some(entry) = self.entries.iter().find(|e| e.exposed == name) else {
            return ToolOutput::error(format!("Lo strumento \"{name}\" non è disponibile."));
        };
        let mut client = self.clients[entry.client].lock().await;
        match client.call(&entry.tool, arguments).await {
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
}
