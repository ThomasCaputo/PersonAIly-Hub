//! Endpoint `/v1/...` compatibili con OpenAI (VS-1): qualunque programma che
//! sa parlare con OpenAI può usare i modelli dell'app, indicando come
//! indirizzo `http://127.0.0.1:<porta>/v1` e come chiave il token dell'API.
//!
//! Nomi dei modelli: `local` per il modello locale caricato,
//! `<provider>/<modello>` per quelli cloud (l'elenco è in `GET /v1/models`).
//! Queste richieste non finiscono nella cronologia dell'app e gli strumenti
//! indicati dal client li esegue il client.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use app_core::chat::{ChatEvent, ChatMessage, ChatOutcome, Image, NOTICE_TRUNCATED};
use app_core::tools::{Risk, ToolCall, ToolDef};
use app_core::{ModelRef, SamplingOverrides};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc};

use crate::{ApiError, ApiResult, ApiState, Outgoing, on_disconnect, sse_json, sse_stream};

/// Nome del modello locale caricato.
pub(crate) const LOCAL_MODEL: &str = "local";

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Modelli ─────────────────────────────────────────────────────────────

async fn models(State(state): State<ApiState>) -> ApiResult<Json<Value>> {
    let loaded = state.core.status().await.loaded;
    let core = state.core.clone();
    let providers = tokio::task::spawn_blocking(move || core.providers())
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;

    let mut data = Vec::new();
    if let Some(loaded) = loaded {
        data.push(json!({
            "id": LOCAL_MODEL,
            "object": "model",
            "created": 0,
            "owned_by": "local",
            "name": loaded.model_name,
        }));
    }
    for info in providers.iter().filter(|p| p.has_key) {
        for model in &info.provider.models {
            data.push(json!({
                "id": format!("{}/{}", info.provider.id, model.id),
                "object": "model",
                "created": 0,
                "owned_by": info.provider.name,
                "name": if model.name.is_empty() { &model.id } else { &model.name },
            }));
        }
    }
    Ok(Json(json!({ "object": "list", "data": data })))
}

/// Dal nome del modello nella richiesta al modello dell'app.
pub(crate) fn parse_model(name: &str) -> Result<ModelRef, String> {
    let name = name.trim();
    if name == LOCAL_MODEL || name.starts_with("local/") {
        return Ok(ModelRef::Local);
    }
    match name.split_once('/') {
        Some((provider, model)) if !provider.is_empty() && !model.is_empty() => {
            Ok(ModelRef::Cloud {
                provider_id: provider.to_string(),
                model: model.to_string(),
            })
        }
        _ => Err(format!(
            "Modello sconosciuto: \"{name}\". Usa \"local\" per il modello locale caricato \
             oppure \"<provider>/<modello>\" come in GET /v1/models."
        )),
    }
}

// ── Richiesta ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct CompletionRequest {
    model: String,
    messages: Vec<InMessage>,
    #[serde(default)]
    stream: bool,
    stream_options: Option<StreamOptions>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    max_tokens: Option<u32>,
    max_completion_tokens: Option<u32>,
    seed: Option<i64>,
    stop: Option<Stop>,
    #[serde(default)]
    tools: Vec<InTool>,
}

