//! Nucleo dell'app.
//!
//! Contiene tutta la logica, senza interfaccia grafica: l'app desktop e
//! l'estensione VS Code sono solo client di questa libreria
//! (vedi `docs/architettura.md`).

pub mod agents;
pub mod attachments;
pub mod backup;
pub mod bundle;
pub mod chat;
pub mod error;
mod export;
pub mod gguf;
pub mod hub;
pub mod instance;
pub mod local;
pub mod log;
pub mod mcp;
pub mod mcp_oauth;
pub mod models;
pub mod named_presets;
pub mod plugins;
pub mod providers;
pub mod rag;
pub mod schedules;
pub mod secrets;
pub mod skills;
pub mod snapshots;
pub mod store;
pub mod tools;

/// I metodi di [`Core`], divisi per argomento.
mod ops;
pub use ops::engine::model_file_name;
pub use ops::plugins::PluginCommandInfo;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use providers::Provider;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, oneshot};

use agents::{
    Agent, AgentSession, LiveSession, LiveState, PendingRequest, RunControl, SessionStatus,
};
use attachments::{Attachment, AttachmentKind};
use chat::{ChatEvent, ChatMessage, GenerationStats};
use error::{Error, Result};
use hub::{DownloadEnd, HubFile, HubRepo};
use local::install::{self, Variant};
use local::pool::Engine;
use local::runtime::{InstalledEngine, LoadedModel, LocalRuntime};
use local::sysmon::{self, MemorySnapshot, ProcessGpuMemory};
use local::{LoadParams, MemoryEstimate, Placement, Preset, SamplingParams};
use models::ModelInfo;
use named_presets::NamedPreset;
use store::{ActionRecord, Conversation, Message, NewAction, Store, ToolData};
use tools::{
    Decision, PermissionLevel, Risk, ToolCall, ToolContext, ToolDef, ToolOutput, ToolSettings,
};

const SETTING_MODEL_FOLDERS: &str = "model_folders";
const SETTING_PROVIDERS: &str = "providers";
const SETTING_GRANTED_FOLDERS: &str = "granted_folders";
const SETTING_TOOL_DEFAULTS: &str = "tool_defaults";
const SETTING_MCP_SERVERS: &str = "mcp_servers";
/// Dati non segreti degli accessi OAuth ai server MCP (ADR 0024).
const SETTING_MCP_OAUTH: &str = "mcp_oauth";
const SETTING_ACTION_LOG_DAYS: &str = "action_log_days";
const SETTING_SECURITY: &str = "security";
const SETTING_SPEND_LIMITS: &str = "spend_limits";
/// La coda dei modelli cloud da provare quando quello scelto non risponde.
const SETTING_FALLBACK_MODELS: &str = "fallback_models";
/// Modelli al massimo nella coda di riserva.
const MAX_FALLBACK_MODELS: usize = 10;
const SETTING_RAG: &str = "rag";
/// I nomi delle skill spente (ADR 0023).
const SETTING_SKILLS_DISABLED: &str = "skills_disabled";
/// I plugin installati e ciò che hanno creato (ADR 0026).
const SETTING_PLUGINS: &str = "plugins";
/// Il modello di embedding si chiude dopo questo tempo senza ricerche.
const EMBEDDER_IDLE: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const SETTING_LANGUAGE: &str = "language";
const SETTING_PREFERENCES: &str = "preferences";
const SETTING_ENGINE_VARIANT: &str = "engine_variant";
/// I server MCP di una conversazione si chiudono dopo questo tempo senza messaggi (ADR 0022).
const CHAT_MCP_IDLE: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// Quante conversazioni possono tenere accesi i loro server MCP insieme.
const CHAT_MCP_MAX: usize = 4;
/// Ogni quanto si controlla se il modello locale è inattivo da scaricare.
const MAINTENANCE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// Lingue dell'interfaccia.
pub const LANGUAGES: &[&str] = &["it", "en"];
/// Per quanti giorni si tengono le voci del registro delle azioni, se l'utente
/// non sceglie altrimenti. `0` vuol dire "per sempre".
pub const DEFAULT_ACTION_LOG_DAYS: u32 = 90;
/// Ogni quanto, al massimo, si ripulisce il registro mentre il nucleo è acceso.
const ACTION_PRUNE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);
/// Quante volte di seguito un modello può chiedere strumenti in una risposta.
const MAX_TOOL_ROUNDS: usize = 25;
const NOTICE_TOO_MANY_TOOLS: &str = "Il modello ha usato strumenti troppe volte di seguito: mi sono fermato. Scrivi per farlo continuare.";
/// Inizio della chiave che identifica il preset di un modello cloud.
const CLOUD_KEY_PREFIX: &str = "cloud:";
const DEFAULT_TITLE: &str = "Nuova conversazione";
const TITLE_MAX_CHARS: usize = 48;

