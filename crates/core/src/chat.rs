//! Invio di una conversazione a un modello e lettura della risposta in streaming.
//!
//! Parla l'API "chat completions" compatibile con OpenAI, quella esposta da
//! `llama-server` e dalla maggior parte dei provider cloud. L'API di Anthropic,
//! che ha una forma diversa, sta in `providers::anthropic`.

use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::error::{Error, Result};
use crate::local::SamplingParams;
use crate::tools::{ToolCall, ToolDef};

/// Un'immagine allegata a un messaggio dell'utente.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// Per esempio `image/png`.
    pub media_type: String,
    /// Contenuto in base64.
    pub data: String,
}

/// Un messaggio della conversazione, in forma indipendente dal provider.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatMessage {
    /// `system`, `user`, `assistant` oppure `tool`.
    pub role: String,
    pub content: String,
    /// Strumenti chiesti dal modello (messaggi `assistant`).
    pub tool_calls: Vec<ToolCall>,
    /// La chiamata a cui questo messaggio risponde (messaggi `tool`).
    pub tool_call_id: Option<String>,
    /// Lo strumento è fallito o è stato negato (messaggi `tool`).
    pub is_error: bool,
    /// Immagini allegate (messaggi `user`).
    pub images: Vec<Image>,
    /// Il messaggio così come l'ha prodotto il provider, ragionamento
    /// compreso. Vale solo mentre gli strumenti di quella stessa risposta
    /// sono in corso: Anthropic lo vuole indietro identico.
    pub native: Option<Value>,
}

impl ChatMessage {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            content: content.into(),
            ..Default::default()
        }
    }
}

/// Misure di una risposta appena generata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GenerationStats {
    /// Tutti i token letti, compresi quelli serviti dalla cache del servizio.
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub tokens_per_second: Option<f64>,
    /// Dei token letti, quanti sono arrivati dalla cache del servizio
    /// (costano circa un decimo). Solo con Anthropic.
    pub cache_read_tokens: Option<u64>,
    /// Dei token letti, quanti sono stati scritti nella cache (costano un
    /// quarto in più). Solo con Anthropic.
    pub cache_write_tokens: Option<u64>,
}

/// Ciò che arriva durante la generazione.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    /// Un pezzo di risposta.
    Text(String),
    /// Un pezzo di ragionamento (modelli che "pensano" prima di rispondere).
    Reasoning(String),
    /// Il nucleo ha salvato nuovi messaggi (una risposta parziale, il
    /// risultato di uno strumento): l'interfaccia li rilegge dall'archivio.
    Saved,
    /// Uno strumento aspetta la conferma dell'utente.
    ToolRequest {
        id: String,
        name: String,
        /// Descrizione breve di ciò che lo strumento farebbe.
        summary: String,
        arguments: Value,
    },
}

/// Esito di una generazione.
#[derive(Debug, Clone, Default)]
pub struct ChatOutcome {
    pub text: String,
    /// Strumenti che il modello chiede di usare prima di continuare.
    pub tool_calls: Vec<ToolCall>,
    /// La risposta nel formato del provider, da rimandare identica
    /// (vedi [`ChatMessage::native`]).
    pub native: Option<Value>,
    pub stats: GenerationStats,
    /// `true` se l'utente ha interrotto la risposta.
    pub cancelled: bool,
    /// Avviso da mostrare all'utente (risposta troncata, rifiuto del modello…).
    pub notice: Option<String>,
}

pub const NOTICE_TRUNCATED: &str =
    "La risposta è stata troncata: raggiunto il limite di token per risposta.";

pub const NOTICE_NO_TOOLS: &str = "Questo modello non sa usare gli strumenti (file, ricerca web, documenti, server MCP): il suo template non li prevede, quindi ha risposto solo con quello che sa. Per usarli carica un modello che li supporta, come Qwen2.5 o Nemotron.";

/// A chi è destinata la richiesta: decide quali parametri si possono inviare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// `llama-server`: accetta tutti i parametri di generazione.
    Llama,
    /// Servizio cloud compatibile con OpenAI: solo i parametri dello standard.
    OpenAi {
        /// I modelli recenti di OpenAI vogliono `max_completion_tokens` al
        /// posto di `max_tokens`.
        max_completion_tokens: bool,
    },
}