#[derive(Debug, Deserialize)]
struct StreamOptions {
    #[serde(default)]
    include_usage: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Stop {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct InMessage {
    role: String,
    #[serde(default)]
    content: Option<Content>,
    #[serde(default)]
    tool_calls: Vec<InToolCall>,
    tool_call_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Parts(Vec<Part>),
}

#[derive(Debug, Deserialize)]
struct Part {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
    image_url: Option<ImageUrl>,
}

#[derive(Debug, Deserialize)]
struct ImageUrl {
    url: String,
}

#[derive(Debug, Deserialize)]
struct InToolCall {
    id: String,
    function: InFunctionCall,
}

#[derive(Debug, Deserialize)]
struct InFunctionCall {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct InTool {
    function: InToolFunction,
}

#[derive(Debug, Deserialize)]
struct InToolFunction {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    parameters: Value,
}

/// Un'immagine in forma `data:image/png;base64,...`. Gli indirizzi web non
/// si scaricano: il nucleo non va su internet per conto del client.
fn image_from_url(url: &str) -> Result<Image, String> {
    let rest = url
        .strip_prefix("data:")
        .ok_or("Le immagini vanno inviate come data URL in base64, non come indirizzo web.")?;
    let (media_type, data) = rest
        .split_once(";base64,")
        .ok_or("Immagine non valida: serve un data URL in base64.")?;
    Ok(Image {
        media_type: media_type.to_string(),
        data: data.to_string(),
    })
}

fn to_chat_messages(messages: Vec<InMessage>) -> Result<Vec<ChatMessage>, String> {
    messages
        .into_iter()
        .map(|m| {
            let role = match m.role.as_str() {
                // I modelli recenti di OpenAI chiamano così le istruzioni di sistema.
                "developer" => "system",
                "system" | "user" | "assistant" | "tool" => m.role.as_str(),
                other => return Err(format!("Ruolo del messaggio sconosciuto: \"{other}\".")),
            }
            .to_string();
            let mut text = String::new();
            let mut images = Vec::new();
            match m.content {
                None => {}
                Some(Content::Text(t)) => text = t,
                Some(Content::Parts(parts)) => {
                    for part in parts {
                        match (part.kind.as_str(), part.text, part.image_url) {
                            ("text", Some(t), _) => {
                                if !text.is_empty() {
                                    text.push('\n');
                                }
                                text.push_str(&t);
                            }
                            ("image_url", _, Some(image)) => {
                                images.push(image_from_url(&image.url)?)
                            }
                            (kind, _, _) => {
                                return Err(format!(
                                    "Parte del messaggio non gestita: \"{kind}\"."
                                ));
                            }
                        }
                    }
                }
            }
            let tool_calls = m
                .tool_calls
                .into_iter()
                .map(|call| ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments: serde_json::from_str(&call.function.arguments)
                        .unwrap_or(Value::String(call.function.arguments)),
                })
                .collect();
            Ok(ChatMessage {
                role,
                content: text,
                tool_calls,
                tool_call_id: m.tool_call_id,
                images,
                ..Default::default()
            })
        })
        .collect()
}

fn to_tool_defs(tools: Vec<InTool>) -> Vec<ToolDef> {
    tools
        .into_iter()
        .map(|tool| ToolDef {
            name: Cow::Owned(tool.function.name),
            description: Cow::Owned(tool.function.description),
            schema: if tool.function.parameters.is_object() {
                tool.function.parameters
            } else {
                json!({ "type": "object", "properties": {} })
            },
            // Li esegue il client: la classe di rischio qui non decide nulla.
            risk: Risk::Read,
        })
        .collect()
}

// ── Risposta ────────────────────────────────────────────────────────────

fn finish_reason(outcome: &ChatOutcome) -> &'static str {
    if !outcome.tool_calls.is_empty() {
        "tool_calls"
    } else if outcome.notice.as_deref() == Some(NOTICE_TRUNCATED) {
        "length"
    } else if outcome.notice.is_some() {
        "content_filter"
    } else {
        "stop"
    }
}

fn tool_calls_json(calls: &[ToolCall], with_index: bool) -> Vec<Value> {
    calls
        .iter()
        .enumerate()
        .map(|(i, call)| {
            let arguments = match &call.arguments {
                Value::String(raw) => raw.clone(),
                other => other.to_string(),
            };
            let mut value = json!({
                "id": call.id,
                "type": "function",
                "function": { "name": call.name, "arguments": arguments },
            });
            if with_index {
                value["index"] = json!(i);
            }
            value
        })
        .collect()
}

fn usage_json(outcome: &ChatOutcome) -> Value {
    let prompt = outcome.stats.prompt_tokens.unwrap_or(0);
    let completion = outcome.stats.completion_tokens.unwrap_or(0);
    json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": prompt + completion,
    })
}

fn completion_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("chatcmpl-{nanos:x}")
}

