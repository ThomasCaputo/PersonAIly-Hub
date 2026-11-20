//! API Messages di Anthropic (modelli Claude), in streaming.
//!
//! Riferimento: <https://platform.claude.com/docs/en/api/messages>.
//! Non esiste un SDK ufficiale per Rust: si parla direttamente HTTP.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::Notify;

use super::{CloudModel, Provider};
use crate::chat::{self, ChatEvent, ChatMessage, ChatOutcome, NOTICE_TRUNCATED, SseParser};
use crate::error::{Error, Result};
use crate::local::SamplingParams;
use crate::tools::{ToolCall, ToolDef};

/// Versione dell'API. Da questa versione le richieste non accettano più
/// `temperature`, `top_p` e `top_k`: per questo non vengono mai inviati.
const API_VERSION: &str = "2026-10-21";

/// Funzione in anteprima: se il modello rifiuta una richiesta per i suoi
/// filtri di sicurezza, il servizio la ripete su un altro modello Claude.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Limite di token per risposta quando l'utente non ne imposta uno.
const DEFAULT_MAX_TOKENS: u64 = 64_000;

const NOTICE_REFUSAL: &str = "Il modello ha rifiutato di rispondere a questa richiesta.";

/// I modelli per cui il servizio offre il ripiego automatico in caso di rifiuto.
fn supports_fallback(model: &str) -> bool {
    [
        "claude-fable-5-2",
        "claude-fable-5-1",
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-sonnet-5-5",
    ]
    .iter()
    .any(|prefix| model.starts_with(prefix))
}

/// Un identificatore di chiamata accettato da Anthropic: lettere, cifre, `_` e `-`.
/// Serve quando la chiamata è nata da un altro provider, a conversazione già avviata.
fn safe_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn is_thinking(block: &Value) -> bool {
    matches!(
        block.get("type").and_then(Value::as_str),
        Some("thinking" | "redacted_thinking")
    )
}

/// I messaggi nel formato di Anthropic.
///
/// - I risultati degli strumenti viaggiano in un messaggio `user`, tutti
///   insieme, subito dopo la risposta che li ha chiesti.
/// - Una risposta i cui strumenti sono ancora in corso torna indietro così
///   com'è arrivata ([`ChatMessage::native`]), ragionamento compreso: il
///   servizio lo richiede. Delle risposte più vecchie resta solo il contenuto.
fn api_messages(messages: &[ChatMessage], keep_thinking: bool) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut results: Vec<Value> = Vec::new();

    for m in messages {
        if m.role == "tool" {
            let mut result = json!({
                "type": "tool_result",
                "tool_use_id": safe_id(m.tool_call_id.as_deref().unwrap_or_default()),
                "content": m.content,
            });
            if m.is_error {
                result["is_error"] = json!(true);
            }
            results.push(result);
            continue;
        }
        if !results.is_empty() {
            out.push(json!({ "role": "user", "content": std::mem::take(&mut results) }));
        }

        if m.role == "assistant" {
            let blocks: Vec<Value> = match &m.native {
                Some(Value::Array(native)) => native
                    .iter()
                    .filter(|block| keep_thinking || !is_thinking(block))
                    .cloned()
                    .collect(),
                _ => {
                    let mut blocks = Vec::new();
                    if !m.content.is_empty() {
                        blocks.push(json!({ "type": "text", "text": m.content }));
                    }
                    for call in &m.tool_calls {
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": safe_id(&call.id),
                            "name": call.name,
                            "input": if call.arguments.is_object() { call.arguments.clone() } else { json!({}) },
                        }));
                    }
                    blocks
                }
            };
            // Un messaggio senza contenuto verrebbe rifiutato.
            if !blocks.is_empty() {
                out.push(json!({ "role": "assistant", "content": blocks }));
            }
        } else if m.images.is_empty() {
            out.push(json!({ "role": "user", "content": m.content }));
        } else {
            let mut blocks: Vec<Value> = m
                .images
                .iter()
                .map(|image| json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": image.media_type, "data": image.data },
                }))
                .collect();
            blocks.push(json!({ "type": "text", "text": m.content }));
            out.push(json!({ "role": "user", "content": blocks }));
        }
    }
    if !results.is_empty() {
        out.push(json!({ "role": "user", "content": results }));
    }
    out
}