/// Versione del nucleo.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `true` se la lingua del sistema è l'italiano: serve finché l'utente non
/// ne sceglie una.
pub fn system_language_is_italian() -> bool {
    sys_locale::get_locale().is_some_and(|tag| tag.to_lowercase().starts_with("it"))
}

/// Esito del controllo "questo modello entra in memoria?".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadCheck {
    pub estimate: MemoryEstimate,
    /// Memoria libera nella collocazione scelta; `None` se non misurabile.
    /// Con la collocazione ibrida è la VRAM libera.
    pub available_bytes: Option<u64>,
    /// La RAM disponibile, quando la collocazione ne usa anche un po': con
    /// quella ibrida, o con "Solo VRAM" per gli embedding d'ingresso.
    pub ram_available_bytes: Option<u64>,
    pub fits: bool,
}

impl LoadCheck {
    /// Confronta la stima con la memoria libera, nella collocazione scelta.
    fn new(estimate: MemoryEstimate, placement: Placement, memory: &MemorySnapshot) -> Self {
        let vram_free = memory.gpu.as_ref().map(|gpu| gpu.free_bytes);
        let ram_free = Some(memory.ram_available_bytes);
        // Senza una misura non possiamo garantire nulla: non blocchiamo.
        let fits_in = |need: u64, free: Option<u64>| free.is_none_or(|free| need <= free);
        match placement {
            // In RAM restano solo gli embedding d'ingresso (`ram_bytes`).
            Placement::Vram => Self {
                estimate,
                available_bytes: vram_free,
                ram_available_bytes: ram_free.filter(|_| estimate.ram_bytes > 0),
                fits: fits_in(estimate.vram_bytes, vram_free)
                    && fits_in(estimate.ram_bytes, ram_free),
            },
            Placement::Ram => Self {
                estimate,
                available_bytes: ram_free,
                ram_available_bytes: None,
                fits: fits_in(estimate.total_bytes, ram_free),
            },
            Placement::Hybrid => Self {
                estimate,
                available_bytes: vram_free,
                ram_available_bytes: ram_free,
                fits: fits_in(estimate.vram_bytes, vram_free)
                    && fits_in(estimate.ram_bytes, ram_free),
            },
        }
    }
}

/// Stato dei modelli locali e della memoria, per gli indicatori dell'interfaccia.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    /// `true` mentre un modello è in fase di caricamento (o scaricamento).
    pub loading: bool,
    /// Il modello principale (ADR 0029).
    pub loaded: Option<LoadedModel>,
    pub memory: MemorySnapshot,
    /// RAM occupata dai processi dei modelli, sommata.
    pub process_ram_bytes: Option<u64>,
    /// Memoria grafica dedicata e condivisa dei processi dei modelli, sommata.
    pub process_gpu: Option<ProcessGpuMemory>,
    /// Presente se un modello caricato in VRAM sta sconfinando in RAM:
    /// quanta RAM il driver sta usando al posto della VRAM (somma di tutti).
    pub vram_spill_bytes: Option<u64>,
    /// Ogni modello caricato, in ordine di caricamento.
    pub models: Vec<LocalModelStatus>,
    /// "Più modelli locali insieme" è acceso.
    pub multi_model: bool,
    /// Con più modelli insieme: VRAM e RAM ancora disponibili per un altro
    /// modello, tolto il margine per il sistema.
    pub room_vram_bytes: Option<u64>,
    pub room_ram_bytes: Option<u64>,
}