/// I messaggi nel formato "chat completions".
fn openai_messages(messages: &[ChatMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| match m.role.as_str() {
            "assistant" if !m.tool_calls.is_empty() => json!({
                "role": "assistant",
                "content": if m.content.is_empty() { Value::Null } else { json!(m.content) },
                "tool_calls": m.tool_calls.iter().map(|call| json!({
                    "id": call.id,
                    "type": "function",
                    "function": {
                        "name": call.name,
                        // Qui i parametri viaggiano come testo JSON. Quelli
                        // illeggibili non si rimandano: il servizio li rifiuterebbe.
                        "arguments": if call.arguments.is_object() {
                            call.arguments.to_string()
                        } else {
                            "{}".to_string()
                        },
                    },
                })).collect::<Vec<_>>(),
            }),
            "tool" => json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id,
                "content": m.content,
            }),
            "user" if !m.images.is_empty() => {
                let mut parts: Vec<Value> = m
                    .images
                    .iter()
                    .map(|image| {
                        json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", image.media_type, image.data),
                            },
                        })
                    })
                    .collect();
                parts.push(json!({ "type": "text", "text": m.content }));
                json!({ "role": "user", "content": parts })
            }
            role => json!({ "role": role, "content": m.content }),
        })
        .collect()
}

/// Adatta la conversazione ai limiti del template di un modello locale.
///
/// - Senza strumenti nel template (Gemma 3), le chiamate e i loro risultati
///   diventano testo: il modello sa comunque che cosa è successo, e il
///   template non va in errore per i messaggi `tool`.
/// - Con i ruoli da alternare, o dopo la trasformazione qui sopra, i
///   messaggi consecutivi dello stesso ruolo si uniscono in uno (succede
///   anche quando l'utente riscrive dopo un errore).
pub fn fit_template(
    messages: Vec<ChatMessage>,
    limits: crate::gguf::TemplateLimits,
) -> Vec<ChatMessage> {
    if !limits.no_tools && !limits.strict_roles {
        return messages;
    }
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    for mut m in messages {
        if limits.no_tools {
            if !m.tool_calls.is_empty() {
                for call in std::mem::take(&mut m.tool_calls) {
                    let line = format!("[Strumento {} con {}]", call.name, call.arguments);
                    if !m.content.is_empty() {
                        m.content.push('\n');
                    }
                    m.content.push_str(&line);
                    names.insert(call.id, call.name);
                }
                m.native = None;
            }
            if m.role == "tool" {
                let name = m
                    .tool_call_id
                    .take()
                    .and_then(|id| names.get(&id).cloned())
                    .unwrap_or_else(|| "strumento".to_string());
                let label = if m.is_error {
                    "Errore di"
                } else {
                    "Risultato di"
                };
                m = ChatMessage::new("user", format!("[{label} {name}]\n{}", m.content));
            }
        }
        match out.last_mut() {
            // Il prompt di sistema resta a sé: è il template a sistemarlo.
            Some(last)
                if last.role == m.role
                    && m.role != "system"
                    && m.tool_calls.is_empty()
                    && last.tool_calls.is_empty()
                    && m.role != "tool" =>
            {
                if !m.content.is_empty() {
                    if !last.content.is_empty() {
                        last.content.push_str("\n\n");
                    }
                    last.content.push_str(&m.content);
                }
                last.images.extend(m.images);
            }
            _ => out.push(m),
        }
    }
    out
}