/// Corpo della richiesta.
///
/// Con `extras` si chiedono comodità che non tutti i modelli accettano: il
/// riassunto del ragionamento (altrimenti invisibile), il ripiego automatico
/// in caso di rifiuto e l'arrivo graduale dei parametri degli strumenti. Se il
/// servizio le respinge, la richiesta viene ripetuta senza.
fn request_body(
    model: &str,
    max_output: Option<u64>,
    messages: &[ChatMessage],
    tools: &[ToolDef],
    system_prompt: &str,
    sampling: &SamplingParams,
    extras: bool,
) -> Value {
    let ceiling = max_output.unwrap_or(DEFAULT_MAX_TOKENS);
    let max_tokens = match sampling.max_tokens {
        Some(n) => (n as u64).min(ceiling),
        None => ceiling.min(DEFAULT_MAX_TOKENS),
    };

    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "stream": true,
        // Senza le comodità non si chiede il ragionamento, quindi quello
        // delle risposte in corso non si rimanda.
        "messages": api_messages(messages, extras),
    });
    // Prompt caching: il servizio rilegge strumenti, prompt di sistema e
    // cronologia a ogni passo; dalla cache costano circa un decimo. Un punto
    // fisso alla fine del prompt di sistema (copre anche gli strumenti, che
    // vengono prima) e, con le comodità, quello automatico che segue la
    // conversazione mentre cresce. Strumenti e prompt sono costruiti sempre
    // nello stesso ordine: altrimenti la cache non servirebbe.
    if !system_prompt.trim().is_empty() {
        body["system"] = json!([{
            "type": "text",
            "text": system_prompt,
            "cache_control": { "type": "ephemeral" },
        }]);
    }
    if extras {
        body["cache_control"] = json!({ "type": "ephemeral" });
    }
    if !sampling.stop.is_empty() {
        body["stop_sequences"] = json!(sampling.stop);
    }
    if !tools.is_empty() {
        body["tools"] = tools
            .iter()
            .map(|tool| {
                let mut def = json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.schema,
                });
                if extras {
                    // Il contenuto di un file da scrivere arriva mentre viene
                    // generato, invece che tutto alla fine dopo una lunga pausa.
                    def["eager_input_streaming"] = json!(true);
                }
                def
            })
            .collect();
    }
    if extras {
        body["thinking"] = json!({ "type": "adaptive", "display": "summarized" });
        if supports_fallback(model) {
            body["fallbacks"] = json!("default");
        }
    }
    body
}

async fn send(
    http: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
    body: &Value,
) -> Result<reqwest::Response> {
    let mut request = http
        .post(format!("{}/v1/messages", provider.base()))
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .json(body);
    if body.get("fallbacks").is_some() {
        request = request.header("anthropic-beta", FALLBACK_BETA);
    }
    Ok(request.send().await?)
}

