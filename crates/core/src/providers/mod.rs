//! Provider cloud: servizi esterni che offrono modelli tramite API.
//!
//! Due famiglie di API: quella di Anthropic e quella "compatibile con OpenAI",
//! parlata da OpenAI, OpenRouter, Groq, Mistral e molti altri.

pub mod anthropic;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

use crate::chat::{self, ChatEvent, ChatMessage, ChatOutcome, Flavor};
use crate::error::{Error, Result};
use crate::local::SamplingParams;
use crate::tools::ToolDef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
    OpenaiCompatible,
}

impl ProviderKind {
    /// Parametri di generazione che questa famiglia di API accetta, con i nomi
    /// usati dall'interfaccia. Gli altri non vengono né mostrati né inviati.
    pub fn supported_params(self) -> &'static [&'static str] {
        match self {
            // I modelli Claude attuali rifiutano temperatura, top-p e top-k.
            Self::Anthropic => &["maxTokens", "stop"],
            Self::OpenaiCompatible => &["temperature", "topP", "maxTokens", "seed", "stop"],
        }
    }
}

/// Un modello offerto da un provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CloudModel {
    pub id: String,
    /// Nome leggibile, se il provider lo fornisce.
    pub name: String,
    pub context_length: Option<u64>,
    pub max_output_tokens: Option<u64>,
    /// Prezzo in dollari per milione di token letti, se noto.
    #[serde(default)]
    pub input_price: Option<f64>,
    /// Prezzo in dollari per milione di token generati, se noto.
    #[serde(default)]
    pub output_price: Option<f64>,
}

impl CloudModel {
    /// Costo stimato in dollari di una risposta. `None` se manca il prezzo o
    /// il conteggio dei token. È una stima: non tiene conto di sconti, cache
    /// o tariffe speciali applicate dal servizio.
    pub fn cost(&self, prompt_tokens: Option<u64>, completion_tokens: Option<u64>) -> Option<f64> {
        let input = prompt_tokens? as f64 * self.input_price?;
        let output = completion_tokens? as f64 * self.output_price?;
        Some((input + output) / 1_000_000.0)
    }

    /// Come [`CloudModel::cost`], tenendo conto della cache del servizio: i
    /// token scritti in cache costano 1,25 volte, quelli letti un decimo.
    /// Alcuni modelli recenti leggono la cache a meno: la stima resta prudente.
    pub fn cost_with_cache(&self, stats: &crate::chat::GenerationStats) -> Option<f64> {
        let read = stats.cache_read_tokens.unwrap_or(0);
        let write = stats.cache_write_tokens.unwrap_or(0);
        let plain = stats.prompt_tokens?.saturating_sub(read + write);
        let base = self.cost(Some(plain), stats.completion_tokens)?;
        let price = self.input_price?;
        Some(base + (write as f64 * 1.25 + read as f64 * 0.1) * price / 1_000_000.0)
    }
}

/// Prezzo "per token" scritto come testo (formato di OpenRouter) → dollari
/// per milione di token.
fn price_per_million(value: Option<&Value>) -> Option<f64> {
    let per_token: f64 = value?.as_str()?.parse().ok()?;
    (per_token >= 0.0).then_some(per_token * 1_000_000.0)
}

/// Un provider configurato dall'utente. La chiave API non sta qui ma nel
/// portachiavi di sistema (vedi `secrets`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    /// Indirizzo dell'API, senza barra finale. Per i servizi compatibili con
    /// OpenAI comprende il `/v1`.
    pub base_url: String,
    /// Elenco dei modelli, come scaricato l'ultima volta.
    #[serde(default)]
    pub models: Vec<CloudModel>,
}

impl Provider {
    pub fn base(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }

    pub fn model(&self, id: &str) -> Option<&CloudModel> {
        self.models.iter().find(|m| m.id == id)
    }
}

/// Scarica l'elenco dei modelli del provider.
pub async fn fetch_models(
    http: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
) -> Result<Vec<CloudModel>> {
    let mut models = match provider.kind {
        ProviderKind::Anthropic => anthropic::fetch_models(http, provider, api_key).await?,
        ProviderKind::OpenaiCompatible => {
            let response = http
                .get(format!("{}/models", provider.base()))
                .bearer_auth(api_key)
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(chat::http_error(response).await);
            }
            parse_openai_models(&response.json::<Value>().await?)
        }
    };
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

/// Elenco modelli nel formato OpenAI: `{"data": [{"id": "..."}]}`. Alcuni
/// servizi (OpenRouter) aggiungono nome e lunghezza del contesto.
fn parse_openai_models(value: &Value) -> Vec<CloudModel> {
    value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            Some(CloudModel {
                name: m
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string(),
                context_length: m.get("context_length").and_then(Value::as_u64),
                max_output_tokens: m
                    .pointer("/top_provider/max_completion_tokens")
                    .and_then(Value::as_u64),
                input_price: price_per_million(m.pointer("/pricing/prompt")),
                output_price: price_per_million(m.pointer("/pricing/completion")),
                id,
            })
        })
        .collect()
}