/// Un modello locale caricato, con la sua memoria.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelStatus {
    #[serde(flatten)]
    pub model: LoadedModel,
    /// È il principale: quello usato quando non se ne sceglie uno.
    pub primary: bool,
    pub process_ram_bytes: Option<u64>,
    pub process_gpu: Option<ProcessGpuMemory>,
    pub vram_spill_bytes: Option<u64>,
}

/// Come caricare un modello locale.
#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    /// Carica anche se la stima dice che non entra.
    pub force: bool,
    /// Scarica prima gli altri modelli, anche con "Più modelli insieme".
    pub replace_others: bool,
    /// Diventa il principale: sì quando lo carica l'utente, no quando lo
    /// carica un agente o una pianificazione.
    pub make_primary: bool,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            force: false,
            replace_others: false,
            make_primary: true,
        }
    }
}

/// Esito dell'invio di un messaggio.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SendOutcome {
    pub conversation: Conversation,
    /// Assente se il modello non ha prodotto testo.
    pub assistant_message: Option<Message>,
    pub stats: GenerationStats,
    pub cancelled: bool,
    /// Avviso da mostrare sotto la risposta (troncata, rifiutata…).
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadPhase {
    Downloading,
    Done,
    Failed,
    Cancelled,
}

/// Avanzamento del download di un modello. Più download possono procedere
/// insieme: ognuno ha il suo identificatore (`<repo>/<file>`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadState {
    pub id: String,
    pub repo: String,
    pub file: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub phase: DownloadPhase,
    pub error: Option<String>,
    /// Hugging Face non ha pubblicato l'impronta del file: è stata controllata
    /// solo la dimensione.
    pub unverified: bool,
}

/// Tema dell'interfaccia.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Chiaro o scuro come il sistema.
    #[default]
    System,
    Light,
    Dark,
}

/// Preferenze generali dell'app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub theme: Theme,
    /// Chiudendo la finestra il nucleo resta attivo nell'area di notifica:
    /// modello caricato, agenti al lavoro e API locale per VS Code.
    pub keep_running: bool,
    /// Dopo quanti minuti senza richieste il modello locale si scarica da
    /// solo e libera la memoria (LO-8); `0` = mai.
    pub idle_unload_minutes: u32,
    /// All'avvio l'app chiede a GitHub se c'è una versione nuova. È l'unica
    /// richiesta che fa da sola; si può spegnere.
    pub check_updates: bool,
    /// L'utente ha concluso (o saltato) il primo avvio guidato.
    pub onboarded: bool,
    /// Più modelli locali caricati insieme, finché entrano in memoria (ADR
    /// 0029). Spento, caricare un modello sostituisce quello di prima.
    pub multi_model: bool,
    /// Un tetto in più al numero di modelli locali insieme; `0` = nessuno,
    /// decide la memoria.
    pub max_local_models: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            keep_running: true,
            idle_unload_minutes: 0,
            check_updates: true,
            onboarded: false,
            multi_model: false,
            max_local_models: 0,
        }
    }
}

/// Limiti di spesa per i modelli cloud, in dollari: `None` = nessun limite.
/// Contano i costi stimati delle risposte (CL-6), quindi solo i modelli di
/// cui si conosce il prezzo.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpendLimits {
    /// Nel mese in corso (dal primo del mese, ora UTC), in tutte le
    /// conversazioni e negli agenti.
    pub monthly_usd: Option<f64>,
    /// In una singola conversazione o sessione di un agente, rami compresi.
    pub conversation_usd: Option<f64>,
}