#[allow(clippy::too_many_arguments)]
pub async fn stream(
    http: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
    model: &str,
    max_output: Option<u64>,
    messages: &[ChatMessage],
    tools: &[ToolDef],
    system_prompt: &str,
    sampling: &SamplingParams,
    cancel: Arc<Notify>,
    mut on_event: impl FnMut(ChatEvent),
) -> Result<ChatOutcome> {
    let body = |extras| {
        request_body(
            model,
            max_output,
            messages,
            tools,
            system_prompt,
            sampling,
            extras,
        )
    };
    let mut response = send(http, provider, api_key, &body(true)).await?;

    // 400 = richiesta non valida: di solito un modello più vecchio che non
    // conosce le comodità richieste. Si riprova con la richiesta essenziale.
    if response.status().as_u16() == 400 {
        response = send(http, provider, api_key, &body(false)).await?;
    }
    if !response.status().is_success() {
        return Err(chat::http_error(response).await);
    }

    let mut outcome = ChatOutcome::default();
    let mut blocks = Blocks::default();
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
            let Ok(event) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            apply_event(&event, &mut outcome, &mut blocks, &mut on_event)?;
        }
    }
    // Una risposta interrotta a metà può contenere chiamate incomplete: non si eseguono.
    if !outcome.cancelled {
        blocks.finish(&mut outcome);
    }
    Ok(outcome)
}

/// Un blocco di contenuto della risposta mentre arriva.
#[derive(Debug, Default)]
struct Block {
    /// Il blocco com'è stato annunciato dal servizio.
    start: Value,
    /// Testo, ragionamento o parametri JSON, accumulati pezzo dopo pezzo.
    text: String,
    signature: String,
}

/// I blocchi della risposta in costruzione, nell'ordine deciso dal servizio.
#[derive(Debug, Default)]
struct Blocks(Vec<Block>);

impl Blocks {
    fn at(&mut self, event: &Value) -> &mut Block {
        let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        if self.0.len() <= index {
            self.0.resize_with(index + 1, Block::default);
        }
        &mut self.0[index]
    }

    /// Ricompone la risposta nel formato del servizio ed estrae le chiamate
    /// agli strumenti.
    fn finish(self, outcome: &mut ChatOutcome) {
        let mut native = Vec::new();
        for block in self.0 {
            let mut value = block.start;
            match value.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    // Senza pezzi in arrivo valgono i parametri annunciati all'inizio.
                    let arguments = if block.text.trim().is_empty() && value["input"].is_object() {
                        value["input"].clone()
                    } else {
                        chat::parse_arguments(&block.text)
                    };
                    value["input"] = if arguments.is_object() {
                        arguments.clone()
                    } else {
                        json!({})
                    };
                    outcome.tool_calls.push(ToolCall {
                        id: value["id"].as_str().unwrap_or_default().to_string(),
                        name: value["name"].as_str().unwrap_or_default().to_string(),
                        arguments,
                    });
                }
                Some("thinking") => {
                    value["thinking"] = json!(block.text);
                    value["signature"] = json!(block.signature);
                }
                // Un blocco di testo vuoto verrebbe rifiutato al giro successivo.
                Some("text") if block.text.is_empty() => continue,
                Some("text") => value = json!({ "type": "text", "text": block.text }),
                // Blocchi senza annuncio (non dovrebbe succedere) si ignorano.
                None => continue,
                // Ogni altro tipo torna indietro così com'è arrivato.
                Some(_) => {}
            }
            native.push(value);
        }
        if !outcome.tool_calls.is_empty() {
            outcome.native = Some(Value::Array(native));
        }
    }
}

