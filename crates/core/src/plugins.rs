//! Plugin (ADR 0026): pacchetti nel formato dei plugin di Claude Code con
//! skill, server MCP, agenti e comandi `/`. Qui si legge un plugin già copiato
//! in una cartella; l'installazione vera è in `ops/plugins.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agents::Agent;
use crate::bundle;
use crate::error::{Error, Result};
use crate::mcp::{McpServerConfig, Transport};
use crate::skills::{self, SKILL_FILE};
use crate::tools::ToolSettings;

pub const PLUGINS_DIR: &str = "plugins";
/// Dove può stare il manifest, in ordine di preferenza.
pub const MANIFESTS: [&str; 2] = [".claude-plugin/plugin.json", "plugin.json"];
pub const LIMITS: bundle::Limits = bundle::Limits {
    files: 500,
    bytes: 50 * 1024 * 1024,
    too_big: "Il plugin è troppo grande: al massimo 500 file e 50 MB.",
};
pub const NO_PLUGIN: &str =
    "Nella cartella o nello zip scelto non c'è un plugin (manca .claude-plugin/plugin.json).";
pub const MANY_PLUGINS: &str = "Lo zip contiene più plugin: importali uno alla volta.";
/// Caratteri massimi del testo di un comando.
const COMMAND_CHARS: usize = 30_000;

/// Un comando `/` di un plugin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginCommand {
    /// Il nome dopo `/`; i comandi in sottocartelle diventano `cartella:nome`.
    pub name: String,
    pub description: String,
    pub argument_hint: String,
    /// Il file del comando, relativo alla cartella del plugin.
    pub file: String,
    /// `true` se il testo usa `$ARGUMENTS` o `$1`, `$2`…
    pub takes_arguments: bool,
}

/// Ciò che un plugin installato ha creato, per poterlo togliere.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginRecord {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub path: String,
    pub skills: Vec<String>,
    /// Identificatori dei server MCP creati.
    pub mcp_servers: Vec<String>,
    /// Identificatori degli agenti creati.
    pub agents: Vec<i64>,
    pub commands: Vec<PluginCommand>,
    /// Avvisi dell'installazione: parti saltate, valori da completare.
    pub notes: Vec<String>,
}

/// Esito di un'importazione: il plugin installato, oppure il nome di quello
/// già presente che andrebbe sostituito (e nulla è cambiato).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstall {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed: Option<PluginRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing: Option<String>,
}

/// Un plugin letto dalla sua cartella, prima di installarne le parti.
#[derive(Debug, Clone, Default)]
pub struct Contents {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    /// Le cartelle delle skill, relative alla cartella del plugin.
    pub skills: Vec<PathBuf>,
    pub agents: Vec<Agent>,
    pub commands: Vec<PluginCommand>,
    pub servers: Vec<McpServerConfig>,
    pub notes: Vec<String>,
}

/// Intestazione YAML semplice (`chiave: valore`, anche con elenchi `- voce`)
/// e testo che la segue. Senza intestazione: nessuna chiave e tutto il testo.
pub fn frontmatter(text: &str) -> (HashMap<String, String>, String) {
    let text = text.trim_start_matches('\u{feff}');
    let mut map = HashMap::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return (map, text.trim().to_string());
    }
    let mut header = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        header.push(line);
    }
    if !closed {
        return (HashMap::new(), text.trim().to_string());
    }
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    let mut current: Option<String> = None;
    for line in header {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented && let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim();
            let value = if value.starts_with('|') || value.starts_with('>') {
                String::new()
            } else {
                skills::unquote(value)
            };
            map.insert(key.clone(), value);
            current = Some(key);
            continue;
        }
        let Some(key) = &current else { continue };
        let item = line.trim();
        if item.is_empty() {
            continue;
        }
        let entry = map.entry(key.clone()).or_default();
        if let Some(item) = item.strip_prefix("- ") {
            if !entry.is_empty() {
                entry.push_str(", ");
            }
            entry.push_str(&skills::unquote(item));
        } else {
            if !entry.is_empty() {
                entry.push(' ');
            }
            entry.push_str(item);
        }
    }
    (map, body)
}