/// Quanto si è speso e con quali limiti.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spending {
    pub month_usd: f64,
    pub limits: SpendLimits,
}

/// Protezioni che rendono l'app più sicura ma chiedono qualche conferma in
/// più: accese di base, l'utente può spegnerle nelle Impostazioni.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SecuritySettings {
    /// Conferma prima che il modello apra una pagina con un indirizzo inventato
    /// mentre vede i file (vedi [`tools::needs_exfiltration_check`]).
    pub exfiltration_guard: bool,
    /// Conferma prima di aprire nel browser un collegamento di una risposta.
    pub confirm_links: bool,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            exfiltration_guard: true,
            confirm_links: true,
        }
    }
}

/// Il modello a cui inviare un messaggio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ModelRef {
    /// Un modello locale: quello del file indicato oppure, senza percorso, il
    /// principale (ADR 0029). `{"kind": "local"}` resta valido com'era.
    Local {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        path: Option<String>,
    },
    /// Un modello di un provider cloud.
    #[serde(rename_all = "camelCase")]
    Cloud { provider_id: String, model: String },
}

/// Un provider con le informazioni che servono all'interfaccia.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    #[serde(flatten)]
    pub provider: Provider,
    /// La chiave API è nel portachiavi. La chiave stessa non esce mai dal nucleo.
    pub has_key: bool,
    pub supported_params: &'static [&'static str],
}

/// Punto d'ingresso unico del nucleo.
pub struct Core {
    data_dir: PathBuf,
    /// Per parlare con `llama-server` su questo computer: mai tramite proxy.
    http: reqwest::Client,
    /// Per i servizi su internet: rispetta il proxy di sistema.
    cloud_http: reqwest::Client,
    store: Mutex<Store>,
    /// I modelli locali caricati, uno per `llama-server` (ADR 0029).
    engines: Mutex<Vec<Arc<Engine>>>,
    /// La chiave del modello principale scelto; `None` = il più vecchio.
    primary: Mutex<Option<String>>,
    /// Numera i motori in ordine di caricamento.
    engine_seq: std::sync::atomic::AtomicU64,
    /// Comandi delle risposte in corso in chat, per conversazione: l'app e
    /// l'estensione VS Code possono generare nello stesso momento.
    generation: Mutex<HashMap<i64, Arc<RunControl>>>,
    /// Sessioni degli agenti al lavoro, per identificatore.
    sessions: Mutex<HashMap<i64, Arc<LiveSession>>>,
    /// Occupato mentre llama.cpp viene scaricato e installato.
    installing: tokio::sync::Mutex<()>,
    /// I download dei modelli, in corso e conclusi, letti dall'interfaccia.
    downloads: Mutex<Vec<DownloadState>>,
    /// Segnali per interrompere i download in corso, per identificatore.
    download_cancel: Mutex<HashMap<String, Arc<Notify>>>,
    /// Per scaricare pagine web su richiesta dei modelli.
    web_http: reqwest::Client,
    /// Strumenti in attesa della conferma dell'utente. La chiave è
    /// `<ambito>:<identificatore della chiamata>` (`c12:…` in chat, `s3:…` in
    /// una sessione): due cicli in parallelo non si confondono.
    approvals: Mutex<HashMap<String, oneshot::Sender<Approval>>>,
    /// Ultima pulizia del registro delle azioni.
    last_action_prune: Mutex<std::time::Instant>,
    /// Dove stanno i valori delle variabili d'ambiente dei server MCP.
    env_vault: Arc<dyn mcp::EnvVault + Send + Sync>,
    /// Il secondo `llama-server`, quello del modello di embedding (RAG).
    embedder: tokio::sync::Mutex<LocalRuntime>,
    /// Ultimo uso del modello di embedding: si chiude dopo un po' di riposo.
    last_embed_use: Mutex<std::time::Instant>,
    /// Avanzamento dell'indicizzazione dei documenti.
    rag_progress: Mutex<rag::RagStatus>,
    /// Il modello di embedding che non dichiara il "pooling" e va avviato con
    /// `--pooling mean` (un modello di chat usato per gli embedding).
    mean_pooling_model: Mutex<Option<String>>,
    /// Copia in memoria dei vettori dell'indice, per non rileggerli a ogni
    /// ricerca. Si svuota quando l'indice cambia e quando il modello di
    /// embedding si chiude per riposo.
    /// Il numero cresce a ogni modifica: una copia costruita mentre l'indice
    /// cambiava non si conserva.
    rag_vectors: Mutex<(u64, Option<Arc<rag::VectorIndex>>)>,
    /// I server MCP accesi per le conversazioni in chat, per identificatore
    /// della conversazione (ADR 0022).
    chat_mcp: Mutex<HashMap<i64, ChatMcp>>,
    /// Gli accessi OAuth aperti nel browser, per identificatore del server.
    oauth_logins: Mutex<HashMap<String, ops::mcp_oauth::LoginSlot>>,
    /// Le pianificazioni in coda o al lavoro (ADR 0028).
    schedules_running: Mutex<HashSet<i64>>,
    /// I turni delle esecuzioni pianificate, uno per modello: quelle sullo
    /// stesso modello vanno una alla volta, nell'ordine di arrivo; quelle su
    /// modelli diversi lavorano insieme (ADR 0029).
    schedule_turns: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Un caricamento "accanto agli altri" alla volta: due insieme
    /// misurerebbero la stessa memoria libera.
    loading_alongside: tokio::sync::Mutex<()>,
    /// Esiti delle esecuzioni pianificate, per le notifiche dell'app.
    schedule_events: tokio::sync::broadcast::Sender<schedules::ScheduleEvent>,
}

