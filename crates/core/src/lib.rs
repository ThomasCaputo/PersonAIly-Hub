//! Nucleo dell'app.
//!
//! Contiene tutta la logica, senza interfaccia grafica: l'app desktop e
//! l'estensione VS Code sono solo client di questa libreria
//! (vedi `docs/architettura.md`).

pub mod chat;
pub mod error;
pub mod gguf;
pub mod hub;
pub mod local;
pub mod models;
pub mod named_presets;
pub mod providers;
pub mod secrets;
pub mod store;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use providers::Provider;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use chat::{ChatEvent, ChatMessage, GenerationStats};
use error::{Error, Result};
use hub::{DownloadEnd, HubFile, HubRepo};
use local::install::{self, Variant};
use local::runtime::{LoadedModel, LocalRuntime};
use local::sysmon::{self, MemorySnapshot, ProcessGpuMemory};
use local::{LoadParams, MemoryEstimate, Placement, Preset, SamplingParams};
use models::ModelInfo;
use named_presets::NamedPreset;
use store::{Conversation, Message, Store};

const SETTING_MODEL_FOLDERS: &str = "model_folders";
const SETTING_PROVIDERS: &str = "providers";
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
    /// Segnale per interrompere la generazione in corso.
    generation: Mutex<Option<Arc<Notify>>>,
    /// Occupato mentre llama.cpp viene scaricato e installato.
    installing: tokio::sync::Mutex<()>,
    /// Stato dell'ultimo download di un modello, letto dall'interfaccia.
    download: Mutex<Option<DownloadState>>,
    /// Segnale per interrompere il download in corso.
    download_cancel: Mutex<Option<Arc<Notify>>>,
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
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            store: Mutex::new(Store::open(&data_dir.join("data.sqlite"))?),
            runtime: tokio::sync::Mutex::new(LocalRuntime::new(
                data_dir.to_path_buf(),
                http.clone(),
            )),
            http,
            cloud_http: reqwest::Client::new(),
            generation: Mutex::new(None),
            installing: tokio::sync::Mutex::new(()),
            download: Mutex::new(None),
            download_cancel: Mutex::new(None),
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

    /// Salva il messaggio dell'utente, lo invia al modello caricato insieme alla
    /// cronologia e salva la risposta. `on_event` riceve la risposta mentre arriva.
    pub async fn send_message(
        &self,
        conversation_id: i64,
        text: &str,
        sampling: &SamplingParams,
        system_prompt: &str,
        model: &ModelRef,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<SendOutcome> {
        // Prima si verifica di poter raggiungere il modello, poi si salva il messaggio.
        enum Target {
            Local(LoadedModel),
            Cloud(Provider, String),
        }
        let target = match model {
            ModelRef::Local => Target::Local(
                self.runtime
                    .try_lock()
                    .ok()
                    .and_then(|mut runtime| runtime.loaded())
                    .ok_or_else(|| Error::msg("Nessun modello locale caricato."))?,
            ),
            ModelRef::Cloud { provider_id, .. } => {
                let provider = self.provider(provider_id)?;
                let key = secrets::api_key(provider_id)?.ok_or_else(|| {
                    Error::msg(format!(
                        "Manca la chiave API di {}: inseriscila nella pagina Modelli.",
                        provider.name
                    ))
                })?;
                Target::Cloud(provider, key)
            }
        };

        let history = {
            let store = self.store();
            let is_first = store.messages(conversation_id)?.is_empty();
            store.add_message(conversation_id, "user", text, None)?;
            if is_first {
                store.rename_conversation(conversation_id, &title_from(text))?;
            }
            store.messages(conversation_id)?
        };

        let messages: Vec<ChatMessage> = history
            .into_iter()
            .map(|m| ChatMessage {
                role: m.role,
                content: m.content,
            })
            .collect();

        let cancel = Arc::new(Notify::new());
        *self.generation.lock().unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());

        let (result, model_name) = match &target {
            Target::Local(loaded) => {
                let mut all = Vec::with_capacity(messages.len() + 1);
                if !system_prompt.trim().is_empty() {
                    all.push(ChatMessage {
                        role: "system".to_string(),
                        content: system_prompt.to_string(),
                    });
                }
                all.extend(messages);
                let url = format!("http://127.0.0.1:{}/v1/chat/completions", loaded.port);
                let body = chat::request_body(&all, sampling, None, chat::Flavor::Llama);
                let result =
                    chat::stream_chat(&self.http, &url, None, &body, cancel, on_event).await;
                (result, loaded.model_name.clone())
            }
            Target::Cloud(provider, key) => {
                let ModelRef::Cloud { model: id, .. } = model else {
                    unreachable!("il bersaglio cloud nasce solo da un riferimento cloud");
                };
                let result = providers::stream(
                    &self.cloud_http,
                    provider,
                    key,
                    id,
                    &messages,
                    system_prompt,
                    sampling,
                    cancel,
                    on_event,
                )
                .await;
                (result, id.clone())
            }
        };

        *self.generation.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let outcome = result?;

        // Il costo si stima solo per i modelli cloud di cui si conosce il prezzo.
        let stats = &outcome.stats;
        let cost_usd = match (&target, model) {
            (Target::Cloud(provider, _), ModelRef::Cloud { model: id, .. }) => provider
                .model(id)
                .and_then(|m| m.cost(stats.prompt_tokens, stats.completion_tokens)),
            _ => None,
        };

        let store = self.store();
        let assistant_message = if outcome.text.is_empty() {
            None
        } else {
            let message = store.add_message(
                conversation_id,
                "assistant",
                &outcome.text,
                Some(&model_name),
            )?;
            Some(store.set_message_usage(
                message.id,
                stats.prompt_tokens,
                stats.completion_tokens,
                cost_usd,
            )?)
        };
        Ok(SendOutcome {
            conversation: store.conversation(conversation_id)?,
            assistant_message,
            stats: outcome.stats,
            cancelled: outcome.cancelled,
            notice: outcome.notice,
        })
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
            cancel.notify_one();
        }
    }
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