/// Corpo della richiesta.
pub fn request_body(
    messages: &[ChatMessage],
    tools: &[ToolDef],
    sampling: &SamplingParams,
    model: Option<&str>,
    flavor: Flavor,
) -> Value {
    let mut body = json!({
        "messages": openai_messages(messages),
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if let Some(model) = model {
        body["model"] = json!(model);
    }
    if !tools.is_empty() {
        body["tools"] = tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.schema,
                    },
                })
            })
            .collect();
    }

    match flavor {
        Flavor::Llama => {
            body["temperature"] = json!(sampling.temperature);
            body["top_p"] = json!(sampling.top_p);
            body["top_k"] = json!(sampling.top_k);
            body["min_p"] = json!(sampling.min_p);
            body["repeat_penalty"] = json!(sampling.repeat_penalty);
            if let Some(max_tokens) = sampling.max_tokens {
                body["max_tokens"] = json!(max_tokens);
            }
        }
        Flavor::OpenAi {
            max_completion_tokens,
        } => {
            // Alcuni modelli cloud rifiutano qualsiasi valore diverso da quello
            // predefinito (1): lo si invia solo se l'utente lo ha cambiato.
            if sampling.temperature != 1.0 {
                body["temperature"] = json!(sampling.temperature);
            }
            if sampling.top_p != 1.0 {
                body["top_p"] = json!(sampling.top_p);
            }
            if let Some(max_tokens) = sampling.max_tokens {
                let key = if max_completion_tokens {
                    "max_completion_tokens"
                } else {
                    "max_tokens"
                };
                body[key] = json!(max_tokens);
            }
        }
    }

    if let Some(seed) = sampling.seed {
        body["seed"] = json!(seed);
    }
    if !sampling.stop.is_empty() {
        body["stop"] = json!(sampling.stop);
    }
    body
}

/// Invia la richiesta a `url` e chiama `on_event` per ogni pezzo ricevuto.
///
/// `cancel` interrompe la generazione: chiudendo la connessione anche il
/// modello smette di lavorare.
pub async fn stream_chat(
    http: &reqwest::Client,
    url: &str,
    api_key: Option<&str>,
    body: &Value,
    cancel: Arc<Notify>,
    mut on_event: impl FnMut(ChatEvent),
) -> Result<ChatOutcome> {
    let mut request = http.post(url).json(body);
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await?;

    if !response.status().is_success() {
        return Err(http_error(response).await);
    }

    let mut outcome = ChatOutcome::default();
    let mut parser = SseParser::default();
    let mut calls: Vec<PartialCall> = Vec::new();

    loop {
        let chunk = tokio::select! {
            chunk = response.chunk() => chunk?,
            _ = cancel.notified() => {
                outcome.cancelled = true;
                break;
            }
        };
        let Some(chunk) = chunk else { break };

        for data in parser.push(&chunk) {
            if data == "[DONE]" {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                return Err(Error::msg(message));
            }
            apply_chunk(&value, &mut outcome, &mut calls, &mut on_event);
        }
    }
    // Una chiamata interrotta a metà è incompleta: non si esegue.
    if !outcome.cancelled {
        outcome.tool_calls = finish_calls(calls);
    }
    Ok(outcome)
}

/// Una chiamata a uno strumento mentre arriva, un pezzo alla volta.
#[derive(Debug, Default)]
struct PartialCall {
    id: String,
    name: String,
    /// I parametri, come testo JSON che si completa pezzo dopo pezzo.
    arguments: String,
}

fn finish_calls(calls: Vec<PartialCall>) -> Vec<ToolCall> {
    calls
        .into_iter()
        .filter(|call| !call.name.is_empty())
        .enumerate()
        .map(|(index, call)| ToolCall {
            // Qualche servizio non assegna un identificatore: se ne inventa uno.
            id: if call.id.is_empty() {
                format!("call_{index}")
            } else {
                call.id
            },
            name: call.name,
            arguments: parse_arguments(&call.arguments),
        })
        .collect()
}

/// I parametri di una chiamata. Se non sono un oggetto JSON valido resta il
/// testo grezzo, e la chiamata fallirà senza eseguire nulla.
pub(crate) fn parse_arguments(text: &str) -> Value {
    if text.trim().is_empty() {
        return json!({});
    }
    match serde_json::from_str::<Value>(text) {
        Ok(value) if value.is_object() => value,
        _ => Value::String(text.to_string()),
    }
}