/// I server MCP di una conversazione, accesi tra un messaggio e l'altro.
struct ChatMcp {
    /// Impronta della configurazione con cui sono partiti: se cambia si riavviano.
    key: u64,
    set: Arc<mcp::McpSet>,
    last_used: std::time::Instant,
}

/// Dove gira un ciclo modello → strumenti: una risposta in chat o una sessione.
struct RunScope<'a> {
    conversation_id: i64,
    session_id: Option<i64>,
    /// Nome della conversazione o dell'agente, per il registro delle azioni.
    source: String,
    /// Inizio delle chiavi delle conferme di questo ciclo.
    key_prefix: String,
    control: &'a RunControl,
    max_rounds: usize,
    /// Avviso quando i giri finiscono.
    limit_notice: String,
    /// I server MCP di questo ciclo: quelli dell'agente o della conversazione.
    mcp: Option<&'a mcp::McpSet>,
    /// Gli agenti a cui si può affidare lavoro (solo nelle sessioni degli agenti).
    delegation: Option<&'a Delegation>,
    /// Solo nelle sessioni degli agenti: i token massimi di un passo. Un passo
    /// troncato non ferma la sessione: il modello viene invitato a riprovare
    /// in breve (al massimo [`MAX_TRUNCATED_RETRIES`] volte di seguito).
    step_tokens: Option<u32>,
    /// Quale coda di riserva provare se un modello cloud non risponde (ADR 0027).
    fallback: Fallback,
    /// Nessuno sorveglia il lavoro (sessioni pianificate, ADR 0028): ciò che
    /// chiederebbe conferma si rifiuta.
    unattended: bool,
}

/// La coda di riserva di una risposta.
enum Fallback {
    /// Nessuna: nel confronto tra modelli ogni colonna risponde con il suo.
    Off,
    /// Quella delle Impostazioni.
    Global,
    /// Quella propria della conversazione o dell'agente.
    Own(Vec<ModelRef>),
}

