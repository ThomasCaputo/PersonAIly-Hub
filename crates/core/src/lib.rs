//! Nucleo dell'app.
//!
//! Contiene tutta la logica, senza interfaccia grafica: l'app desktop e
//! l'estensione VS Code sono solo client di questa libreria
//! (vedi `docs/architettura.md`).

pub mod agents;
pub mod attachments;
pub mod chat;
pub mod error;
pub mod gguf;
pub mod hub;
pub mod local;
pub mod mcp;
pub mod models;
pub mod named_presets;
pub mod providers;
pub mod secrets;
pub mod store;
pub mod tools;

use std::collections::HashMap;
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
use local::runtime::{LoadedModel, LocalRuntime};
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

/// Esito del controllo "questo modello entra in memoria?".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadCheck {
    pub estimate: MemoryEstimate,
    /// Memoria libera nella collocazione scelta; `None` se non misurabile.
    pub available_bytes: Option<u64>,
    pub fits: bool,
}

/// Stato dei modelli locali e della memoria, per gli indicatori dell'interfaccia.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    /// `true` mentre un modello è in fase di caricamento.
    pub loading: bool,
    pub loaded: Option<LoadedModel>,
    pub memory: MemorySnapshot,
    /// RAM occupata dal processo del modello.
    pub process_ram_bytes: Option<u64>,
    /// Memoria grafica dedicata e condivisa del processo del modello.
    pub process_gpu: Option<ProcessGpuMemory>,
    /// Presente se un modello caricato in VRAM sta sconfinando in RAM:
    /// quanta RAM il driver sta usando al posto della VRAM.
    pub vram_spill_bytes: Option<u64>,
}

/// Esito dell'invio di un messaggio.
#[derive(Debug, Clone, Serialize)]
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

/// Avanzamento del download di un modello.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadState {
    pub repo: String,
    pub file: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub phase: DownloadPhase,
    pub error: Option<String>,
}

/// Il modello a cui inviare un messaggio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ModelRef {
    /// Il modello locale attualmente caricato.
    Local,
    /// Un modello di un provider cloud.
    #[serde(rename_all = "camelCase")]
    Cloud { provider_id: String, model: String },
}

/// Un provider con le informazioni che servono all'interfaccia.
#[derive(Debug, Clone, Serialize)]
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
    runtime: tokio::sync::Mutex<LocalRuntime>,
    /// Comandi della risposta in corso in chat, per interromperla.
    generation: Mutex<Option<Arc<RunControl>>>,
    /// Sessioni degli agenti al lavoro, per identificatore.
    sessions: Mutex<HashMap<i64, Arc<LiveSession>>>,
    /// Occupato mentre llama.cpp viene scaricato e installato.
    installing: tokio::sync::Mutex<()>,
    /// Stato dell'ultimo download di un modello, letto dall'interfaccia.
    download: Mutex<Option<DownloadState>>,
    /// Segnale per interrompere il download in corso.
    download_cancel: Mutex<Option<Arc<Notify>>>,
    /// Per scaricare pagine web su richiesta dei modelli.
    web_http: reqwest::Client,
    /// Strumenti in attesa della conferma dell'utente. La chiave è
    /// `<ambito>:<identificatore della chiamata>` (`c12:…` in chat, `s3:…` in
    /// una sessione): due cicli in parallelo non si confondono.
    approvals: Mutex<HashMap<String, oneshot::Sender<Approval>>>,
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
    /// I server MCP avviati per questo ciclo (solo nelle sessioni degli agenti).
    mcp: Option<&'a mcp::McpSet>,
}

/// Descrizione breve di una chiamata, per la conferma e il registro.
fn summary_of(run: &RunScope<'_>, call: &ToolCall) -> String {
    run.mcp
        .and_then(|set| set.summary(&call.name))
        .unwrap_or_else(|| tools::summary(call))
}

/// Esito di una chiamata a uno strumento.
struct CallResult {
    /// `None` se l'utente ha interrotto nel frattempo.
    output: Option<ToolOutput>,
    /// Come è stata decisa, per il registro: `auto`, `user`, `denied`, `refused`.
    approval: &'static str,
}

/// Risposta dell'utente alla richiesta di usare uno strumento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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

/// Stato dell'installazione di llama.cpp.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    /// Cartella della versione installata, se c'è.
    pub installed: Option<String>,
    /// Build che l'app installerebbe.
    pub build: &'static str,
    pub recommended: Variant,
    pub download_bytes: u64,
}