/// Applica un evento dello streaming all'esito in costruzione.
fn apply_event(
    event: &Value,
    outcome: &mut ChatOutcome,
    blocks: &mut Blocks,
    on_event: &mut impl FnMut(ChatEvent),
) -> Result<()> {
    match event.get("type").and_then(Value::as_str) {
        Some("message_start") => {
            // I token letti comprendono quelli serviti dalla cache del servizio.
            if let Some(usage) = event.pointer("/message/usage") {
                let read = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
                outcome.stats.prompt_tokens = Some(
                    read("input_tokens")
                        + read("cache_read_input_tokens")
                        + read("cache_creation_input_tokens"),
                );
                outcome.stats.cache_read_tokens = Some(read("cache_read_input_tokens"));
                outcome.stats.cache_write_tokens = Some(read("cache_creation_input_tokens"));
            }
        }
        Some("content_block_start") => {
            blocks.at(event).start = event["content_block"].clone();
        }
        Some("content_block_delta") => {
            let delta = &event["delta"];
            let block = blocks.at(event);
            let piece = |key: &str| delta.get(key).and_then(Value::as_str).unwrap_or("");
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => {
                    let text = piece("text");
                    if block.start.is_null() {
                        block.start = json!({ "type": "text" });
                    }
                    block.text.push_str(text);
                    outcome.text.push_str(text);
                    on_event(ChatEvent::Text(text.to_string()));
                }
                Some("thinking_delta") => {
                    let text = piece("thinking");
                    block.text.push_str(text);
                    if !text.is_empty() {
                        on_event(ChatEvent::Reasoning(text.to_string()));
                    }
                }
                Some("signature_delta") => block.signature.push_str(piece("signature")),
                Some("input_json_delta") => block.text.push_str(piece("partial_json")),
                _ => {}
            }
        }
        Some("message_delta") => {
            if let Some(n) = event
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64)
            {
                outcome.stats.completion_tokens = Some(n);
            }
            match event.pointer("/delta/stop_reason").and_then(Value::as_str) {
                Some("max_tokens") => outcome.notice = Some(NOTICE_TRUNCATED.to_string()),
                Some("refusal") => outcome.notice = Some(NOTICE_REFUSAL.to_string()),
                _ => {}
            }
        }
        Some("error") => {
            let message = event
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("errore sconosciuto");
            return Err(Error::msg(format!(
                "Il servizio ha interrotto la risposta: {message}"
            )));
        }
        _ => {}
    }
    Ok(())
}

/// Scarica l'elenco dei modelli, una pagina alla volta.
pub async fn fetch_models(
    http: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
) -> Result<Vec<CloudModel>> {
    let mut models = Vec::new();
    let mut after: Option<String> = None;

    // Il limite di pagine evita un ciclo infinito se il servizio risponde in modo anomalo.
    for _ in 0..20 {
        // Gli identificatori dei modelli contengono solo lettere, cifre, punti e
        // trattini: si possono mettere nell'indirizzo così come sono.
        let mut url = format!("{}/v1/models?limit=100", provider.base());
        if let Some(after) = &after {
            url.push_str(&format!("&after_id={after}"));
        }
        let response = http
            .get(url)
            .header("x-api-key", api_key)
            .header("anthropic-version", API_VERSION)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(chat::http_error(response).await);
        }
        let page: Value = response.json().await?;
        models.extend(parse_models(&page));

        let has_more = page
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        after = page
            .get("last_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        if !has_more || after.is_none() {
            break;
        }
    }
    Ok(models)
}

/// Prezzi di listino dei modelli Claude, in dollari per milione di token
/// (letti, generati), aggiornati a novembre 2026. L'API non li comunica, quindi
/// stanno qui: vanno rivisti quando escono nuovi modelli o cambiano i listini.
/// Fonte: <https://platform.claude.com/docs/en/about-claude/pricing>.
///
/// I prefissi più specifici vengono prima: `claude-opus-5-5` va riconosciuto
/// prima di `claude-opus-5`.
const LIST_PRICES: &[(&str, f64, f64)] = &[
    ("claude-fable-5-2", 8.0, 40.0),
    ("claude-mythos-5-2", 8.0, 40.0),
    ("claude-fable-5", 10.0, 50.0),
    ("claude-mythos-5", 10.0, 50.0),
    ("claude-opus-5-5", 4.0, 20.0),
    ("claude-opus-5", 5.0, 25.0),
    ("claude-opus-4-8", 5.0, 25.0),
    ("claude-opus-4-7", 5.0, 25.0),
    ("claude-opus-4-6", 5.0, 25.0),
    ("claude-sonnet-5", 2.0, 10.0),
    ("claude-sonnet-4-6", 3.0, 15.0),
    // Fino a 100 000 token di conversazione; oltre, il listino è più alto.
    ("claude-haiku-5-5", 0.10, 0.50),
    ("claude-haiku-4-5", 1.0, 5.0),
];

