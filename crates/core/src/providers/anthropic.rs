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

/// Corpo della richiesta.
///
/// Con `extras` si chiedono due comodità che non tutti i modelli accettano:
/// il riassunto del ragionamento (altrimenti invisibile) e il ripiego
/// automatico in caso di rifiuto. Se il servizio le respinge, la richiesta
/// viene ripetuta senza.
fn request_body(
    model: &str,
    max_output: Option<u64>,
    messages: &[ChatMessage],
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
        "messages": messages,
    });
    if !system_prompt.trim().is_empty() {
        body["system"] = json!(system_prompt);
    }
    if !sampling.stop.is_empty() {
        body["stop_sequences"] = json!(sampling.stop);
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
    system_prompt: &str,
    sampling: &SamplingParams,
    cancel: Arc<Notify>,
    mut on_event: impl FnMut(ChatEvent),
) -> Result<ChatOutcome> {
    let body = request_body(model, max_output, messages, system_prompt, sampling, true);
    let mut response = send(http, provider, api_key, &body).await?;

    // 400 = richiesta non valida: di solito un modello più vecchio che non
    // conosce le comodità richieste. Si riprova con la richiesta essenziale.
    if response.status().as_u16() == 400 {
        let plain = request_body(model, max_output, messages, system_prompt, sampling, false);
        response = send(http, provider, api_key, &plain).await?;
    }
    if !response.status().is_success() {
        return Err(chat::http_error(response).await);
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
            let Ok(event) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            apply_event(&event, &mut outcome, &mut on_event)?;
        }
    }
    Ok(outcome)
}

/// Applica un evento dello streaming all'esito in costruzione.
fn apply_event(
    event: &Value,
    outcome: &mut ChatOutcome,
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
            }
        }
        Some("content_block_delta") => {
            let delta = &event["delta"];
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => {
                    if let Some(text) = delta.get("text").and_then(Value::as_str) {
                        outcome.text.push_str(text);
                        on_event(ChatEvent::Text(text.to_string()));
                    }
                }
                Some("thinking_delta") => {
                    if let Some(text) = delta.get("thinking").and_then(Value::as_str)
                        && !text.is_empty()
                    {
                        on_event(ChatEvent::Reasoning(text.to_string()));
                    }
                }
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
        vec![ChatMessage {
            role: "user".into(),
            content: text.into(),
        }]
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
        let body = request_body("claude-opus-5-5", None, &user("ciao"), "", &sampling, true);
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
            request_body("m", max_output, &user("x"), "", sampling, false)["max_tokens"].clone()
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
        let with = request_body("claude-opus-5-5", None, &user("x"), "Sii breve.", &s, true);
        assert_eq!(with["thinking"]["display"], json!("summarized"));
        assert_eq!(with["fallbacks"], json!("default"));
        assert_eq!(with["system"], json!("Sii breve."));
        assert_eq!(with["stop_sequences"], json!(["FINE"]));

        // Un modello senza ripiego automatico.
        let haiku = request_body("claude-haiku-5-5", None, &user("x"), "", &s, true);
        assert!(haiku.get("fallbacks").is_none());

        // Richiesta essenziale, usata come secondo tentativo.
        let plain = request_body("claude-opus-5-5", None, &user("x"), "", &s, false);
        assert!(plain.get("thinking").is_none());
        assert!(plain.get("fallbacks").is_none());
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
        for event in &events {
            apply_event(event, &mut outcome, &mut |e| seen.push(e)).unwrap();
        }

        assert_eq!(outcome.text, "Ciao mondo");
        assert_eq!(seen[0], ChatEvent::Reasoning("Rifletto.".into()));
        assert_eq!(seen.len(), 3);
        assert_eq!(outcome.stats.prompt_tokens, Some(100));
        assert_eq!(outcome.stats.completion_tokens, Some(12));
        assert_eq!(outcome.notice, None);
    }

    #[test]
    fn rifiuto_troncamento_ed_errore_vengono_segnalati() {
        let stop = |reason: &str| {
            let mut outcome = ChatOutcome::default();
            let event = json!({"type": "message_delta", "delta": {"stop_reason": reason}});
            apply_event(&event, &mut outcome, &mut |_| {}).unwrap();
            outcome.notice
        };
        assert_eq!(stop("refusal").as_deref(), Some(NOTICE_REFUSAL));
        assert_eq!(stop("max_tokens").as_deref(), Some(NOTICE_TRUNCATED));

        let error = json!({"type": "error", "error": {"type": "overloaded_error",
            "message": "Overloaded"}});
        let result = apply_event(&error, &mut ChatOutcome::default(), &mut |_| {});
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