impl Core {
    /// Apre il nucleo usando `data_dir` per archivio, preset, binari e registri.
    pub fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let http = reqwest::Client::builder().no_proxy().build()?;
        let store = Store::open(&data_dir.join("data.sqlite"))?;
        // Nessuna sessione lavora più dopo un riavvio.
        store.mark_sessions_interrupted()?;
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            store: Mutex::new(store),
            runtime: tokio::sync::Mutex::new(LocalRuntime::new(
                data_dir.to_path_buf(),
                http.clone(),
            )),
            http,
            cloud_http: reqwest::Client::new(),
            generation: Mutex::new(None),
            sessions: Mutex::new(HashMap::new()),
            installing: tokio::sync::Mutex::new(()),
            download: Mutex::new(None),
            download_cancel: Mutex::new(None),
            web_http: tools::web::client()?,
            approvals: Mutex::new(HashMap::new()),
        })
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.store().schema_version()
    }

    // ── Libreria modelli ────────────────────────────────────────────────

    pub fn model_folders(&self) -> Result<Vec<String>> {
        Ok(match self.store().setting(SETTING_MODEL_FOLDERS)? {
            Some(json) => serde_json::from_str(&json)?,
            None => Vec::new(),
        })
    }

    pub fn set_model_folders(&self, folders: &[String]) -> Result<()> {
        self.store()
            .set_setting(SETTING_MODEL_FOLDERS, &serde_json::to_string(folders)?)
    }

    pub fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let folders: Vec<PathBuf> = self.model_folders()?.iter().map(PathBuf::from).collect();
        Ok(models::scan(&folders))
    }

    // ── Preset ──────────────────────────────────────────────────────────

    /// File del preset. `model_key` è il percorso del file per un modello
    /// locale, oppure `cloud:<provider>:<modello>` per uno cloud.
    fn preset_path(&self, model_key: &str) -> PathBuf {
        let file_name = if model_key.starts_with(CLOUD_KEY_PREFIX) {
            // Caratteri come `:` e `/` non sono ammessi nei nomi di file.
            model_key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect()
        } else {
            Path::new(model_key)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "modello".to_string())
        };
        self.data_dir
            .join("presets")
            .join(format!("{file_name}.json"))
    }

    /// Parametri salvati per un modello; quelli predefiniti se non ce ne sono.
    pub fn preset(&self, model_key: &str) -> Result<Preset> {
        match std::fs::read_to_string(self.preset_path(model_key)) {
            Ok(json) => Ok(serde_json::from_str(&json)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut preset = Preset::default();
                if model_key.starts_with(CLOUD_KEY_PREFIX) {
                    // Per i servizi cloud si parte dai valori neutri dell'API:
                    // alcuni modelli non ne accettano altri.
                    preset.sampling.temperature = 1.0;
                    preset.sampling.top_p = 1.0;
                }
                Ok(preset)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_preset(&self, model_path: &str, preset: &Preset) -> Result<()> {
        let path = self.preset_path(model_path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(preset)?)?;
        Ok(())
    }

    // ── Motore llama.cpp ────────────────────────────────────────────────

    /// Dice se llama.cpp è installato e quale variante conviene a questo PC.
    pub async fn engine_status(&self) -> EngineStatus {
        let installed = self
            .runtime
            .try_lock()
            .ok()
            .and_then(|runtime| runtime.find_server().ok())
            .and_then(|exe| Some(exe.parent()?.file_name()?.to_string_lossy().into_owned()));
        // Con una GPU NVIDIA si usa CUDA; altrimenti Vulkan, che copre AMD e Intel.
        let has_nvidia = tokio::task::spawn_blocking(|| sysmon::snapshot().gpu.is_some())
            .await
            .unwrap_or(false);
        let recommended = if has_nvidia {
            Variant::Cuda
        } else {
            Variant::Vulkan
        };
        EngineStatus {
            installed,
            build: local::LLAMA_CPP_BUILD,
            recommended,
            download_bytes: recommended.download_bytes(),
        }
    }

    /// Scarica e installa llama.cpp. Una sola installazione alla volta.
    pub async fn install_engine(
        &self,
        variant: Variant,
        on_progress: impl FnMut(install::Progress),
    ) -> Result<()> {
        let _guard = self
            .installing
            .try_lock()
            .map_err(|_| Error::msg("Un'installazione è già in corso."))?;
        let bin_dir = self.data_dir.join("bin").join("llama.cpp");
        std::fs::create_dir_all(&bin_dir)?;
        install::install(&self.cloud_http, &bin_dir, variant, on_progress).await?;
        Ok(())
    }

    // ── Caricamento dei modelli locali ──────────────────────────────────

    /// Confronta la memoria necessaria con quella libera nella collocazione scelta.
    pub fn check_load(&self, model_path: &str, params: &LoadParams) -> LoadCheck {
        let model = models::inspect(Path::new(model_path));
        let estimate = local::estimate_memory(&model, params);
        let memory = sysmon::snapshot();
        let available_bytes = match params.placement {
            Placement::Vram => memory.gpu.map(|gpu| gpu.free_bytes),
            Placement::Ram => Some(memory.ram_available_bytes),
        };
        LoadCheck {
            estimate,
            available_bytes,
            // Senza una misura non possiamo garantire nulla: non blocchiamo.
            fits: available_bytes.is_none_or(|free| estimate.total_bytes <= free),
        }
    }

    /// Carica un modello. Se la stima dice che non entra, rifiuta, a meno che
    /// `force` non sia `true`.
    pub async fn load_model(
        &self,
        model_path: &str,
        params: &LoadParams,
        force: bool,
    ) -> Result<LoadedModel> {
        let mut runtime = self.runtime.lock().await;
        // Prima si libera la memoria del modello precedente, poi si misura.
        runtime.unload().await;

        let model = models::inspect(Path::new(model_path));
        if let Some(error) = &model.error {
            return Err(Error::msg(format!(
                "file del modello non leggibile: {error}"
            )));
        }

        if !force {
            let path = model_path.to_string();
            let params_for_check = params.clone();
            let check = self.check_load_blocking(path, params_for_check).await?;
            if !check.fits {
                let place = match params.placement {
                    Placement::Vram => "VRAM",
                    Placement::Ram => "RAM",
                };
                return Err(Error::msg(format!(
                    "Il modello non entra nella {place}: servono circa {} GB, liberi {} GB.",
                    gib(check.estimate.total_bytes),
                    gib(check.available_bytes.unwrap_or(0)),
                )));
            }
        }

        runtime.load(model_path, &model.name, params).await
    }

    async fn check_load_blocking(&self, path: String, params: LoadParams) -> Result<LoadCheck> {
        // La misura della VRAM avvia un processo esterno: fuori dal ciclo asincrono.
        let model = models::inspect(Path::new(&path));
        let estimate = local::estimate_memory(&model, &params);
        let memory = tokio::task::spawn_blocking(sysmon::snapshot)
            .await
            .map_err(|e| Error::msg(e.to_string()))?;
        let available_bytes = match params.placement {
            Placement::Vram => memory.gpu.map(|gpu| gpu.free_bytes),
            Placement::Ram => Some(memory.ram_available_bytes),
        };
        Ok(LoadCheck {
            estimate,
            available_bytes,
            fits: available_bytes.is_none_or(|free| estimate.total_bytes <= free),
        })
    }

    pub async fn unload_model(&self) {
        self.stop_generation();
        self.runtime.lock().await.unload().await;
    }

    pub async fn status(&self) -> RuntimeStatus {
        // Durante un caricamento il runtime è occupato: lo segnaliamo senza attendere.
        let (loading, loaded) = match self.runtime.try_lock() {
            Ok(mut runtime) => (false, runtime.loaded()),
            Err(_) => (true, None),
        };
        let pid = loaded.as_ref().map(|m| m.pid);
        let (memory, process_ram_bytes, process_gpu) = tokio::task::spawn_blocking(move || {
            (
                sysmon::snapshot(),
                pid.and_then(sysmon::process_ram),
                pid.and_then(sysmon::process_gpu_memory),
            )
        })
        .await
        .unwrap_or_default();

        // Solo in VRAM ha senso parlare di sconfinamento: in RAM è tutto lì per scelta.
        let in_vram = loaded
            .as_ref()
            .is_some_and(|m| m.params.placement == Placement::Vram);
        let vram_spill_bytes = process_gpu
            .filter(|_| in_vram)
            .and_then(|gpu| local::vram_spill(gpu.shared_bytes));

        RuntimeStatus {
            loading,
            loaded,
            memory,
            process_ram_bytes,
            process_gpu,
            vram_spill_bytes,
        }
    }

    // ── Preset con nome ─────────────────────────────────────────────────

    fn named_presets_dir(&self) -> PathBuf {
        self.data_dir.join("presets").join("named")
    }

    pub fn named_presets(&self) -> Vec<NamedPreset> {
        named_presets::list(&self.named_presets_dir())
    }

    pub fn save_named_preset(&self, preset: &NamedPreset) -> Result<NamedPreset> {
        named_presets::save(&self.named_presets_dir(), preset)
    }

    pub fn delete_named_preset(&self, name: &str) -> Result<()> {
        named_presets::delete(&self.named_presets_dir(), name)
    }

    pub fn export_named_preset(&self, name: &str, destination: &Path) -> Result<()> {
        named_presets::export(&self.named_presets_dir(), name, destination)
    }

    pub fn import_named_preset(&self, source: &Path) -> Result<NamedPreset> {
        named_presets::import(&self.named_presets_dir(), source)
    }

    // ── Download dei modelli da Hugging Face ────────────────────────────

    pub async fn search_hub(&self, query: &str) -> Result<Vec<HubRepo>> {
        hub::search(&self.cloud_http, query).await
    }

    pub async fn hub_files(&self, repo: &str) -> Result<Vec<HubFile>> {
        hub::files(&self.cloud_http, repo).await
    }

    /// Stato dell'ultimo download avviato, se ce n'è stato uno.
    pub fn download_status(&self) -> Option<DownloadState> {
        self.download
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn update_download(&self, change: impl FnOnce(&mut DownloadState)) {
        if let Some(state) = self
            .download
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            change(state);
        }
    }

    /// Scarica un modello in una delle cartelle della libreria. Termina a
    /// download concluso: chi la chiama la esegue in sottofondo e segue
    /// l'avanzamento con [`Core::download_status`].
    pub async fn download_model(&self, repo: &str, file: &HubFile, folder: &str) -> Result<()> {
        // Si scrive solo dove l'utente ha già detto di tenere i modelli.
        if !self.model_folders()?.iter().any(|f| f == folder) {
            return Err(Error::msg(
                "La cartella di destinazione non è tra quelle della libreria.",
            ));
        }

        let cancel = Arc::new(Notify::new());
        {
            let mut current = self.download.lock().unwrap_or_else(|e| e.into_inner());
            if current
                .as_ref()
                .is_some_and(|d| d.phase == DownloadPhase::Downloading)
            {
                return Err(Error::msg("C'è già un download in corso."));
            }
            *current = Some(DownloadState {
                repo: repo.to_string(),
                file: file.path.clone(),
                done_bytes: 0,
                total_bytes: file.size_bytes,
                phase: DownloadPhase::Downloading,
                error: None,
            });
            *self
                .download_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());
        }

        let result = hub::download(
            &self.cloud_http,
            repo,
            file,
            Path::new(folder),
            cancel,
            |done| self.update_download(|state| state.done_bytes = done),
        )
        .await;

        self.update_download(|state| match &result {
            Ok(DownloadEnd::Completed(_)) => state.phase = DownloadPhase::Done,
            Ok(DownloadEnd::Cancelled) => state.phase = DownloadPhase::Cancelled,
            Err(e) => {
                state.phase = DownloadPhase::Failed;
                state.error = Some(e.to_string());
            }
        });
        result.map(|_| ())
    }

    /// Interrompe il download in corso; il file parziale resta per riprendere.
    pub fn cancel_download(&self) {
        if let Some(cancel) = self
            .download_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            cancel.notify_one();
        }
    }

    // ── Conversazioni ───────────────────────────────────────────────────

    pub fn create_conversation(&self) -> Result<Conversation> {
        self.store().create_conversation(DEFAULT_TITLE)
    }

    /// Conversazioni dalla più recente; con `query` solo quelle che contengono
    /// il testo cercato nel titolo o in un messaggio.
    pub fn list_conversations(&self, query: &str) -> Result<Vec<Conversation>> {
        self.store().list_conversations(query)
    }

    pub fn rename_conversation(&self, id: i64, title: &str) -> Result<()> {
        self.store().rename_conversation(id, title)
    }

    pub fn delete_conversation(&self, id: i64) -> Result<()> {
        self.store().delete_conversation(id)
    }

    pub fn messages(&self, conversation_id: i64) -> Result<Vec<Message>> {
        self.store().messages(conversation_id)
    }

    // ── Strumenti e permessi ────────────────────────────────────────────

    /// Cartelle in cui i modelli possono leggere e scrivere file.
    pub fn granted_folders(&self) -> Result<Vec<String>> {
        Ok(match self.store().setting(SETTING_GRANTED_FOLDERS)? {
            Some(json) => serde_json::from_str(&json)?,
            None => Vec::new(),
        })
    }

    pub fn set_granted_folders(&self, folders: &[String]) -> Result<()> {
        self.store()
            .set_setting(SETTING_GRANTED_FOLDERS, &serde_json::to_string(folders)?)
    }

    /// Strumenti e permessi con cui nasce una nuova conversazione.
    pub fn tool_defaults(&self) -> Result<ToolSettings> {
        Ok(match self.store().setting(SETTING_TOOL_DEFAULTS)? {
            Some(json) => serde_json::from_str(&json).unwrap_or_default(),
            None => ToolSettings::default(),
        })
    }

    pub fn set_tool_defaults(&self, settings: &ToolSettings) -> Result<()> {
        self.store()
            .set_setting(SETTING_TOOL_DEFAULTS, &serde_json::to_string(settings)?)
    }

    /// Strumenti e permessi di una conversazione; quelli predefiniti finché
    /// l'utente non li cambia.
    pub fn conversation_tools(&self, conversation_id: i64) -> Result<ToolSettings> {
        let saved = self.store().conversation_tools(conversation_id)?;
        match saved.and_then(|json| serde_json::from_str(&json).ok()) {
            Some(settings) => Ok(settings),
            None => self.tool_defaults(),
        }
    }

    pub fn set_conversation_tools(
        &self,
        conversation_id: i64,
        settings: &ToolSettings,
    ) -> Result<()> {
        self.store()
            .set_conversation_tools(conversation_id, &serde_json::to_string(settings)?)
    }

    /// Risposta dell'utente a una richiesta di conferma ([`ChatEvent::ToolRequest`]).
    pub fn answer_tool_request(&self, id: &str, approval: Approval) {
        let waiting = self
            .approvals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if let Some(sender) = waiting {
            // Se nessuno aspetta più (risposta interrotta) non c'è nulla da fare.
            let _ = sender.send(approval);
        }
        // Una sessione che aspettava questa conferma torna al lavoro.
        for live in self.live_sessions() {
            let mut state = live.state();
            if state.request.as_ref().is_some_and(|r| r.id == id) {
                state.request = None;
                state.status = Some(SessionStatus::Running);
            }
        }
    }

    /// Esegue una chiamata a uno strumento, chiedendo prima conferma
    /// all'utente se il livello di permesso lo richiede.
    async fn run_call(
        &self,
        run: &RunScope<'_>,
        call: &ToolCall,
        settings: &mut ToolSettings,
        folders: &[PathBuf],
        on_event: &mut impl FnMut(ChatEvent),
    ) -> CallResult {
        // Uno strumento di un server MCP della sessione.
        let external = run.mcp.filter(|set| set.has(&call.name));
        let risk = if external.is_some() {
            if !call.arguments.is_object() {
                return CallResult {
                    output: Some(ToolOutput::error(
                        "I parametri della chiamata non sono un oggetto JSON valido: riprova.",
                    )),
                    approval: "refused",
                };
            }
            Risk::Execute
        } else {
            // Una chiamata che fallirebbe comunque non merita di disturbare l'utente.
            if let Some(refused) = tools::precheck(call, settings, folders) {
                return CallResult {
                    output: Some(refused),
                    approval: "refused",
                };
            }
            let Some(risk) = tools::risk_of(&call.name) else {
                return CallResult {
                    output: Some(ToolOutput::error("Strumento sconosciuto.")),
                    approval: "refused",
                };
            };
            risk
        };
        let cancel = &run.control.cancel;
        // "Consenti sempre" su uno strumento MCP vale solo per quello strumento.
        let allowed_always = external.is_some() && settings.allowed_tools.contains(&call.name);

        let mut approval = "auto";
        if !allowed_always && tools::decide(settings.permission, risk) == Decision::Ask {
            let key = format!("{}:{}", run.key_prefix, call.id);
            let (sender, receiver) = oneshot::channel();
            self.approvals
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key.clone(), sender);
            on_event(ChatEvent::ToolRequest {
                id: key,
                name: call.name.clone(),
                summary: summary_of(run, call),
                arguments: call.arguments.clone(),
            });
            let answer = tokio::select! {
                answer = receiver => answer.unwrap_or(Approval::Deny),
                _ = cancel.notified() => return CallResult { output: None, approval: "interrupted" },
            };
            match answer {
                Approval::Deny => {
                    return CallResult {
                        output: Some(ToolOutput::error(
                            "L'utente ha negato il permesso per questa azione. Non riprovare: \
                             chiedigli come vuole procedere.",
                        )),
                        approval: "denied",
                    };
                }
                Approval::Once => {}
                Approval::Always => {
                    // Vale per il resto della conversazione (o della sessione),
                    // anche dopo il riavvio.
                    if external.is_some() {
                        settings.allowed_tools.push(call.name.clone());
                    } else {
                        settings.permission = match risk {
                            Risk::Execute => PermissionLevel::All,
                            _ => settings.permission.max_with(PermissionLevel::Edits),
                        };
                    }
                    let _ = self.set_conversation_tools(run.conversation_id, settings);
                }
            }
            approval = "user";
        }

        let ctx = ToolContext {
            folders,
            web_http: &self.web_http,
        };
        let output = match external {
            Some(set) => tokio::select! {
                output = set.call(&call.name, &call.arguments) => Some(output),
                _ = cancel.notified() => None,
            },
            None => tokio::select! {
                output = tools::run(call, settings, &ctx) => Some(output),
                _ = cancel.notified() => None,
            },
        };
        CallResult { output, approval }
    }

    // ── Invio dei messaggi ──────────────────────────────────────────────

    /// A chi mandare le richieste per un modello; errore se non è raggiungibile.
    fn target(&self, model: &ModelRef) -> Result<Target> {
        Ok(match model {
            ModelRef::Local => Target::Local(
                self.runtime
                    .try_lock()
                    .ok()
                    .and_then(|mut runtime| runtime.loaded())
                    .ok_or_else(|| Error::msg("Nessun modello locale caricato."))?,
            ),
            ModelRef::Cloud {
                provider_id,
                model: id,
            } => {
                let provider = self.provider(provider_id)?;
                let key = secrets::api_key(provider_id)?.ok_or_else(|| {
                    Error::msg(format!(
                        "Manca la chiave API di {}: inseriscila nella pagina Modelli.",
                        provider.name
                    ))
                })?;
                Target::Cloud(provider, key, id.clone())
            }
        })
    }

    /// Salva il messaggio dell'utente, lo invia al modello insieme alla
    /// cronologia e salva la risposta. Se il modello chiede degli strumenti li
    /// esegue (con conferma, dove serve) e gli rimanda i risultati, finché non
    /// arriva la risposta finale. `on_event` segue tutto mentre accade.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_message(
        &self,
        conversation_id: i64,
        text: &str,
        attachment_paths: &[String],
        sampling: &SamplingParams,
        system_prompt: &str,
        model: &ModelRef,
        mut on_event: impl FnMut(ChatEvent),
    ) -> Result<SendOutcome> {
        // Prima si verifica di poter raggiungere il modello, poi si salva il messaggio.
        let target = self.target(model)?;

        let attachments = attachment_paths
            .iter()
            .map(|path| attachments::load(Path::new(path)))
            .collect::<Result<Vec<Attachment>>>()?;
        let reads_images = matches!(target, Target::Cloud(..));
        if !reads_images && attachments.iter().any(|a| a.kind == AttachmentKind::Image) {
            return Err(Error::msg(
                "Il modello locale caricato non legge le immagini: scegli un modello cloud \
                 che le supporti, oppure togli l'immagine.",
            ));
        }

        let mut settings = self.conversation_tools(conversation_id)?;
        // Il terminale è degli agenti: in chat non si offre.
        settings.terminal = false;
        let folders: Vec<PathBuf> = self
            .granted_folders()?
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let defs = tools::available(&settings, &folders);
        let system = join_prompt(&[system_prompt, &tools::system_preamble(&defs, &folders)]);

        let (chat, title) = {
            let store = self.store();
            let is_first = store.messages(conversation_id)?.is_empty();
            store.add_message_with(conversation_id, "user", text, None, None, &attachments)?;
            if is_first {
                store.rename_conversation(conversation_id, &title_from(text))?;
            }
            (
                chat_history(store.messages(conversation_id)?, reads_images),
                store.conversation(conversation_id)?.title,
            )
        };

        let control = Arc::new(RunControl::new());
        *self.generation.lock().unwrap_or_else(|e| e.into_inner()) = Some(control.clone());

        let scope = RunScope {
            conversation_id,
            session_id: None,
            source: title,
            key_prefix: format!("c{conversation_id}"),
            control: &control,
            max_rounds: MAX_TOOL_ROUNDS,
            limit_notice: NOTICE_TOO_MANY_TOOLS.to_string(),
            mcp: None,
        };
        let result = self
            .run_rounds(
                &scope,
                &target,
                chat,
                &defs,
                &mut settings,
                &folders,
                &system,
                sampling,
                &mut on_event,
            )
            .await;

        *self.generation.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.forget_approvals(&scope.key_prefix);
        result
    }

    /// Dimentica le conferme rimaste in sospeso di un ciclo concluso.
    fn forget_approvals(&self, key_prefix: &str) {
        let prefix = format!("{key_prefix}:");
        self.approvals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|key, _| !key.starts_with(&prefix));
    }

    /// Il ciclo modello → strumenti → modello di una risposta.
    #[allow(clippy::too_many_arguments)]
    async fn run_rounds(
        &self,
        run: &RunScope<'_>,
        target: &Target,
        mut chat: Vec<ChatMessage>,
        defs: &[ToolDef],
        settings: &mut ToolSettings,
        folders: &[PathBuf],
        system: &str,
        sampling: &SamplingParams,
        on_event: &mut impl FnMut(ChatEvent),
    ) -> Result<SendOutcome> {
        let conversation_id = run.conversation_id;
        let cancel = run.control.cancel.clone();
        let mut last_reply: Option<Message> = None;
        let mut stats = GenerationStats::default();
        let mut notice = None;
        let mut cancelled = false;

        for round in 1..=run.max_rounds {
            // Una pausa chiesta dall'utente ferma il lavoro qui, tra un passo e l'altro.
            if !run.control.checkpoint().await {
                cancelled = true;
                break;
            }
            let (result, model_name) = match target {
                Target::Local(loaded) => {
                    let mut all = Vec::with_capacity(chat.len() + 1);
                    if !system.is_empty() {
                        all.push(ChatMessage::new("system", system));
                    }
                    all.extend(chat.iter().cloned());
                    let url = format!("http://127.0.0.1:{}/v1/chat/completions", loaded.port);
                    let body = chat::request_body(&all, defs, sampling, None, chat::Flavor::Llama);
                    let result = chat::stream_chat(
                        &self.http,
                        &url,
                        None,
                        &body,
                        cancel.clone(),
                        &mut *on_event,
                    )
                    .await;
                    (result, loaded.model_name.clone())
                }
                Target::Cloud(provider, key, id) => {
                    let result = providers::stream(
                        &self.cloud_http,
                        provider,
                        key,
                        id,
                        &chat,
                        defs,
                        system,
                        sampling,
                        cancel.clone(),
                        &mut *on_event,
                    )
                    .await;
                    (result, id.clone())
                }
            };
            let mut outcome = result?;
            stats = outcome.stats.clone();
            cancelled = outcome.cancelled || run.control.is_stopped();
            // Una risposta troncata o rifiutata può contenere chiamate a metà:
            // i suoi strumenti non si eseguono.
            if outcome.notice.is_some() {
                notice = outcome.notice.take();
                outcome.tool_calls.clear();
            }
            let calls = std::mem::take(&mut outcome.tool_calls);

            if !outcome.text.is_empty() || !calls.is_empty() {
                // Il costo si stima solo per i modelli cloud di cui si conosce il prezzo.
                let cost_usd = match target {
                    Target::Cloud(provider, _, id) => provider
                        .model(id)
                        .and_then(|m| m.cost(stats.prompt_tokens, stats.completion_tokens)),
                    Target::Local(_) => None,
                };
                let tool = (!calls.is_empty()).then(|| ToolData {
                    calls: calls.clone(),
                    ..Default::default()
                });
                let store = self.store();
                let message = store.add_message_with(
                    conversation_id,
                    "assistant",
                    &outcome.text,
                    Some(&model_name),
                    tool.as_ref(),
                    &[],
                )?;
                last_reply = Some(store.set_message_usage(
                    message.id,
                    stats.prompt_tokens,
                    stats.completion_tokens,
                    cost_usd,
                )?);
            }
            if calls.is_empty() || cancelled {
                break;
            }
            if round == run.max_rounds {
                notice = Some(run.limit_notice.clone());
                // Le chiamate di quest'ultima risposta restano senza seguito:
                // ricevono comunque un risultato, per la coerenza della cronologia.
                for call in &calls {
                    self.save_tool_result(
                        conversation_id,
                        call,
                        &ToolOutput::error("Non eseguito: raggiunto il limite di passi."),
                    )?;
                }
                break;
            }

            on_event(ChatEvent::Saved);
            chat.push(ChatMessage {
                tool_calls: calls.clone(),
                native: outcome.native.take(),
                ..ChatMessage::new("assistant", outcome.text)
            });

            for call in &calls {
                // Dopo un'interruzione le chiamate rimaste ricevono comunque
                // un risultato: la cronologia deve restare coerente.
                if !cancelled && !run.control.checkpoint().await {
                    cancelled = true;
                }
                let output = if cancelled {
                    None
                } else {
                    let result = self
                        .run_call(run, call, settings, folders, &mut *on_event)
                        .await;
                    self.log_action(run, call, &result);
                    result.output
                };
                cancelled |= output.is_none();
                let output = output.unwrap_or_else(|| ToolOutput::error("Interrotto dall'utente."));
                self.save_tool_result(conversation_id, call, &output)?;
                chat.push(ChatMessage {
                    tool_call_id: Some(call.id.clone()),
                    is_error: output.is_error,
                    ..ChatMessage::new("tool", output.content)
                });
                on_event(ChatEvent::Saved);
            }
            if cancelled {
                break;
            }
        }

        Ok(SendOutcome {
            conversation: self.store().conversation(conversation_id)?,
            assistant_message: last_reply,
            stats,
            cancelled,
            notice,
        })
    }

    fn save_tool_result(
        &self,
        conversation_id: i64,
        call: &ToolCall,
        output: &ToolOutput,
    ) -> Result<()> {
        self.store().add_message_with(
            conversation_id,
            "tool",
            &output.content,
            None,
            Some(&ToolData {
                call_id: Some(call.id.clone()),
                name: Some(call.name.clone()),
                is_error: output.is_error,
                ..Default::default()
            }),
            &[],
        )?;
        Ok(())
    }

    /// Annota l'azione nel registro (SI-6). Un registro che non si scrive non
    /// deve fermare il lavoro.
    fn log_action(&self, run: &RunScope<'_>, call: &ToolCall, result: &CallResult) {
        let (approval, is_error, text) = match &result.output {
            Some(output) => (result.approval, output.is_error, output.content.as_str()),
            None => ("interrupted", true, "Interrotto dall'utente."),
        };
        let _ = self.store().add_action(&NewAction {
            conversation_id: run.conversation_id,
            session_id: run.session_id,
            source: &run.source,
            tool: &call.name,
            summary: &summary_of(run, call),
            approval,
            is_error,
            result: text,
        });
    }

    /// Le azioni compiute dagli strumenti, dalla più recente; con `session_id`
    /// solo quelle di una sessione.
    pub fn actions(&self, limit: u32, session_id: Option<i64>) -> Result<Vec<ActionRecord>> {
        self.store().actions(limit.clamp(1, 1000), session_id)
    }

    // ── Server MCP ──────────────────────────────────────────────────────

    /// I server MCP configurati, in ordine alfabetico.
    pub fn mcp_servers(&self) -> Result<Vec<mcp::McpServerConfig>> {
        let mut servers: Vec<mcp::McpServerConfig> =
            match self.store().setting(SETTING_MCP_SERVERS)? {
                Some(json) => serde_json::from_str(&json)?,
                None => Vec::new(),
            };
        servers.sort_by_key(|s| s.name.to_lowercase());
        Ok(servers)
    }

    fn store_mcp_servers(&self, servers: &[mcp::McpServerConfig]) -> Result<()> {
        self.store()
            .set_setting(SETTING_MCP_SERVERS, &serde_json::to_string(servers)?)
    }

    /// Crea (con `id` vuoto) o aggiorna un server MCP.
    pub fn save_mcp_server(
        &self,
        mut config: mcp::McpServerConfig,
    ) -> Result<mcp::McpServerConfig> {
        config.name = config.name.trim().to_string();
        config.command = config.command.trim().to_string();
        config.args.retain(|a| !a.trim().is_empty());
        config.validate()?;
        let mut all = self.mcp_servers()?;
        match all
            .iter_mut()
            .find(|s| s.id == config.id && !config.id.is_empty())
        {
            Some(existing) => *existing = config.clone(),
            None => {
                config.id = providers::new_id(&config.name);
                all.push(config.clone());
            }
        }
        self.store_mcp_servers(&all)?;
        Ok(config)
    }

    /// Elimina un server MCP. Gli agenti che lo usavano non lo trovano più
    /// e le loro sessioni lo segnalano.
    pub fn delete_mcp_server(&self, id: &str) -> Result<()> {
        let mut all = self.mcp_servers()?;
        all.retain(|s| s.id != id);
        self.store_mcp_servers(&all)
    }

    /// Avvia un server, ne elenca gli strumenti e lo chiude.
    pub async fn probe_mcp_server(
        &self,
        config: &mcp::McpServerConfig,
    ) -> Result<Vec<mcp::McpToolInfo>> {
        mcp::probe(config).await
    }

    fn mcp_servers_for(&self, ids: &[String]) -> Result<Vec<mcp::McpServerConfig>> {
        let all = self.mcp_servers()?;
        ids.iter()
            .map(|id| {
                all.iter().find(|s| &s.id == id).cloned().ok_or_else(|| {
                    Error::msg(
                        "Un server MCP di questo agente è stato eliminato: togli il segno                          dall'agente o configura di nuovo il server nelle Impostazioni.",
                    )
                })
            })
            .collect()
    }

    // ── Agenti ──────────────────────────────────────────────────────────

    pub fn agents(&self) -> Result<Vec<Agent>> {
        self.store().agents()
    }

    /// Crea (con `id` 0) o aggiorna un agente.
    pub fn save_agent(&self, agent: Agent) -> Result<Agent> {
        let agent = agent.normalized();
        if agent.name.is_empty() {
            return Err(Error::msg("Dai un nome all'agente."));
        }
        self.store().save_agent(&agent)
    }

    pub fn duplicate_agent(&self, id: i64) -> Result<Agent> {
        let store = self.store();
        let original = store.agent(id)?;
        store.save_agent(&Agent {
            id: 0,
            name: format!("{} (copia)", original.name),
            ..original
        })
    }

    pub fn delete_agent(&self, id: i64) -> Result<()> {
        self.store().delete_agent(id)
    }

    // ── Sessioni degli agenti ───────────────────────────────────────────

    fn live_sessions(&self) -> Vec<Arc<LiveSession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    fn live_session(&self, id: i64) -> Option<Arc<LiveSession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    fn with_live_status(&self, mut session: AgentSession) -> AgentSession {
        if let Some(status) = self
            .live_session(session.id)
            .and_then(|l| l.snapshot().status)
        {
            session.status = status;
        }
        session
    }

    /// Prepara una sessione: l'agente lavorerà sul compito appena chi la
    /// chiama avvia [`Core::run_session`] (di solito in sottofondo).
    pub fn create_session(&self, agent_id: i64, task: &str) -> Result<AgentSession> {
        let task = task.trim();
        if task.is_empty() {
            return Err(Error::msg("Scrivi il compito da affidare all'agente."));
        }
        let store = self.store();
        let agent = store.agent(agent_id)?;
        store.create_session(&agent, task)
    }

    /// Aggiunge un messaggio dell'utente a una sessione ferma, perché l'agente
    /// riprenda da lì con [`Core::run_session`].
    pub fn reply_session(&self, id: i64, text: &str) -> Result<AgentSession> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::msg("Scrivi qualcosa all'agente."));
        }
        if self.live_session(id).is_some() {
            return Err(Error::msg("L'agente sta già lavorando."));
        }
        let store = self.store();
        let session = store.session(id)?;
        store.add_message(session.conversation_id, "user", text, None)?;
        store.set_session_status(id, SessionStatus::Running, None)?;
        store.session(id)
    }

    /// Le sessioni, dalla più recente, con lo stato attuale di quelle al lavoro.
    pub fn sessions(&self) -> Result<Vec<AgentSession>> {
        let sessions = self.store().sessions()?;
        Ok(sessions
            .into_iter()
            .map(|s| self.with_live_status(s))
            .collect())
    }

    pub fn session(&self, id: i64) -> Result<AgentSession> {
        let session = self.store().session(id)?;
        Ok(self.with_live_status(session))
    }

    /// Ciò che una sessione al lavoro sta facendo; `None` se è ferma.
    pub fn session_live(&self, id: i64) -> Option<LiveState> {
        self.live_session(id).map(|live| live.snapshot())
    }

    /// Mette in pausa una sessione: si ferma prima del passo successivo.
    pub fn pause_session(&self, id: i64) -> Result<()> {
        let live = self
            .live_session(id)
            .ok_or_else(|| Error::msg("La sessione non sta lavorando."))?;
        live.control.pause();
        Ok(())
    }

    pub fn resume_session(&self, id: i64) -> Result<()> {
        let live = self
            .live_session(id)
            .ok_or_else(|| Error::msg("La sessione non sta lavorando."))?;
        live.control.resume();
        Ok(())
    }

    /// Ferma una sessione: la risposta in corso si interrompe e il comando in
    /// esecuzione viene terminato. Ciò che è già arrivato resta salvato.
    pub fn stop_session(&self, id: i64) {
        if let Some(live) = self.live_session(id) {
            live.control.stop();
        }
    }

    pub fn stop_all_sessions(&self) {
        for live in self.live_sessions() {
            live.control.stop();
        }
    }

    pub fn delete_session(&self, id: i64) -> Result<()> {
        if self.live_session(id).is_some() {
            return Err(Error::msg("Ferma l'agente prima di eliminare la sessione."));
        }
        self.store().delete_session(id)
    }

    /// Fa lavorare l'agente della sessione finché non ha finito, non viene
    /// fermato o non arriva al limite di passi. Più sessioni possono girare
    /// insieme (AG-6); la stessa sessione una volta sola.
    pub async fn run_session(&self, id: i64) -> Result<()> {
        let live = Arc::new(LiveSession::new());
        {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            if sessions.contains_key(&id) {
                return Err(Error::msg("L'agente sta già lavorando."));
            }
            sessions.insert(id, live.clone());
        }
        let _ = self
            .store()
            .set_session_status(id, SessionStatus::Running, None);

        let result = self.drive_session(id, &live).await;
        let (status, note) = match &result {
            Ok(outcome) if outcome.cancelled => (SessionStatus::Stopped, None),
            Ok(outcome) => match &outcome.notice {
                Some(notice) => (SessionStatus::Stopped, Some(notice.clone())),
                None => (SessionStatus::Done, None),
            },
            // Fermata mentre partivano i server MCP: non è un guasto.
            Err(_) if live.control.is_stopped() => (SessionStatus::Stopped, None),
            Err(error) => (SessionStatus::Failed, Some(error.to_string())),
        };
        let saved = self.store().set_session_status(id, status, note.as_deref());

        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        self.forget_approvals(&format!("s{id}"));
        saved?;
        result.map(|_| ())
    }

    async fn drive_session(&self, id: i64, live: &Arc<LiveSession>) -> Result<SendOutcome> {
        let session = self.store().session(id)?;
        let agent = session.definition.ok_or_else(|| {
            Error::msg("La definizione dell'agente di questa sessione è illeggibile.")
        })?;
        let target = self.target(&agent.model)?;
        let preset_key = match &target {
            Target::Local(loaded) => loaded.model_path.clone(),
            Target::Cloud(provider, _, model) => {
                format!("{CLOUD_KEY_PREFIX}{}:{model}", provider.id)
            }
        };
        let sampling = self.preset(&preset_key)?.sampling;
        let reads_images = matches!(target, Target::Cloud(..));

        let mut settings = self.conversation_tools(session.conversation_id)?;
        let folders: Vec<PathBuf> = agent.folders.iter().map(PathBuf::from).collect();
        let mut defs = tools::available(&settings, &folders);

        // I server MCP dell'agente partono con la sessione e si chiudono con lei.
        let servers = self.mcp_servers_for(&agent.mcp_servers)?;
        let mcp_set = if servers.is_empty() {
            None
        } else {
            let start = mcp::McpSet::start(&servers, folders.first().map(PathBuf::as_path));
            let set = tokio::select! {
                set = start => set?,
                _ = live.control.cancel.notified() => {
                    return Err(Error::msg("Fermato durante l'avvio dei server MCP."));
                }
            };
            defs.extend(set.defs().iter().cloned());
            Some(set)
        };

        let system = join_prompt(&[
            agents::AGENT_PREAMBLE,
            &agent.instructions,
            &tools::system_preamble(&defs, &folders),
        ]);
        let chat = chat_history(
            self.store().messages(session.conversation_id)?,
            reads_images,
        );

        let scope = RunScope {
            conversation_id: session.conversation_id,
            session_id: Some(id),
            source: agent.name.clone(),
            key_prefix: format!("s{id}"),
            control: &live.control,
            max_rounds: agent.max_steps as usize,
            limit_notice: format!(
                "L'agente ha fatto {} passi, il massimo previsto, e si è fermato. \
                 Rispondigli per farlo continuare.",
                agent.max_steps
            ),
            mcp: mcp_set.as_ref(),
        };
        let mut on_event = |event| {
            let mut state = live.state();
            match event {
                ChatEvent::Text(text) => state.streaming.push_str(&text),
                ChatEvent::Reasoning(text) => state.reasoning.push_str(&text),
                ChatEvent::Saved => {
                    state.streaming.clear();
                    state.reasoning.clear();
                    state.revision += 1;
                }
                ChatEvent::ToolRequest {
                    id,
                    name,
                    summary,
                    arguments,
                } => {
                    state.status = Some(SessionStatus::Waiting);
                    state.request = Some(PendingRequest {
                        id,
                        name,
                        summary,
                        arguments,
                    });
                }
            }
        };
        self.run_rounds(
            &scope,
            &target,
            chat,
            &defs,
            &mut settings,
            &folders,
            &system,
            &sampling,
            &mut on_event,
        )
        .await
    }

    // ── Provider cloud ──────────────────────────────────────────────────

    fn stored_providers(&self) -> Result<Vec<Provider>> {
        Ok(match self.store().setting(SETTING_PROVIDERS)? {
            Some(json) => serde_json::from_str(&json)?,
            None => Vec::new(),
        })
    }

    fn store_providers(&self, providers: &[Provider]) -> Result<()> {
        self.store()
            .set_setting(SETTING_PROVIDERS, &serde_json::to_string(providers)?)
    }

    fn provider(&self, id: &str) -> Result<Provider> {
        self.stored_providers()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::msg("Provider non trovato: forse è stato eliminato."))
    }

    fn provider_info(provider: Provider) -> ProviderInfo {
        ProviderInfo {
            // Se il portachiavi non risponde, l'interfaccia chiederà di reinserire la chiave.
            has_key: matches!(secrets::api_key(&provider.id), Ok(Some(_))),
            supported_params: provider.kind.supported_params(),
            provider,
        }
    }

    /// I provider configurati, in ordine alfabetico.
    pub fn providers(&self) -> Result<Vec<ProviderInfo>> {
        let mut providers = self.stored_providers()?;
        providers.sort_by_key(|p| p.name.to_lowercase());
        Ok(providers.into_iter().map(Self::provider_info).collect())
    }

    /// Crea un provider (se `id` è vuoto) o ne aggiorna uno esistente.
    /// La chiave API, se indicata, va nel portachiavi di sistema.
    pub fn save_provider(
        &self,
        mut provider: Provider,
        api_key: Option<&str>,
    ) -> Result<ProviderInfo> {
        provider.name = provider.name.trim().to_string();
        provider.base_url = provider.base().trim().to_string();
        providers::validate(&provider)?;

        let mut all = self.stored_providers()?;
        match all.iter_mut().find(|p| p.id == provider.id) {
            Some(existing) => {
                // L'elenco dei modelli non arriva dall'interfaccia: si conserva.
                provider.models = std::mem::take(&mut existing.models);
                *existing = provider.clone();
            }
            None => {
                provider.id = providers::new_id(&provider.name);
                provider.models.clear();
                all.push(provider.clone());
            }
        }
        if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
            secrets::set_api_key(&provider.id, key)?;
        }
        self.store_providers(&all)?;
        Ok(Self::provider_info(provider))
    }

    /// Elimina un provider e la sua chiave.
    pub fn delete_provider(&self, id: &str) -> Result<()> {
        let mut all = self.stored_providers()?;
        all.retain(|p| p.id != id);
        self.store_providers(&all)?;
        secrets::delete_api_key(id)
    }

    /// Scarica dal provider l'elenco aggiornato dei suoi modelli.
    pub async fn refresh_provider_models(&self, id: &str) -> Result<ProviderInfo> {
        let provider = self.provider(id)?;
        let key = secrets::api_key(id)?
            .ok_or_else(|| Error::msg("Inserisci prima la chiave API di questo provider."))?;
        let models = providers::fetch_models(&self.cloud_http, &provider, &key).await?;

        let mut all = self.stored_providers()?;
        let stored = all
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::msg("Provider non trovato: forse è stato eliminato."))?;
        stored.models = models;
        let updated = stored.clone();
        self.store_providers(&all)?;
        Ok(Self::provider_info(updated))
    }

    /// Interrompe la risposta in corso, se c'è. Il testo già arrivato viene salvato.
    pub fn stop_generation(&self) {
        if let Some(cancel) = self
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            cancel.stop();
        }
    }
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