impl Fallback {
    /// `None` della conversazione o dell'agente vuol dire "quella delle Impostazioni".
    fn from_setting(own: &Option<Vec<ModelRef>>) -> Self {
        match own {
            Some(models) => Fallback::Own(models.clone()),
            None => Fallback::Global,
        }
    }
}

/// Controlla una coda di riserva: solo modelli cloud, senza doppioni (l'ordine
/// resta), al massimo [`MAX_FALLBACK_MODELS`].
fn normalize_fallbacks(models: &[ModelRef]) -> Result<Vec<ModelRef>> {
    let mut queue: Vec<ModelRef> = Vec::new();
    for model in models {
        let ModelRef::Cloud {
            provider_id,
            model: id,
        } = model
        else {
            return Err(Error::msg("La coda di riserva accetta solo modelli cloud."));
        };
        if provider_id.trim().is_empty() || id.trim().is_empty() {
            return Err(Error::msg("La coda di riserva accetta solo modelli cloud."));
        }
        if !queue.contains(model) {
            queue.push(model.clone());
        }
    }
    if queue.len() > MAX_FALLBACK_MODELS {
        return Err(Error::msg(format!(
            "La coda di riserva può avere al massimo {MAX_FALLBACK_MODELS} modelli."
        )));
    }
    Ok(queue)
}

/// Quante volte di seguito un agente può riprovare un passo troncato.
const MAX_TRUNCATED_RETRIES: u32 = 2;
/// Il messaggio che invita l'agente a riprovare un passo troncato. Non si
/// salva: vale solo per il passo successivo.
const NUDGE_TRUNCATED: &str = "Il tuo ultimo passo è stato interrotto: hai superato il \
    limite di token per passo e le chiamate agli strumenti a metà sono state scartate. \
    Riprova in modo più breve: usa percorsi relativi alla cartella di lavoro e, per i file \
    lunghi, scrivili in più parti (write_file per l'inizio, poi edit_file).";

/// Chi può ricevere lavoro da una sessione (AG-8).
struct Delegation {
    agents: Vec<Agent>,
}

/// Risposta dell'utente alla richiesta di usare uno strumento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum Approval {
    Deny,
    /// Solo questa volta.
    Once,
    /// Anche le prossime volte, in questa conversazione.
    Always,
}

/// A chi va una richiesta.
enum Target {
    Local(LoadedModel),
    /// Provider, chiave API e identificatore del modello.
    Cloud(Provider, String, String),
}

impl Target {
    /// I modelli cloud ricevono le immagini (rifiuta il servizio, se il
    /// modello non le legge); quelli locali solo con il componente visivo.
    fn reads_images(&self) -> bool {
        match self {
            Target::Local(loaded) => loaded.vision,
            Target::Cloud(..) => true,
        }
    }

    /// Chiave del preset del modello (vedi [`Core::preset`]).
    fn preset_key(&self) -> String {
        match self {
            Target::Local(loaded) => loaded.model_path.clone(),
            Target::Cloud(provider, _, model) => {
                format!("{CLOUD_KEY_PREFIX}{}:{model}", provider.id)
            }
        }
    }
}

/// Parametri di generazione indicati da un client esterno: quelli assenti
/// si prendono dal preset del modello.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SamplingOverrides {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub seed: Option<i64>,
    pub stop: Option<Vec<String>>,
}

impl SamplingOverrides {
    pub fn apply(&self, base: &mut SamplingParams) {
        if let Some(v) = self.temperature {
            base.temperature = v;
        }
        if let Some(v) = self.top_p {
            base.top_p = v;
        }
        if let Some(v) = self.max_tokens {
            base.max_tokens = Some(v);
        }
        if let Some(v) = self.seed {
            base.seed = Some(v);
        }
        if let Some(v) = &self.stop {
            base.stop = v.clone();
        }
    }
}

/// Una variante di llama.cpp che l'app sa installare.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantInfo {
    pub variant: Variant,
    pub download_bytes: u64,
}