/// Estrae da un pezzo della risposta testo, ragionamento, strumenti e misure.
fn apply_chunk(
    value: &Value,
    outcome: &mut ChatOutcome,
    calls: &mut Vec<PartialCall>,
    on_event: &mut impl FnMut(ChatEvent),
) {
    if let Some(choice) = value.pointer("/choices/0") {
        if let Some(delta) = choice.get("delta") {
            for piece in delta
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let index = piece.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if calls.len() <= index {
                    calls.resize_with(index + 1, PartialCall::default);
                }
                let call = &mut calls[index];
                if let Some(id) = piece.get("id").and_then(Value::as_str) {
                    call.id.push_str(id);
                }
                if let Some(name) = piece.pointer("/function/name").and_then(Value::as_str) {
                    call.name.push_str(name);
                }
                if let Some(text) = piece.pointer("/function/arguments").and_then(Value::as_str) {
                    call.arguments.push_str(text);
                }
            }
            // `reasoning_content` è il nome usato da llama-server, `reasoning` da OpenRouter.
            let reasoning = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .and_then(Value::as_str);
            if let Some(text) = reasoning
                && !text.is_empty()
            {
                on_event(ChatEvent::Reasoning(text.to_string()));
            }
            if let Some(text) = delta.get("content").and_then(Value::as_str)
                && !text.is_empty()
            {
                outcome.text.push_str(text);
                on_event(ChatEvent::Text(text.to_string()));
            }
        }
        if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
            outcome.notice = Some(NOTICE_TRUNCATED.to_string());
        }
    }

    let stats = &mut outcome.stats;
    if let Some(usage) = value.get("usage").filter(|u| u.is_object()) {
        stats.prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_u64);
        stats.completion_tokens = usage.get("completion_tokens").and_then(Value::as_u64);
    }
    // Campo proprio di llama-server, con la velocità misurata dal motore.
    if let Some(timings) = value.get("timings") {
        if let Some(speed) = timings.get("predicted_per_second").and_then(Value::as_f64) {
            stats.tokens_per_second = Some(speed);
        }
        if let Some(n) = timings.get("prompt_n").and_then(Value::as_u64) {
            stats.prompt_tokens.get_or_insert(n);
        }
        if let Some(n) = timings.get("predicted_n").and_then(Value::as_u64) {
            stats.completion_tokens.get_or_insert(n);
        }
    }
}

/// Trasforma una risposta HTTP di errore in un messaggio leggibile.
pub(crate) async fn http_error(response: reqwest::Response) -> Error {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let hint = match status.as_u16() {
        401 | 403 => " Controlla la chiave API.",
        429 => " Troppe richieste o credito esaurito: riprova più tardi.",
        _ => "",
    };
    let message = error_message(&body);
    Error::msg(format!(
        "Il servizio ha risposto con un errore ({status}): {}.{hint}",
        message.trim().trim_end_matches('.')
    ))
}

/// Il messaggio d'errore dentro una risposta JSON, o l'inizio del testo grezzo.
pub(crate) fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(300).collect())
}

/// Ricompone le righe `data: ...` di uno streaming Server-Sent Events, anche
/// quando una riga arriva spezzata tra due pacchetti.
#[derive(Default)]
pub(crate) struct SseParser {
    buffer: Vec<u8>,
}

impl SseParser {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(end) = self.buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            if let Some(data) = line.trim_end().strip_prefix("data:") {
                out.push(data.trim_start().to_string());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::TemplateLimits;