/// La cronologia salvata, nella forma da inviare a un modello.
///
/// Ogni chiamata a uno strumento deve avere il suo risultato, altrimenti i
/// servizi rifiutano la conversazione: a quelle rimaste senza (app chiusa a
/// metà) se ne aggiunge uno di ripiego.
fn chat_history(history: Vec<Message>, with_images: bool) -> Vec<ChatMessage> {
    fn close(open: &mut Vec<ToolCall>, out: &mut Vec<ChatMessage>) {
        for call in open.drain(..) {
            out.push(ChatMessage {
                tool_call_id: Some(call.id),
                is_error: true,
                ..ChatMessage::new("tool", "Interrotto prima del risultato.")
            });
        }
    }

    let mut out = Vec::with_capacity(history.len());
    let mut open: Vec<ToolCall> = Vec::new();
    for m in history {
        if m.role == "tool" {
            // Un risultato senza la sua chiamata non si può inviare.
            let position = m
                .tool_call_id
                .as_ref()
                .and_then(|id| open.iter().position(|call| call.id == *id));
            if let Some(position) = position {
                open.remove(position);
                out.push(ChatMessage {
                    tool_call_id: m.tool_call_id,
                    is_error: m.is_error,
                    ..ChatMessage::new("tool", m.content)
                });
            }
            continue;
        }
        close(&mut open, &mut out);

        if m.role == "assistant" {
            if m.content.is_empty() && m.tool_calls.is_empty() {
                continue;
            }
            open = m.tool_calls.clone();
            out.push(ChatMessage {
                tool_calls: m.tool_calls,
                ..ChatMessage::new("assistant", m.content)
            });
        } else {
            let mut content = attachments::with_text_attachments(&m.content, &m.attachment_data);
            let mut images = Vec::new();
            for a in m.attachment_data {
                if a.kind != AttachmentKind::Image {
                    continue;
                }
                if with_images {
                    images.push(chat::Image {
                        media_type: a.media_type.unwrap_or_else(|| "image/png".to_string()),
                        data: a.data,
                    });
                } else {
                    content.push_str(&format!(
                        "\n[Immagine allegata che questo modello non può vedere: {}]",
                        a.name
                    ));
                }
            }
            out.push(ChatMessage {
                images,
                ..ChatMessage::new(&m.role, content)
            });
        }
    }
    close(&mut open, &mut out);
    out
}