/// Gli strumenti di un agente dal campo `tools` di Claude Code.
pub fn agent_tools(tools: Option<&str>) -> ToolSettings {
    let Some(list) = tools.filter(|t| !t.trim().is_empty()) else {
        return Agent::default().tools;
    };
    let names: Vec<&str> = list
        .split([',', ' '])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    let has = |set: &[&str]| names.iter().any(|n| set.contains(n));
    ToolSettings {
        files: has(&[
            "Read",
            "Write",
            "Edit",
            "MultiEdit",
            "Glob",
            "Grep",
            "LS",
            "NotebookRead",
            "NotebookEdit",
        ]),
        terminal: has(&["Bash"]),
        web: has(&["WebFetch", "WebSearch"]),
        ..Default::default()
    }
}

/// Sostituisce `${CLAUDE_PLUGIN_ROOT}` e le `${VARIABILE}` (anche con
/// `:-predefinito`). Le variabili senza valore restano vuote e finiscono in `missing`.
pub fn expand(
    text: &str,
    root: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    missing: &mut Vec<String>,
) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &after[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((n, d)) => (n, Some(d)),
            None => (inner, None),
        };
        if name == "CLAUDE_PLUGIN_ROOT" {
            out.push_str(&root.to_string_lossy());
        } else if let Some(value) = env(name).filter(|v| !v.is_empty()) {
            out.push_str(&value);
        } else if let Some(default) = default {
            out.push_str(default);
        } else if !missing.iter().any(|m| m == name) {
            missing.push(name.to_string());
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn string_map(value: Option<&Value>) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default();
    pairs.sort();
    pairs
}

/// I server MCP di un oggetto `{ "nome": { … } }` come in `.mcp.json`.
fn servers_from(
    map: &serde_json::Map<String, Value>,
    root: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    notes: &mut Vec<String>,
) -> Vec<McpServerConfig> {
    let mut servers = Vec::new();
    let mut names: Vec<&String> = map.keys().collect();
    names.sort();
    for name in names {
        let spec = &map[name];
        let mut missing = Vec::new();
        let mut x = |s: &str| expand(s, root, env, &mut missing);
        let kind = spec.get("type").and_then(Value::as_str).unwrap_or("");
        let config = if let Some(command) = spec.get("command").and_then(Value::as_str) {
            McpServerConfig {
                name: name.clone(),
                transport: Transport::Stdio,
                command: x(command),
                args: spec
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(&mut x).collect())
                    .unwrap_or_default(),
                env: string_map(spec.get("env"))
                    .into_iter()
                    .map(|(k, v)| (k, x(&v)))
                    .collect(),
                ..Default::default()
            }
        } else if let Some(url) = spec.get("url").and_then(Value::as_str) {
            if kind == "sse" {
                notes.push(format!(
                    "Server MCP «{name}» saltato: usa il trasporto SSE, che l'app non supporta."
                ));
                continue;
            }
            McpServerConfig {
                name: name.clone(),
                transport: Transport::Http,
                url: x(url),
                headers: string_map(spec.get("headers"))
                    .into_iter()
                    .map(|(k, v)| (k, x(&v)))
                    .collect(),
                ..Default::default()
            }
        } else {
            notes.push(format!(
                "Server MCP «{name}» saltato: manca il comando o l'indirizzo."
            ));
            continue;
        };
        if !missing.is_empty() {
            notes.push(format!(
                "Server MCP «{name}»: completa nelle Impostazioni i valori di {}.",
                missing.join(", ")
            ));
        }
        servers.push(config);
    }
    servers
}