async fn chat_completions(
    State(state): State<ApiState>,
    Json(request): Json<CompletionRequest>,
) -> ApiResult<Response> {
    let model = parse_model(&request.model).map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e))?;
    let messages = to_chat_messages(request.messages).map_err(ApiError::bad_request)?;
    let tools = to_tool_defs(request.tools);
    let overrides = SamplingOverrides {
        temperature: request.temperature,
        top_p: request.top_p,
        max_tokens: request.max_completion_tokens.or(request.max_tokens),
        seed: request.seed,
        stop: request.stop.map(|stop| match stop {
            Stop::One(s) => vec![s],
            Stop::Many(v) => v,
        }),
    };
    // Si verifica subito che il modello sia raggiungibile: l'errore arriva
    // come risposta HTTP anche quando si chiede lo streaming.
    state.core.preset_for(&model)?;

    let id = completion_id();
    let created = now_secs();
    let name = request.model.clone();

    if !request.stream {
        let outcome = state
            .core
            .complete(
                &model,
                &messages,
                &tools,
                &overrides,
                Arc::new(Notify::new()),
                |_| {},
            )
            .await?;
        let mut message = json!({ "role": "assistant", "content": outcome.text });
        if !outcome.tool_calls.is_empty() {
            message["tool_calls"] = json!(tool_calls_json(&outcome.tool_calls, false));
        }
        let body = json!({
            "id": id,
            "object": "chat.completion",
            "created": created,
            "model": name,
            "choices": [{ "index": 0, "message": message, "finish_reason": finish_reason(&outcome) }],
            "usage": usage_json(&outcome),
        });
        return Ok(Json(body).into_response());
    }

    let include_usage = request.stream_options.is_some_and(|o| o.include_usage);
    let chunk = {
        let (id, name) = (id.clone(), name.clone());
        move |delta: Value, finish: Option<&str>| -> Event {
            sse_json(
                None,
                &json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": name,
                    "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }],
                }),
            )
        }
    };

    let (tx, rx) = mpsc::unbounded_channel::<Outgoing>();
    let cancel = Arc::new(Notify::new());
    let watcher = {
        let cancel = cancel.clone();
        on_disconnect(&tx, move || cancel.notify_one())
    };
    let core = state.core.clone();
    tokio::spawn(async move {
        let send = |event: Event| {
            let _ = tx.send(Outgoing { event, last: false });
        };
        send(chunk(json!({ "role": "assistant", "content": "" }), None));
        let result = core
            .complete(
                &model,
                &messages,
                &tools,
                &overrides,
                cancel,
                |event| match event {
                    ChatEvent::Text(text) => send(chunk(json!({ "content": text }), None)),
                    ChatEvent::Reasoning(text) => {
                        send(chunk(json!({ "reasoning_content": text }), None))
                    }
                    ChatEvent::Saved | ChatEvent::ToolRequest { .. } => {}
                },
            )
            .await;
        watcher.abort();
        match result {
            Ok(outcome) => {
                if !outcome.tool_calls.is_empty() {
                    send(chunk(
                        json!({ "tool_calls": tool_calls_json(&outcome.tool_calls, true) }),
                        None,
                    ));
                }
                send(chunk(json!({}), Some(finish_reason(&outcome))));
                if include_usage {
                    send(sse_json(
                        None,
                        &json!({
                            "id": id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": name,
                            "choices": [],
                            "usage": usage_json(&outcome),
                        }),
                    ));
                }
            }
            Err(e) => send(sse_json(
                None,
                &json!({ "error": { "message": e.to_string(), "type": "server_error" } }),
            )),
        }
        let _ = tx.send(Outgoing {
            event: Event::default().data("[DONE]"),
            last: true,
        });
    });
    Ok(Sse::new(sse_stream(rx))
        .keep_alive(KeepAlive::default())
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names() {
        assert_eq!(parse_model("local"), Ok(ModelRef::Local));
        assert_eq!(parse_model("local/qwen"), Ok(ModelRef::Local));
        assert_eq!(
            parse_model("openrouter-1a/anthropic/claude"),
            Ok(ModelRef::Cloud {
                provider_id: "openrouter-1a".into(),
                model: "anthropic/claude".into()
            })
        );
        assert!(parse_model("gpt-4o").is_err());
        assert!(parse_model("/x").is_err());
    }

    #[test]
    fn messages_from_openai_format() {
        let request: CompletionRequest = serde_json::from_value(json!({
            "model": "local",
            "messages": [
                { "role": "developer", "content": "Sii breve." },
                { "role": "user", "content": [
                    { "type": "text", "text": "Ciao" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
                ]},
                { "role": "assistant", "content": null, "tool_calls": [
                    { "id": "c1", "type": "function", "function": { "name": "f", "arguments": "{\"a\":1}" } }
                ]},
                { "role": "tool", "tool_call_id": "c1", "content": "42" }
            ],
            "stop": "FINE"
        }))
        .unwrap();
        let messages = to_chat_messages(request.messages).unwrap();
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].content, "Ciao");
        assert_eq!(messages[1].images[0].media_type, "image/png");
        assert_eq!(messages[2].tool_calls[0].arguments, json!({ "a": 1 }));
        assert_eq!(messages[3].tool_call_id.as_deref(), Some("c1"));
        assert!(matches!(request.stop, Some(Stop::One(_))));
    }

    #[test]
    fn web_images_are_refused() {
        assert!(image_from_url("https://example.com/a.png").is_err());
    }
}
