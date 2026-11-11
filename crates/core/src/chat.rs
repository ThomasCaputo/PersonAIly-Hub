//! Invio di una conversazione a un modello e lettura della risposta in streaming.
//!
//! Parla l'API "chat completions" compatibile con OpenAI, quella esposta da
//! `llama-server` e dalla maggior parte dei provider cloud.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::error::{Error, Result};
use crate::local::SamplingParams;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Misure di una risposta appena generata.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationStats {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub tokens_per_second: Option<f64>,
}

/// Ciò che arriva durante la generazione.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    /// Un pezzo di risposta.
    Text(String),
    /// Un pezzo di ragionamento (modelli che "pensano" prima di rispondere).
    Reasoning(String),
}

/// Esito di una generazione.
#[derive(Debug, Clone, Default)]
pub struct ChatOutcome {
    pub text: String,
    pub stats: GenerationStats,
    /// `true` se l'utente ha interrotto la risposta.
    pub cancelled: bool,
}

/// Corpo della richiesta.
pub fn request_body(messages: &[ChatMessage], sampling: &SamplingParams) -> Value {
    let mut body = json!({
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
        "temperature": sampling.temperature,
        "top_p": sampling.top_p,
        "top_k": sampling.top_k,
        "min_p": sampling.min_p,
        "repeat_penalty": sampling.repeat_penalty,
    });
    if let Some(seed) = sampling.seed {
        body["seed"] = json!(seed);
    }
    if let Some(max_tokens) = sampling.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    if !sampling.stop.is_empty() {
        body["stop"] = json!(sampling.stop);
    }
    body
}

/// Invia la conversazione e chiama `on_event` per ogni pezzo ricevuto.
///
/// `cancel` interrompe la generazione: chiudendo la connessione anche il
/// modello smette di lavorare.
pub async fn stream_chat(
    http: &reqwest::Client,
    base_url: &str,
    messages: &[ChatMessage],
    sampling: &SamplingParams,
    cancel: Arc<Notify>,
    mut on_event: impl FnMut(ChatEvent),
) -> Result<ChatOutcome> {
    let mut response = http
        .post(format!("{base_url}/v1/chat/completions"))
        .json(&request_body(messages, sampling))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(Error::msg(format!(
            "il modello ha risposto con un errore ({status}): {}",
            error_message(&body)
        )));
    }

    let mut outcome = ChatOutcome::default();
    let mut parser = SseParser::default();

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
            apply_chunk(&value, &mut outcome, &mut on_event);
        }
    }
    Ok(outcome)
}

/// Estrae da un pezzo della risposta testo, ragionamento e misure.
fn apply_chunk(value: &Value, outcome: &mut ChatOutcome, on_event: &mut impl FnMut(ChatEvent)) {
    if let Some(delta) = value.pointer("/choices/0/delta") {
        if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str)
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

fn error_message(body: &str) -> String {
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
struct SseParser {
    buffer: Vec<u8>,
}

impl SseParser {
    fn push(&mut self, chunk: &[u8]) -> Vec<String> {
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
        let chunks = [
            json!({"choices": [{"delta": {"reasoning_content": "penso"}}]}),
            json!({"choices": [{"delta": {"content": "Ciao"}}]}),
            json!({"choices": [{"delta": {"content": " mondo"}}]}),
            json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 2},
                   "timings": {"predicted_per_second": 41.5}}),
        ];
        for chunk in &chunks {
            apply_chunk(chunk, &mut outcome, &mut |e| events.push(e));
        }

        assert_eq!(outcome.text, "Ciao mondo");
        assert_eq!(events[0], ChatEvent::Reasoning("penso".into()));
        assert_eq!(events.len(), 3);
        assert_eq!(outcome.stats.prompt_tokens, Some(12));
        assert_eq!(outcome.stats.completion_tokens, Some(2));
        assert_eq!(outcome.stats.tokens_per_second, Some(41.5));
    }

    #[test]
    fn i_parametri_facoltativi_non_viaggiano_se_assenti() {
        let body = request_body(&[], &SamplingParams::default());
        assert!(body.get("seed").is_none());
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("stop").is_none());
        assert_eq!(body["stream"], json!(true));

        let body = request_body(
            &[],
            &SamplingParams {
                seed: Some(7),
                max_tokens: Some(100),
                stop: vec!["FINE".into()],
                ..Default::default()
            },
        );
        assert_eq!(body["seed"], json!(7));
        assert_eq!(body["max_tokens"], json!(100));
        assert_eq!(body["stop"], json!(["FINE"]));
    }
}
