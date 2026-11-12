//! Invio di una conversazione a un modello e lettura della risposta in streaming.
//!
//! Parla l'API "chat completions" compatibile con OpenAI, quella esposta da
//! `llama-server` e dalla maggior parte dei provider cloud. L'API di Anthropic,
//! che ha una forma diversa, sta in `providers::anthropic`.

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
    /// Avviso da mostrare all'utente (risposta troncata, rifiuto del modello…).
    pub notice: Option<String>,
}

pub const NOTICE_TRUNCATED: &str =
    "La risposta è stata troncata: raggiunto il limite di token per risposta.";

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

/// Corpo della richiesta.
pub fn request_body(
    messages: &[ChatMessage],
    sampling: &SamplingParams,
    model: Option<&str>,
    flavor: Flavor,
) -> Value {
    let mut body = json!({
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if let Some(model) = model {
        body["model"] = json!(model);
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
    if let Some(choice) = value.pointer("/choices/0") {
        if let Some(delta) = choice.get("delta") {
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
        assert_eq!(outcome.notice, None);
    }

    #[test]
    fn segnala_la_risposta_troncata() {
        let mut outcome = ChatOutcome::default();
        let chunk = json!({"choices": [{"delta": {}, "finish_reason": "length"}]});
        apply_chunk(&chunk, &mut outcome, &mut |_| {});
        assert_eq!(outcome.notice.as_deref(), Some(NOTICE_TRUNCATED));
    }

    #[test]
    fn llama_riceve_tutti_i_parametri() {
        let body = request_body(&[], &SamplingParams::default(), None, Flavor::Llama);
        assert!(body.get("model").is_none());
        assert!(body.get("top_k").is_some());
        assert!(body.get("min_p").is_some());
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
        let body = request_body(&[], &neutral, Some("modello-x"), flavor);
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
        let body = request_body(&[], &custom, Some("modello-x"), flavor);
        assert_eq!(body["temperature"], json!(0.2f32));
        assert_eq!(body["max_tokens"], json!(500));

        let openai = Flavor::OpenAi {
            max_completion_tokens: true,
        };
        let body = request_body(&[], &custom, Some("modello-x"), openai);
        assert_eq!(body["max_completion_tokens"], json!(500));
        assert!(body.get("max_tokens").is_none());
    }
}