/// Stato dell'installazione di llama.cpp.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    /// Cartella della versione che verrà usata per il prossimo caricamento.
    pub installed: Option<String>,
    /// Tutte le versioni installate.
    pub engines: Vec<InstalledEngine>,
    /// Variante scelta dall'utente; `None` finché non ne installa una.
    pub selected: Option<Variant>,
    /// Build che l'app installerebbe.
    pub build: &'static str,
    /// Variante adatta a questo PC.
    pub recommended: Variant,
    pub download_bytes: u64,
    pub variants: Vec<VariantInfo>,
}

impl Core {
    /// Apre il nucleo usando `data_dir` per archivio, preset, binari e registri.
    pub fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let http = reqwest::Client::builder().no_proxy().build()?;
        // Un ripristino scelto dall'utente si applica qui, ad archivio chiuso.
        if let Err(e) = backup::apply_pending(data_dir) {
            log::error(format!("ripristino del backup non riuscito: {e}"));
        }
        let store = Store::open(&data_dir.join("data.sqlite"))?;
        // Nessuna sessione lavora più dopo un riavvio.
        store.mark_sessions_interrupted()?;
        // Un confronto non salvato prima di chiudere l'app non si recupera.
        store.delete_temporary_conversations()?;
        // Il registro non deve crescere senza limite: si ripulisce a ogni avvio
        // (e poi di tanto in tanto, vedi `log_action`).
        let _ = store.prune_actions_by_setting();
        log::info(format!(
            "archivio allo schema {} in {}",
            store.schema_version().unwrap_or(0),
            data_dir.display()
        ));
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            store: Mutex::new(store),
            engines: Mutex::new(Vec::new()),
            primary: Mutex::new(None),
            engine_seq: std::sync::atomic::AtomicU64::new(0),
            http,
            cloud_http: cloud_client()?,
            generation: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            installing: tokio::sync::Mutex::new(()),
            downloads: Mutex::new(Vec::new()),
            download_cancel: Mutex::new(HashMap::new()),
            web_http: tools::web::client()?,
            approvals: Mutex::new(HashMap::new()),
            last_action_prune: Mutex::new(std::time::Instant::now()),
            env_vault: Arc::new(secrets::KeyringVault),
            embedder: tokio::sync::Mutex::new(LocalRuntime::named(
                data_dir.to_path_buf(),
                reqwest::Client::builder().no_proxy().build()?,
                "embedding-server.log",
            )),
            last_embed_use: Mutex::new(std::time::Instant::now()),
            rag_progress: Mutex::new(rag::RagStatus::default()),
            mean_pooling_model: Mutex::new(None),
            rag_vectors: Mutex::new((0, None)),
            chat_mcp: Mutex::new(HashMap::new()),
            oauth_logins: Mutex::new(HashMap::new()),
            schedules_running: Mutex::new(HashSet::new()),
            schedule_turns: Mutex::new(HashMap::new()),
            loading_alongside: tokio::sync::Mutex::new(()),
            schedule_events: tokio::sync::broadcast::channel(32).0,
        })
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.store().schema_version()
    }
}

/// Client per i servizi su internet (provider, Hugging Face, GitHub).
///
/// Nessun tempo massimo per l'intera richiesta: una risposta in streaming o
/// il download di un modello possono durare ore. Si limita invece l'attesa
/// della connessione e quella tra un pezzo di dati e il successivo, così un
/// servizio che smette di rispondere non lascia la chat ferma per sempre.
/// Dieci minuti bastano anche ai modelli che "pensano" a lungo in silenzio.
fn cloud_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .read_timeout(std::time::Duration::from_secs(600))
        .build()
}

/// Unisce le parti di un prompt di sistema, saltando quelle vuote.
fn join_prompt(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(
            "

",
        )
}

/// Byte in gigabyte con una cifra decimale e la virgola, come si scrive in italiano.
fn gib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0 * 1024.0)).replace('.', ",")
}

#[cfg(test)]
mod tests;