/// Byte in gigabyte con una cifra decimale e la virgola, come si scrive in italiano.
fn gib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0 * 1024.0)).replace('.', ",")
}

/// Titolo di una conversazione ricavato dal primo messaggio.
fn title_from(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim();
    if line.is_empty() {
        return DEFAULT_TITLE.to_string();
    }
    if line.chars().count() <= TITLE_MAX_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(TITLE_MAX_CHARS).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_titolo_viene_dal_primo_messaggio() {
        assert_eq!(title_from("Ciao, come va?"), "Ciao, come va?");
        assert_eq!(title_from("\n\n  prima riga  \nseconda"), "prima riga");
        assert_eq!(title_from("   "), DEFAULT_TITLE);

        let long = "parola ".repeat(20);
        let title = title_from(&long);
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= TITLE_MAX_CHARS + 1);
    }

    #[test]
    fn preset_salvato_e_riletto() {
        let dir = std::env::temp_dir().join(format!("core-preset-{}", std::process::id()));
        let core = Core::new(&dir).unwrap();

        let model = r"C:\modelli\prova.gguf";
        assert_eq!(core.preset(model).unwrap(), Preset::default());

        let mut preset = Preset::default();
        preset.sampling.temperature = 0.2;
        preset.load.placement = Placement::Ram;
        preset.system_prompt = "Rispondi in italiano.".to_string();
        core.save_preset(model, &preset).unwrap();
        assert_eq!(core.preset(model).unwrap(), preset);

        drop(core);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_cronologia_chiude_le_chiamate_rimaste_aperte() {
        let store = Store::open_in_memory().unwrap();
        let conv = store.create_conversation("prova").unwrap();
        let call = |id: &str| ToolCall {
            id: id.into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "a.txt"}),
        };
        let image = Attachment {
            name: "foto.png".into(),
            kind: AttachmentKind::Image,
            data: "AQID".into(),
            media_type: Some("image/png".into()),
        };
        store
            .add_message_with(conv.id, "user", "guarda", None, None, &[image])
            .unwrap();
        // Due chiamate, ma l'app si è chiusa dopo il primo risultato.
        let calls = ToolData {
            calls: vec![call("a"), call("b")],
            ..Default::default()
        };
        store
            .add_message_with(conv.id, "assistant", "", Some("m"), Some(&calls), &[])
            .unwrap();
        let result = ToolData {
            call_id: Some("a".into()),
            ..Default::default()
        };
        store
            .add_message_with(conv.id, "tool", "contenuto", None, Some(&result), &[])
            .unwrap();
        // Un risultato orfano non va inviato.
        let orphan = ToolData {
            call_id: Some("z".into()),
            ..Default::default()
        };
        store
            .add_message_with(conv.id, "tool", "???", None, Some(&orphan), &[])
            .unwrap();
        store.add_message(conv.id, "user", "allora?", None).unwrap();

        let history = store.messages(conv.id).unwrap();
        let chat = chat_history(history.clone(), true);
        let roles: Vec<&str> = chat.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["user", "assistant", "tool", "tool", "user"]);
        assert_eq!(chat[0].images.len(), 1);
        assert_eq!(chat[3].tool_call_id.as_deref(), Some("b"));
        assert!(chat[3].is_error);

        // Un modello che non vede le immagini riceve almeno un avviso.
        let blind = chat_history(history, false);
        assert!(blind[0].images.is_empty());
        assert!(blind[0].content.contains("foto.png"));
    }

    /// Un servizio compatibile OpenAI finto: a ogni richiesta risponde con il
    /// prossimo flusso SSE dell'elenco. Restituisce l'indirizzo base.
    fn fake_service(replies: Vec<Vec<serde_json::Value>>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for (stream, chunks) in listener.incoming().zip(replies) {
                let mut stream = stream.unwrap();
                // Si legge la richiesta intera: intestazioni e corpo.
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    request.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&request).to_lowercase();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let mut body = String::new();
                for chunk in chunks {
                    body.push_str(&format!("data: {chunk}\n\n"));
                }
                body.push_str("data: [DONE]\n\n");
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        format!("http://127.0.0.1:{port}/v1")
    }

    fn command_call(command: &str) -> Vec<serde_json::Value> {
        let arguments = serde_json::json!({ "command": command }).to_string();
        vec![serde_json::json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "id": "call_1",
            "function": {"name": "run_command", "arguments": arguments}
        }]}, "finish_reason": "tool_calls"}]})]
    }

    fn final_text(text: &str) -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({"choices": [{"delta": {"content": text}, "finish_reason": "stop"}]}),
        ]
    }

    /// Nucleo, cartella di lavoro e sessione di un agente pronti per una prova.
    fn agent_fixture(name: &str, permission: PermissionLevel) -> (Core, PathBuf, AgentSession) {
        let dir = std::env::temp_dir().join(format!("core-agent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let work = dir.join("progetto");
        std::fs::create_dir_all(&work).unwrap();
        let core = Core::new(&dir.join("dati")).unwrap();
        let agent = core
            .save_agent(Agent {
                name: "Prova".into(),
                folders: vec![work.to_string_lossy().into_owned()],
                tools: ToolSettings {
                    files: true,
                    web: false,
                    terminal: true,
                    permission,
                    allowed_tools: Vec::new(),
                },
                ..Default::default()
            })
            .unwrap();
        let session = core.create_session(agent.id, "Esegui il comando").unwrap();
        (core, dir, session)
    }

    async fn drive(
        core: &Core,
        session: &AgentSession,
        base_url: String,
        control: &RunControl,
        on_event: &mut impl FnMut(ChatEvent),
    ) -> SendOutcome {
        let agent = session.definition.clone().unwrap();
        let folders: Vec<PathBuf> = agent.folders.iter().map(PathBuf::from).collect();
        let mut settings = core.conversation_tools(session.conversation_id).unwrap();
        let defs = tools::available(&settings, &folders);
        let provider = Provider {
            id: "finto".into(),
            name: "Finto".into(),
            kind: providers::ProviderKind::OpenaiCompatible,
            base_url,
            models: Vec::new(),
        };
        let target = Target::Cloud(provider, "chiave".into(), "modello".into());
        let chat = chat_history(core.messages(session.conversation_id).unwrap(), true);
        let scope = RunScope {
            conversation_id: session.conversation_id,
            session_id: Some(session.id),
            source: agent.name.clone(),
            key_prefix: format!("s{}", session.id),
            control,
            max_rounds: 10,
            limit_notice: "limite".into(),
            mcp: None,
        };
        core.run_rounds(
            &scope,
            &target,
            chat,
            &defs,
            &mut settings,
            &folders,
            "",
            &SamplingParams::default(),
            on_event,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn un_agente_usa_il_terminale_e_poi_risponde() {
        let (core, dir, session) = agent_fixture("ok", PermissionLevel::All);
        let base = fake_service(vec![command_call("echo 'fatto'"), final_text("Ho finito.")]);
        let control = RunControl::new();
        let outcome = drive(&core, &session, base, &control, &mut |_| {}).await;

        assert!(!outcome.cancelled);
        assert_eq!(outcome.notice, None);
        let messages = core.messages(session.conversation_id).unwrap();
        let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["user", "assistant", "tool", "assistant"]);
        assert!(
            messages[2].content.contains("fatto"),
            "{}",
            messages[2].content
        );
        assert!(!messages[2].is_error);
        assert_eq!(messages[3].content, "Ho finito.");

        let actions = core.actions(10, Some(session.id)).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].tool, "run_command");
        assert_eq!(actions[0].approval, "auto");
        assert_eq!(actions[0].source, "Prova");

        drop(core);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn fermare_un_agente_in_attesa_di_conferma_non_esegue_il_comando() {
        let (core, dir, session) = agent_fixture("stop", PermissionLevel::Ask);
        let base = fake_service(vec![command_call("New-Item segno.txt")]);
        let control = RunControl::new();
        let mut asked = None;
        let outcome = drive(&core, &session, base, &control, &mut |event| {
            if let ChatEvent::ToolRequest { id, summary, .. } = event {
                asked = Some((id, summary));
                control.stop();
            }
        })
        .await;

        let (id, summary) = asked.expect("doveva chiedere conferma");
        assert_eq!(id, format!("s{}:call_1", session.id));
        assert!(summary.contains("New-Item"));
        assert!(outcome.cancelled);
        let messages = core.messages(session.conversation_id).unwrap();
        let last = messages.last().unwrap();
        assert_eq!(last.role, "tool");
        assert!(last.is_error);
        assert!(!dir.join("progetto").join("segno.txt").exists());
        assert_eq!(
            core.actions(10, Some(session.id)).unwrap()[0].approval,
            "interrupted"
        );

        drop(core);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn consenti_sempre_su_uno_strumento_mcp_vale_solo_per_quello() {
        let (core, dir, session) = agent_fixture("mcp", PermissionLevel::Ask);
        let server = mcp::tests::fake_server(&dir);
        let set = mcp::McpSet::start(std::slice::from_ref(&server), None)
            .await
            .unwrap();
        let call = |id: &str, text: &str| {
            let arguments = serde_json::json!({ "text": text }).to_string();
            vec![serde_json::json!({"choices": [{"delta": {"tool_calls": [{
                "index": 0, "id": id,
                "function": {"name": "mcp__Finto__echo", "arguments": arguments}
            }]}, "finish_reason": "tool_calls"}]})]
        };
        let base = fake_service(vec![
            call("c1", "uno"),
            call("c2", "due"),
            final_text("Fatto."),
        ]);

        let agent = session.definition.clone().unwrap();
        let folders: Vec<PathBuf> = agent.folders.iter().map(PathBuf::from).collect();
        let mut settings = core.conversation_tools(session.conversation_id).unwrap();
        let mut defs = tools::available(&settings, &folders);
        defs.extend(set.defs().iter().cloned());
        let provider = Provider {
            id: "finto".into(),
            name: "Finto".into(),
            kind: providers::ProviderKind::OpenaiCompatible,
            base_url: base,
            models: Vec::new(),
        };
        let target = Target::Cloud(provider, "chiave".into(), "modello".into());
        let chat = chat_history(core.messages(session.conversation_id).unwrap(), true);
        let control = RunControl::new();
        let scope = RunScope {
            conversation_id: session.conversation_id,
            session_id: Some(session.id),
            source: "Prova".into(),
            key_prefix: format!("s{}", session.id),
            control: &control,
            max_rounds: 10,
            limit_notice: "limite".into(),
            mcp: Some(&set),
        };
        let mut asked = Vec::new();
        let outcome = core
            .run_rounds(
                &scope,
                &target,
                chat,
                &defs,
                &mut settings,
                &folders,
                "",
                &SamplingParams::default(),
                &mut |event| {
                    if let ChatEvent::ToolRequest { id, summary, .. } = event {
                        asked.push(summary);
                        core.answer_tool_request(&id, Approval::Always);
                    }
                },
            )
            .await
            .unwrap();

        assert!(!outcome.cancelled);
        // Chiesto una volta sola; il livello della sessione non cambia.
        assert_eq!(asked.len(), 1);
        assert!(asked[0].contains("«echo»"), "{}", asked[0]);
        let saved = core.conversation_tools(session.conversation_id).unwrap();
        assert_eq!(saved.permission, PermissionLevel::Ask);
        assert_eq!(saved.allowed_tools, ["mcp__Finto__echo"]);
        let results: Vec<String> = core
            .messages(session.conversation_id)
            .unwrap()
            .into_iter()
            .filter(|m| m.role == "tool")
            .map(|m| m.content)
            .collect();
        assert_eq!(results, ["eco: uno", "eco: due"]);
        let approvals: Vec<String> = core
            .actions(10, Some(session.id))
            .unwrap()
            .into_iter()
            .map(|a| a.approval)
            .collect();
        assert_eq!(approvals, ["auto", "user"]);

        drop(set);
        drop(core);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn le_cartelle_dei_modelli_si_ricordano() {
        let dir = std::env::temp_dir().join(format!("core-folders-{}", std::process::id()));
        let core = Core::new(&dir).unwrap();

        assert!(core.model_folders().unwrap().is_empty());
        core.set_model_folders(&["C:\\modelli".to_string()])
            .unwrap();
        assert_eq!(core.model_folders().unwrap(), vec!["C:\\modelli"]);

        drop(core);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