fn read_json(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| {
        Error::msg(format!(
            "File del plugin illeggibile ({}): {e}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
    })
}

/// I comandi della cartella `commands/`, anche in sottocartelle.
fn commands_in(dir: &Path, notes: &mut Vec<String>) -> Vec<PluginCommand> {
    let mut commands = Vec::new();
    for file in bundle::files_in(&dir.join("commands")) {
        if file
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(dir.join("commands").join(&file)) else {
            continue;
        };
        let (meta, body) = frontmatter(&text);
        let name = file
            .with_extension("")
            .to_string_lossy()
            .replace(['\\', '/'], ":");
        let description = meta
            .get("description")
            .cloned()
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| {
                body.lines()
                    .map(|l| l.trim_start_matches('#').trim())
                    .find(|l| !l.is_empty())
                    .unwrap_or("")
                    .chars()
                    .take(100)
                    .collect()
            });
        let takes_arguments =
            body.contains("$ARGUMENTS") || (1..=9).any(|n| body.contains(&format!("${n}")));
        if body.contains("!`") {
            notes.push(format!(
                "Comando /{name}: le righe che eseguono comandi (!`…`) restano testo, l'app non le esegue."
            ));
        }
        commands.push(PluginCommand {
            name,
            description,
            argument_hint: meta.get("argument-hint").cloned().unwrap_or_default(),
            file: format!("commands/{}", file.to_string_lossy().replace('\\', "/")),
            takes_arguments,
        });
    }
    commands
}

/// Legge il plugin che sta in `dir`. `root` è la cartella dove verrà
/// installato (per `${CLAUDE_PLUGIN_ROOT}`); `env` dà le variabili di Windows.
pub fn read_contents(
    dir: &Path,
    root: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Contents> {
    let manifest_path = MANIFESTS
        .iter()
        .map(|m| dir.join(m))
        .find(|p| p.is_file())
        .ok_or_else(|| Error::msg(NO_PLUGIN))?;
    let manifest = read_json(&manifest_path)?;
    let text = |key: &str| {
        manifest
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let name = text("name");
    skills::validate_name(&name).map_err(|_| {
        Error::msg(format!(
            "Nome del plugin non valido: «{name}». Il manifest deve avere un name con solo lettere minuscole, cifre e trattini."
        ))
    })?;
    let author = match manifest.get("author") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        None => String::new(),
    };
    let mut contents = Contents {
        name,
        version: text("version"),
        description: text("description"),
        author,
        ..Default::default()
    };

    // Skill: le cartelle di `skills/` con uno SKILL.md.
    if let Ok(entries) = std::fs::read_dir(dir.join("skills")) {
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| e.path().join(SKILL_FILE).is_file())
            .map(|e| PathBuf::from("skills").join(e.file_name()))
            .collect();
        found.sort();
        contents.skills = found;
    }

    // Agenti: i file `.md` di `agents/`.
    for file in bundle::files_in(&dir.join("agents")) {
        if file
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(dir.join("agents").join(&file)) else {
            continue;
        };
        let (meta, body) = frontmatter(&text);
        let name = meta
            .get("name")
            .cloned()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| file.with_extension("").to_string_lossy().into_owned());
        contents.agents.push(Agent {
            name,
            description: meta.get("description").cloned().unwrap_or_default(),
            instructions: body,
            tools: agent_tools(meta.get("tools").map(String::as_str)),
            ..Default::default()
        });
    }

    contents.commands = commands_in(dir, &mut contents.notes);

    // Server MCP: `.mcp.json` e il campo `mcpServers` del manifest.
    let mut notes = Vec::new();
    let mut add_servers = |value: &Value, notes: &mut Vec<String>| {
        let map = value
            .get("mcpServers")
            .and_then(Value::as_object)
            .or_else(|| value.as_object());
        if let Some(map) = map {
            contents.servers.extend(servers_from(map, root, env, notes));
        }
    };
    let mcp_file = dir.join(".mcp.json");
    if mcp_file.is_file() {
        add_servers(&read_json(&mcp_file)?, &mut notes);
    }
    match manifest.get("mcpServers") {
        Some(Value::String(path)) => {
            let path = dir.join(path.trim_start_matches("./"));
            // Il percorso deve restare dentro il plugin.
            let inside = path.canonicalize().ok().zip(dir.canonicalize().ok());
            match inside {
                Some((p, d))
                    if p.starts_with(&d) && p != mcp_file.canonicalize().unwrap_or_default() =>
                {
                    add_servers(&read_json(&p)?, &mut notes)
                }
                Some(_) => {}
                None => notes.push("Il file dei server MCP indicato nel manifest non c'è.".into()),
            }
        }
        Some(value @ Value::Object(_)) => add_servers(&json_wrap(value), &mut notes),
        _ => {}
    }
    contents.notes.extend(notes);

    for (part, note) in [
        (
            "hooks",
            "Il plugin contiene hooks: l'app non li supporta e li salta.",
        ),
        (
            "lspServers",
            "Il plugin contiene server LSP: l'app non li supporta e li salta.",
        ),
        (
            "outputStyles",
            "Il plugin contiene stili di risposta: l'app non li supporta e li salta.",
        ),
    ] {
        if manifest.get(part).is_some() || dir.join(part).exists() {
            contents.notes.push(note.to_string());
        }
    }
    Ok(contents)
}