fn list_price(model: &str) -> Option<(f64, f64)> {
    LIST_PRICES
        .iter()
        .find(|(prefix, _, _)| model.starts_with(prefix))
        .map(|&(_, input, output)| (input, output))
}

fn parse_models(page: &Value) -> Vec<CloudModel> {
    page.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            Some(CloudModel {
                name: m
                    .get("display_name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string(),
                context_length: m.get("max_input_tokens").and_then(Value::as_u64),
                max_output_tokens: m.get("max_tokens").and_then(Value::as_u64),
                input_price: list_price(&id).map(|p| p.0),
                output_price: list_price(&id).map(|p| p.1),
                id,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Vec<ChatMessage> {
        vec![ChatMessage::new("user", text)]
    }

    #[test]
    fn non_invia_mai_i_parametri_di_campionamento() {
        let sampling = SamplingParams {
            temperature: 0.1,
            top_p: 0.5,
            top_k: 5,
            seed: Some(3),
            ..Default::default()
        };
        let body = request_body(
            "claude-opus-5-5",
            None,
            &user("ciao"),
            &[],
            "",
            &sampling,
            true,
        );
        for key in ["temperature", "top_p", "top_k", "seed", "min_p", "system"] {
            assert!(body.get(key).is_none(), "{key} non dovrebbe esserci");
        }
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["messages"][0]["role"], json!("user"));
    }

    #[test]
    fn il_limite_di_token_rispetta_il_massimo_del_modello() {
        let none = SamplingParams::default();
        let body = |max_output, sampling: &SamplingParams| {
            request_body("m", max_output, &user("x"), &[], "", sampling, false)["max_tokens"]
                .clone()
        };
        // Senza indicazioni: 64 000, o meno se il modello non ci arriva.
        assert_eq!(body(None, &none), json!(64_000));
        assert_eq!(body(Some(128_000), &none), json!(64_000));
        assert_eq!(body(Some(8_192), &none), json!(8_192));
        // Con un limite dell'utente: quello, ma mai oltre il massimo del modello.
        let wants = |n| SamplingParams {
            max_tokens: Some(n),
            ..Default::default()
        };
        assert_eq!(body(Some(128_000), &wants(100_000)), json!(100_000));
        assert_eq!(body(Some(8_192), &wants(100_000)), json!(8_192));
    }

    #[test]
    fn le_comodita_si_chiedono_solo_dove_esistono() {
        let s = SamplingParams {
            stop: vec!["FINE".into()],
            ..Default::default()
        };
        let with = request_body(
            "claude-opus-5-5",
            None,
            &user("x"),
            &[],
            "Sii breve.",
            &s,
            true,
        );
        assert_eq!(with["thinking"]["display"], json!("summarized"));
        assert_eq!(with["fallbacks"], json!("default"));
        // Prompt caching: punto fisso sul prompt di sistema più quello automatico.
        assert_eq!(with["system"][0]["text"], json!("Sii breve."));
        assert_eq!(
            with["system"][0]["cache_control"]["type"],
            json!("ephemeral")
        );
        assert_eq!(with["cache_control"]["type"], json!("ephemeral"));
        assert_eq!(with["stop_sequences"], json!(["FINE"]));

        // Un modello senza ripiego automatico.
        let haiku = request_body("claude-haiku-5-5", None, &user("x"), &[], "", &s, true);
        assert!(haiku.get("fallbacks").is_none());

        // Richiesta essenziale, usata come secondo tentativo.
        let plain = request_body("claude-opus-5-5", None, &user("x"), &[], "", &s, false);
        assert!(plain.get("thinking").is_none());
        assert!(plain.get("fallbacks").is_none());
        assert!(plain.get("cache_control").is_none());
    }

    #[test]
    fn ricostruisce_la_risposta_dagli_eventi() {
        let events = [
            json!({"type": "message_start", "message": {"usage":
                {"input_tokens": 10, "cache_read_input_tokens": 90}}}),
            json!({"type": "content_block_start", "index": 0,
                "content_block": {"type": "thinking", "thinking": ""}}),
            json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "thinking_delta", "thinking": "Rifletto."}}),
            json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "signature_delta", "signature": "abc"}}),
            json!({"type": "content_block_delta", "index": 1,
                "delta": {"type": "text_delta", "text": "Ciao"}}),
            json!({"type": "content_block_delta", "index": 1,
                "delta": {"type": "text_delta", "text": " mondo"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 12}}),
            json!({"type": "message_stop"}),
        ];
        let mut outcome = ChatOutcome::default();
        let mut seen = Vec::new();
        let mut blocks = Blocks::default();
        for event in &events {
            apply_event(event, &mut outcome, &mut blocks, &mut |e| seen.push(e)).unwrap();
        }

        assert_eq!(outcome.text, "Ciao mondo");
        assert_eq!(seen[0], ChatEvent::Reasoning("Rifletto.".into()));
        assert_eq!(seen.len(), 3);
        assert_eq!(outcome.stats.prompt_tokens, Some(100));
        assert_eq!(outcome.stats.completion_tokens, Some(12));
        assert_eq!(outcome.notice, None);
    }

    #[test]
    fn ricompone_le_chiamate_e_conserva_il_ragionamento() {
        let events = [
            json!({"type": "content_block_start", "index": 0,
                "content_block": {"type": "thinking", "thinking": ""}}),
            json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "thinking_delta", "thinking": "Leggo il file."}}),
            json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "signature_delta", "signature": "firma"}}),
            json!({"type": "content_block_start", "index": 1,
                "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1,
                "delta": {"type": "text_delta", "text": "Un attimo."}}),
            json!({"type": "content_block_start", "index": 2,
                "content_block": {"type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {}}}),
            json!({"type": "content_block_delta", "index": 2,
                "delta": {"type": "input_json_delta", "partial_json": "{\"path\": \"no"}}),
            json!({"type": "content_block_delta", "index": 2,
                "delta": {"type": "input_json_delta", "partial_json": "te.txt\"}"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
        ];
        let mut outcome = ChatOutcome::default();
        let mut blocks = Blocks::default();
        for event in &events {
            apply_event(event, &mut outcome, &mut blocks, &mut |_| {}).unwrap();
        }
        blocks.finish(&mut outcome);

        assert_eq!(outcome.tool_calls.len(), 1);
        assert_eq!(outcome.tool_calls[0].id, "toolu_1");
        assert_eq!(outcome.tool_calls[0].arguments, json!({"path": "note.txt"}));
        let native = outcome.native.clone().unwrap();
        assert_eq!(
            native[0],
            json!({"type": "thinking", "thinking": "Leggo il file.", "signature": "firma"})
        );
        assert_eq!(native[1], json!({"type": "text", "text": "Un attimo."}));
        assert_eq!(native[2]["input"], json!({"path": "note.txt"}));

        // Il giro successivo: la risposta torna identica, i risultati in un unico messaggio utente.
        let history = vec![
            ChatMessage::new("user", "Leggi note.txt e cerca sul web"),
            ChatMessage {
                tool_calls: outcome.tool_calls.clone(),
                native: outcome.native.clone(),
                ..ChatMessage::new("assistant", "Un attimo.")
            },
            ChatMessage {
                tool_call_id: Some("toolu_1".into()),
                ..ChatMessage::new("tool", "pane")
            },
            ChatMessage {
                tool_call_id: Some("call.2".into()),
                is_error: true,
                ..ChatMessage::new("tool", "negato")
            },
        ];
        let sent = api_messages(&history, true);
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[1]["content"], native);
        assert_eq!(sent[2]["role"], json!("user"));
        assert_eq!(sent[2]["content"][0]["tool_use_id"], json!("toolu_1"));
        assert!(sent[2]["content"][0].get("is_error").is_none());
        // Identificatori nati da un altro provider resi accettabili.
        assert_eq!(sent[2]["content"][1]["tool_use_id"], json!("call_2"));
        assert_eq!(sent[2]["content"][1]["is_error"], json!(true));

        // Nella richiesta essenziale il ragionamento non si rimanda.
        let plain = api_messages(&history, false);
        assert_eq!(plain[1]["content"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn una_risposta_senza_ragionamento_salvato_si_ricostruisce() {
        let history = vec![
            ChatMessage::new("user", "ciao"),
            ChatMessage {
                tool_calls: vec![ToolCall {
                    id: "call_0".into(),
                    name: "web_search".into(),
                    arguments: json!("rotto"),
                }],
                ..ChatMessage::new("assistant", "")
            },
            ChatMessage {
                tool_call_id: Some("call_0".into()),
                ..ChatMessage::new("tool", "risultati")
            },
            ChatMessage::new("assistant", "Ecco."),
        ];
        let sent = api_messages(&history, true);
        assert_eq!(
            sent[1]["content"],
            json!([{"type": "tool_use", "id": "call_0", "name": "web_search", "input": {}}])
        );
        assert_eq!(
            sent[3]["content"],
            json!([{"type": "text", "text": "Ecco."}])
        );
    }

    #[test]
    fn rifiuto_troncamento_ed_errore_vengono_segnalati() {
        let stop = |reason: &str| {
            let mut outcome = ChatOutcome::default();
            let event = json!({"type": "message_delta", "delta": {"stop_reason": reason}});
            apply_event(&event, &mut outcome, &mut Blocks::default(), &mut |_| {}).unwrap();
            outcome.notice
        };
        assert_eq!(stop("refusal").as_deref(), Some(NOTICE_REFUSAL));
        assert_eq!(stop("max_tokens").as_deref(), Some(NOTICE_TRUNCATED));

        let error = json!({"type": "error", "error": {"type": "overloaded_error",
            "message": "Overloaded"}});
        let result = apply_event(
            &error,
            &mut ChatOutcome::default(),
            &mut Blocks::default(),
            &mut |_| {},
        );
        assert!(result.unwrap_err().to_string().contains("Overloaded"));
    }

    #[test]
    fn legge_l_elenco_dei_modelli() {
        let models = parse_models(&json!({
            "data": [{"id": "claude-opus-5-5", "display_name": "Claude Opus 5.5",
                      "max_input_tokens": 1_000_000, "max_tokens": 128_000}],
            "has_more": false, "last_id": "claude-opus-5-5"
        }));
        assert_eq!(models[0].name, "Claude Opus 5.5");
        assert_eq!(models[0].context_length, Some(1_000_000));
        assert_eq!(models[0].max_output_tokens, Some(128_000));
        assert_eq!(models[0].input_price, Some(4.0));
        assert_eq!(models[0].output_price, Some(20.0));
    }

    #[test]
    fn i_prezzi_di_listino_distinguono_le_versioni() {
        assert_eq!(list_price("claude-opus-5-5"), Some((4.0, 20.0)));
        assert_eq!(list_price("claude-opus-5"), Some((5.0, 25.0)));
        assert_eq!(list_price("claude-fable-5-2"), Some((8.0, 40.0)));
        assert_eq!(list_price("claude-fable-5-1"), Some((10.0, 50.0)));
        assert_eq!(list_price("claude-sonnet-5-5"), Some((2.0, 10.0)));
        // Un modello sconosciuto non ha prezzo: meglio nessuna stima che una sbagliata.
        assert_eq!(list_price("claude-futuro-9"), None);
    }
}