/// Invia una conversazione a un modello del provider, in streaming.
#[allow(clippy::too_many_arguments)]
pub async fn stream(
    http: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
    model: &str,
    messages: &[ChatMessage],
    tools: &[ToolDef],
    system_prompt: &str,
    sampling: &SamplingParams,
    cancel: Arc<Notify>,
    on_event: impl FnMut(ChatEvent),
) -> Result<ChatOutcome> {
    match provider.kind {
        ProviderKind::Anthropic => {
            let max_output = provider.model(model).and_then(|m| m.max_output_tokens);
            anthropic::stream(
                http,
                provider,
                api_key,
                model,
                max_output,
                messages,
                tools,
                system_prompt,
                sampling,
                cancel,
                on_event,
            )
            .await
        }
        ProviderKind::OpenaiCompatible => {
            let mut all = Vec::with_capacity(messages.len() + 1);
            if !system_prompt.trim().is_empty() {
                all.push(ChatMessage::new("system", system_prompt));
            }
            all.extend_from_slice(messages);

            let flavor = Flavor::OpenAi {
                max_completion_tokens: provider.base().contains("api.openai.com"),
            };
            let body = chat::request_body(&all, tools, sampling, Some(model), flavor);
            let url = format!("{}/chat/completions", provider.base());
            chat::stream_chat(http, &url, Some(api_key), &body, cancel, on_event).await
        }
    }
}

/// Identificatore per un nuovo provider: leggibile e unico.
pub fn new_id(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!(
        "{}-{millis:x}",
        if slug.is_empty() { "provider" } else { slug }
    )
}

/// Controlla i dati inseriti dall'utente prima di salvarli.
pub fn validate(provider: &Provider) -> Result<()> {
    if provider.name.trim().is_empty() {
        return Err(Error::msg("Dai un nome al provider."));
    }
    // Si guarda il nome del computer vero, non l'inizio del testo:
    // `http://localhost.altro-sito.it` non è questo computer.
    let parsed = reqwest::Url::parse(provider.base()).ok();
    let scheme = parsed.as_ref().map(|u| u.scheme());
    let is_local = scheme == Some("http")
        && parsed
            .as_ref()
            .is_some_and(|u| matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")));
    if scheme != Some("https") && !is_local {
        return Err(Error::msg(
            "L'indirizzo deve iniziare con https:// (http:// è ammesso solo per questo computer): \
             altrimenti la chiave API viaggerebbe in chiaro.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(base_url: &str) -> Provider {
        Provider {
            id: "p".into(),
            name: "Prova".into(),
            kind: ProviderKind::OpenaiCompatible,
            base_url: base_url.into(),
            models: Vec::new(),
        }
    }

    #[test]
    fn legge_l_elenco_modelli_in_formato_openai() {
        let models = parse_openai_models(&json!({
            "data": [
                {"id": "gpt-x", "object": "model"},
                {"id": "fornitore/modello", "name": "Modello Bello", "context_length": 128000,
                 "top_provider": {"max_completion_tokens": 8192}},
                {"senza": "id"}
            ]
        }));
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "gpt-x");
        assert_eq!(models[1].name, "Modello Bello");
        assert_eq!(models[1].context_length, Some(128000));
        assert_eq!(models[1].max_output_tokens, Some(8192));
        assert_eq!(models[0].input_price, None);
    }

    #[test]
    fn calcola_il_costo_dai_prezzi_del_servizio() {
        let models = parse_openai_models(&json!({
            "data": [{"id": "m", "pricing": {"prompt": "0.000003", "completion": "0.000015"}}]
        }));
        let model = &models[0];
        assert!((model.input_price.unwrap() - 3.0).abs() < 1e-9);
        assert!((model.output_price.unwrap() - 15.0).abs() < 1e-9);
        // 2000 token letti a 3 $/milione + 1000 generati a 15 $/milione = 0,021 $
        assert!((model.cost(Some(2000), Some(1000)).unwrap() - 0.021).abs() < 1e-9);
        // Senza conteggio dei token non si inventa un costo.
        assert_eq!(model.cost(None, Some(1000)), None);
        // 1000 letti dalla cache (0,1x) e 1000 scritti (1,25x): 0,00405 $ + 0,015 $.
        let stats = crate::chat::GenerationStats {
            prompt_tokens: Some(2000),
            completion_tokens: Some(1000),
            cache_read_tokens: Some(1000),
            cache_write_tokens: Some(1000),
            ..Default::default()
        };
        assert!((model.cost_with_cache(&stats).unwrap() - 0.01905).abs() < 1e-9);
    }

    #[test]
    fn rifiuta_gli_indirizzi_non_cifrati() {
        assert!(validate(&provider("https://api.example.com/v1")).is_ok());
        assert!(validate(&provider("http://127.0.0.1:8080/v1")).is_ok());
        assert!(validate(&provider("http://localhost:1234/v1")).is_ok());
        assert!(validate(&provider("http://api.example.com/v1")).is_err());
        // Il nome del computer conta per intero, non solo l'inizio dell'indirizzo.
        assert!(validate(&provider("http://localhost.example.com/v1")).is_err());
        assert!(validate(&provider("http://127.0.0.1.nip.io/v1")).is_err());
        assert!(validate(&provider("http://localhost@example.com/v1")).is_err());
        assert!(validate(&provider("http://[::1]:8080/v1")).is_ok());
        assert!(validate(&provider("ftp://api.example.com/v1")).is_err());
        assert!(validate(&provider("api.example.com")).is_err());
    }

    #[test]
    fn la_barra_finale_non_conta() {
        assert_eq!(
            provider("https://api.example.com/v1/").base(),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn gli_identificatori_sono_leggibili() {
        let id = new_id("Open Router!");
        assert!(id.starts_with("open-router-"));
        assert!(new_id("???").starts_with("provider-"));
    }
}