/// Il campo `mcpServers` del manifest, visto come il contenuto di `.mcp.json`.
fn json_wrap(value: &Value) -> Value {
    serde_json::json!({ "mcpServers": value })
}

/// Il testo di un comando con i suoi argomenti: `$ARGUMENTS` è tutto ciò che
/// l'utente ha scritto, `$1`, `$2`… le sue parole.
pub fn command_text(plugin_dir: &Path, command: &PluginCommand, arguments: &str) -> Result<String> {
    let path = plugin_dir.join(&command.file);
    // Il file deve stare dentro il plugin.
    let real = path
        .canonicalize()
        .map_err(|_| Error::msg("Il file di questo comando non c'è più: reinstalla il plugin."))?;
    if !real.starts_with(plugin_dir.canonicalize()?) {
        return Err(Error::msg(
            "Il file di questo comando non c'è più: reinstalla il plugin.",
        ));
    }
    let (_, body) = frontmatter(&std::fs::read_to_string(real)?);
    let words: Vec<&str> = arguments.split_whitespace().collect();
    let mut text = body.replace("$ARGUMENTS", arguments.trim());
    for n in (1..=9).rev() {
        text = text.replace(&format!("${n}"), words.get(n - 1).copied().unwrap_or(""));
    }
    Ok(text.chars().take(COMMAND_CHARS).collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn intestazioni_con_elenchi_e_senza() {
        let (meta, body) = frontmatter(
            "---\nname: revisore\ndescription: \"Rivede il codice\"\ntools:\n  - Read\n  - Bash\nmodel: sonnet\n---\n\nSei un revisore.",
        );
        assert_eq!(meta["name"], "revisore");
        assert_eq!(meta["description"], "Rivede il codice");
        assert_eq!(meta["tools"], "Read, Bash");
        assert_eq!(body, "Sei un revisore.");
        let (meta, body) = frontmatter("# Solo testo\n$ARGUMENTS");
        assert!(meta.is_empty());
        assert_eq!(body, "# Solo testo\n$ARGUMENTS");
    }

    #[test]
    fn gli_strumenti_degli_agenti() {
        let t = agent_tools(Some("Read, Grep, Bash"));
        assert!(t.files && t.terminal && !t.web);
        let t = agent_tools(Some("WebSearch"));
        assert!(!t.files && !t.terminal && t.web);
        assert_eq!(agent_tools(None), Agent::default().tools);
    }

    #[test]
    fn le_variabili_dei_server() {
        let env = |name: &str| (name == "CHIAVE").then(|| "segreta".to_string());
        let mut missing = Vec::new();
        let root = Path::new("C:\\dati\\plugins\\p");
        assert_eq!(
            expand("${CLAUDE_PLUGIN_ROOT}/server.js", root, &env, &mut missing),
            "C:\\dati\\plugins\\p/server.js"
        );
        assert_eq!(expand("k=${CHIAVE}", root, &env, &mut missing), "k=segreta");
        assert_eq!(expand("${PORTA:-8080}", root, &env, &mut missing), "8080");
        assert_eq!(
            expand("Bearer ${TOKEN}", root, &env, &mut missing),
            "Bearer "
        );
        assert_eq!(missing, ["TOKEN"]);
        assert_eq!(expand("${aperta", root, &env, &mut missing), "${aperta");
    }

    pub(crate) fn sample_plugin(dir: &Path) {
        let write = |rel: &str, text: &str| {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            ".claude-plugin/plugin.json",
            r#"{"name": "revisione", "version": "1.2.0", "description": "Strumenti per rivedere il codice", "author": {"name": "Prova"}}"#,
        );
        write(
            "skills/stile/SKILL.md",
            "---\nname: stile\ndescription: Regole di stile del codice\n---\nUsa nomi chiari.",
        );
        write("skills/stile/esempi.md", "esempio");
        write(
            "agents/revisore.md",
            "---\nname: revisore\ndescription: Rivede le modifiche\ntools: Read, Grep\n---\nRivedi con cura.",
        );
        write(
            "commands/rivedi.md",
            "---\ndescription: Rivede un file\nargument-hint: <file>\n---\nRivedi il file $1 con attenzione. Note: $ARGUMENTS",
        );
        write("commands/git/stato.md", "# Mostra lo stato\n!`git status`");
        write(
            ".mcp.json",
            r#"{"mcpServers": {
                "locale": {"command": "node", "args": ["${CLAUDE_PLUGIN_ROOT}/server.js"], "env": {"API_KEY": "${PROVA_CHIAVE_ASSENTE}"}},
                "remoto": {"type": "http", "url": "https://esempio.it/mcp"},
                "vecchio": {"type": "sse", "url": "https://esempio.it/sse"}
            }}"#,
        );
        write("hooks/hooks.json", "{}");
    }

    #[test]
    fn si_legge_un_plugin_completo() {
        let dir = std::env::temp_dir().join(format!("pa-plugin-leggi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        sample_plugin(&dir);
        let root = Path::new("C:\\installato");
        let c = read_contents(&dir, root, &|_| None).unwrap();
        assert_eq!(
            (c.name.as_str(), c.version.as_str(), c.author.as_str()),
            ("revisione", "1.2.0", "Prova")
        );
        assert_eq!(c.skills, [PathBuf::from("skills").join("stile")]);
        assert_eq!(c.agents.len(), 1);
        assert_eq!(c.agents[0].instructions, "Rivedi con cura.");
        assert!(c.agents[0].tools.files && !c.agents[0].tools.terminal);
        let names: Vec<&str> = c.commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["git:stato", "rivedi"]);
        assert!(c.commands[1].takes_arguments && !c.commands[0].takes_arguments);
        assert_eq!(c.commands[0].description, "Mostra lo stato");
        assert_eq!(c.commands[1].argument_hint, "<file>");
        let servers: Vec<&str> = c.servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(servers, ["locale", "remoto"]);
        assert_eq!(c.servers[0].args, ["C:\\installato/server.js"]);
        assert_eq!(c.servers[0].env, [("API_KEY".to_string(), String::new())]);
        let notes = c.notes.join("\n");
        assert!(notes.contains("«vecchio» saltato"), "{notes}");
        assert!(notes.contains("PROVA_CHIAVE_ASSENTE"), "{notes}");
        assert!(notes.contains("hooks"), "{notes}");
        assert!(notes.contains("/git:stato"), "{notes}");

        let text = command_text(&dir, &c.commands[1], "main.rs  in fretta").unwrap();
        assert_eq!(
            text,
            "Rivedi il file main.rs con attenzione. Note: main.rs  in fretta"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_manifest_senza_nome_valido_si_rifiuta() {
        let dir = std::env::temp_dir().join(format!("pa-plugin-nome-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.json"), r#"{"name": "Con Spazi"}"#).unwrap();
        assert!(read_contents(&dir, &dir, &|_| None).is_err());
        std::fs::remove_file(dir.join("plugin.json")).unwrap();
        assert_eq!(
            read_contents(&dir, &dir, &|_| None)
                .unwrap_err()
                .to_string(),
            NO_PLUGIN
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