    /// La cronologia della conversazione che mandava Gemma 3 in errore.
    fn history_with_tools() -> Vec<ChatMessage> {
        vec![
            ChatMessage::new("system", "Sei un assistente."),
            ChatMessage::new("user", "Cosa c'è nella cartella?"),
            ChatMessage {
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "list_dir".into(),
                    arguments: json!({ "path": "." }),
                }],
                ..ChatMessage::new("assistant", "")
            },
            ChatMessage {
                tool_call_id: Some("c1".into()),
                ..ChatMessage::new("tool", "a.txt b.txt")
            },
            ChatMessage::new("assistant", "Ci sono a.txt e b.txt."),
            ChatMessage::new("user", "Usa il server MCP."),
            // Riscritto dopo un errore: due messaggi dell'utente di fila.
            ChatMessage::new("user", "Mi senti?"),
        ]
    }

    #[test]
    fn senza_strumenti_nel_template_la_cronologia_diventa_testo_alternato() {
        let gemma = TemplateLimits {
            no_tools: true,
            strict_roles: true,
        };
        let out = fit_template(history_with_tools(), gemma);
        let roles: Vec<&str> = out.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            ["system", "user", "assistant", "user", "assistant", "user"]
        );
        assert!(
            out.iter()
                .all(|m| m.tool_calls.is_empty() && m.tool_call_id.is_none())
        );
        assert_eq!(out[2].content, "[Strumento list_dir con {\"path\":\".\"}]");
        assert_eq!(out[3].content, "[Risultato di list_dir]\na.txt b.txt");
        assert_eq!(out[5].content, "Usa il server MCP.\n\nMi senti?");
    }

    #[test]
    fn con_gli_strumenti_si_uniscono_solo_i_ruoli_ripetuti() {
        let strict = TemplateLimits {
            no_tools: false,
            strict_roles: true,
        };
        let out = fit_template(history_with_tools(), strict);
        let roles: Vec<&str> = out.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            ["system", "user", "assistant", "tool", "assistant", "user"]
        );
        assert_eq!(out[2].tool_calls.len(), 1);
        // Un template senza limiti riceve la cronologia così com'è.
        assert_eq!(
            fit_template(history_with_tools(), TemplateLimits::default()).len(),
            7
        );
    }

    #[test]
    fn sse_ricompone_le_righe_spezzate() {
        let mut parser = SseParser::default();
        assert!(parser.push(b"data: {\"a\":").is_empty());
        assert_eq!(parser.push(b"1}\n\ndata: [DO"), vec!["{\"a\":1}"]);
        assert_eq!(parser.push(b"NE]\n"), vec!["[DONE]"]);
    }

    #[test]
    fn sse_non_spezza_i_caratteri_accentati() {
        let bytes = "data: perché\n".as_bytes();
        let (a, b) = bytes.split_at(11); // taglia a metà la "é"
        let mut parser = SseParser::default();
        assert!(parser.push(a).is_empty());
        assert_eq!(parser.push(b), vec!["perché"]);
    }

    #[test]
    fn raccoglie_testo_e_misure() {
        let mut outcome = ChatOutcome::default();
        let mut events = Vec::new();
        let mut calls = Vec::new();
        let chunks = [
            json!({"choices": [{"delta": {"reasoning_content": "penso"}}]}),
            json!({"choices": [{"delta": {"content": "Ciao"}}]}),
            json!({"choices": [{"delta": {"content": " mondo"}}]}),
            json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 2},
                   "timings": {"predicted_per_second": 41.5}}),
        ];
        for chunk in &chunks {
            apply_chunk(chunk, &mut outcome, &mut calls, &mut |e| events.push(e));
        }

        assert_eq!(outcome.text, "Ciao mondo");
        assert_eq!(events[0], ChatEvent::Reasoning("penso".into()));
        assert_eq!(events.len(), 3);
        assert_eq!(outcome.stats.prompt_tokens, Some(12));
        assert_eq!(outcome.stats.completion_tokens, Some(2));
        assert_eq!(outcome.stats.tokens_per_second, Some(41.5));
        assert_eq!(outcome.notice, None);
    }

    #[test]
    fn ricompone_le_chiamate_agli_strumenti() {
        let mut outcome = ChatOutcome::default();
        let mut calls = Vec::new();
        // Due chiamate in parallelo, con i parametri che arrivano a pezzi.
        let chunks = [
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "call_a", "function": {"name": "read_file", "arguments": ""}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "{\"path\": \"no"}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 1, "function": {"name": "web_search", "arguments": "{\"query\""}},
                {"index": 0, "function": {"arguments": "te.txt\"}"}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 1, "function": {"arguments": ": rotto"}}]}, "finish_reason": "tool_calls"}]}),
        ];
        for chunk in &chunks {
            apply_chunk(chunk, &mut outcome, &mut calls, &mut |_| {});
        }
        let calls = finish_calls(calls);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call_a");
        assert_eq!(calls[0].arguments, json!({"path": "note.txt"}));
        // Senza identificatore se ne assegna uno; il JSON rotto resta testo.
        assert_eq!(calls[1].id, "call_1");
        assert_eq!(calls[1].arguments, json!("{\"query\": rotto"));
        assert_eq!(parse_arguments(""), json!({}));
        assert_eq!(parse_arguments("[1]"), json!("[1]"));
    }

    #[test]
    fn la_cronologia_con_strumenti_e_immagini_ha_il_formato_giusto() {
        let call = ToolCall {
            id: "call_a".into(),
            name: "read_file".into(),
            arguments: json!({"path": "note.txt"}),
        };
        let messages = [
            ChatMessage {
                images: vec![Image {
                    media_type: "image/png".into(),
                    data: "AQID".into(),
                }],
                ..ChatMessage::new("user", "Cosa vedi?")
            },
            ChatMessage {
                tool_calls: vec![call],
                ..ChatMessage::new("assistant", "")
            },
            ChatMessage {
                tool_call_id: Some("call_a".into()),
                ..ChatMessage::new("tool", "pane")
            },
        ];
        let tools = crate::tools::available(
            &crate::tools::ToolSettings {
                files: false,
                web: true,
                ..Default::default()
            },
            &[],
        );
        let body = request_body(
            &messages,
            &tools,
            &SamplingParams::default(),
            None,
            Flavor::Llama,
        );

        let sent = body["messages"].as_array().unwrap();
        assert_eq!(
            sent[0]["content"][0]["image_url"]["url"],
            json!("data:image/png;base64,AQID")
        );
        assert_eq!(sent[0]["content"][1]["text"], json!("Cosa vedi?"));
        assert_eq!(sent[1]["content"], Value::Null);
        assert_eq!(
            sent[1]["tool_calls"][0]["function"]["arguments"],
            json!("{\"path\":\"note.txt\"}")
        );
        assert_eq!(
            sent[2],
            json!({"role": "tool", "tool_call_id": "call_a", "content": "pane"})
        );
        assert_eq!(body["tools"][0]["type"], json!("function"));
        assert_eq!(body["tools"][0]["function"]["name"], json!("web_search"));
        // Senza strumenti il campo non c'è.
        let plain = request_body(
            &messages,
            &[],
            &SamplingParams::default(),
            None,
            Flavor::Llama,
        );
        assert!(plain.get("tools").is_none());
    }

    #[test]
    fn segnala_la_risposta_troncata() {
        let mut outcome = ChatOutcome::default();
        let chunk = json!({"choices": [{"delta": {}, "finish_reason": "length"}]});
        apply_chunk(&chunk, &mut outcome, &mut Vec::new(), &mut |_| {});
        assert_eq!(outcome.notice.as_deref(), Some(NOTICE_TRUNCATED));
    }

    #[test]
    fn llama_riceve_tutti_i_parametri() {
        let body = request_body(&[], &[], &SamplingParams::default(), None, Flavor::Llama);
        assert!(body.get("model").is_none());
        assert!(body.get("top_k").is_some());
        assert!(body.get("min_p").is_some());
        assert!(body.get("seed").is_none());
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("stop").is_none());
        assert_eq!(body["stream"], json!(true));

        let body = request_body(
            &[],
            &[],
            &SamplingParams {
                seed: Some(7),
                max_tokens: Some(100),
                stop: vec!["FINE".into()],
                ..Default::default()
            },
            None,
            Flavor::Llama,
        );
        assert_eq!(body["seed"], json!(7));
        assert_eq!(body["max_tokens"], json!(100));
        assert_eq!(body["stop"], json!(["FINE"]));
    }

    #[test]
    fn il_cloud_riceve_solo_i_parametri_standard() {
        let flavor = Flavor::OpenAi {
            max_completion_tokens: false,
        };
        let neutral = SamplingParams {
            temperature: 1.0,
            top_p: 1.0,
            ..Default::default()
        };
        let body = request_body(&[], &[], &neutral, Some("modello-x"), flavor);
        assert_eq!(body["model"], json!("modello-x"));
        // Valori predefiniti: non si inviano affatto.
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        // Parametri che esistono solo in llama.cpp.
        assert!(body.get("top_k").is_none());
        assert!(body.get("min_p").is_none());
        assert!(body.get("repeat_penalty").is_none());

        let custom = SamplingParams {
            temperature: 0.2,
            max_tokens: Some(500),
            ..neutral
        };
        let body = request_body(&[], &[], &custom, Some("modello-x"), flavor);
        assert_eq!(body["temperature"], json!(0.2f32));
        assert_eq!(body["max_tokens"], json!(500));

        let openai = Flavor::OpenAi {
            max_completion_tokens: true,
        };
        let body = request_body(&[], &[], &custom, Some("modello-x"), openai);
        assert_eq!(body["max_completion_tokens"], json!(500));
        assert!(body.get("max_tokens").is_none());
    }
}
