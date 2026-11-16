//! Guscio dell'app desktop: crea la finestra e inoltra i comandi al nucleo.
//! Qui non va logica propria: sta tutta in `app-core`.
//!
//! Il nucleo vive finché vive il processo, non la finestra: chiudendola,
//! l'interfaccia si distrugge (e libera la sua memoria) mentre il nucleo resta
//! attivo nell'area di notifica, con il modello caricato, gli agenti al lavoro
//! e l'API locale per VS Code ([ADR 0013](../../../../docs/adr/0013-nucleo-in-area-di-notifica.md)).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use app_core::DownloadState;
use app_core::agents::{Agent, AgentSession, LiveState};
use app_core::chat::ChatEvent;
use app_core::hub::{HubFile, HubRepo};
use app_core::instance::InstanceKind;
use app_core::local::install::{Phase, Progress, Variant};
use app_core::local::runtime::InstalledEngine;
use app_core::local::runtime::LoadedModel;
use app_core::local::{LoadParams, Preset, SamplingParams};
use app_core::mcp::{McpServerConfig, McpToolInfo};
use app_core::models::ModelInfo;
use app_core::named_presets::NamedPreset;
use app_core::providers::Provider;
use app_core::store::{ActionRecord, Conversation, Message};
use app_core::tools::ToolSettings;
use app_core::{
    Approval, Core, EngineStatus, LoadCheck, ModelRef, Preferences, ProviderInfo, RuntimeStatus,
    SecuritySettings, SendOutcome,
};
use app_server::ApiHandle;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, State, WebviewWindowBuilder};

const MAIN_WINDOW: &str = "main";
const TRAY_ID: &str = "main";

/// Come si sta chiudendo l'app.
#[derive(Default)]
struct Lifecycle {
    /// L'icona nell'area di notifica c'è: si può chiudere la finestra senza
    /// perdere il modo di riaprirla.
    tray: AtomicBool,
    /// L'utente ha scelto "Esci": il processo deve finire davvero.
    quitting: AtomicBool,
}

type CoreState<'a> = State<'a, Arc<Core>>;

/// L'API locale avviata dall'app (VS-1), oppure il motivo per cui non è partita.
#[derive(Default)]
struct LocalApi {
    handle: tokio::sync::Mutex<Option<ApiHandle>>,
    error: Mutex<Option<String>>,
}

type ApiState<'a> = State<'a, Arc<LocalApi>>;
type CmdResult<T> = Result<T, String>;

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Esegue fuori dal thread dell'interfaccia un'operazione che legge il disco.
async fn blocking<T: Send + 'static>(
    core: &Arc<Core>,
    f: impl FnOnce(&Core) -> app_core::error::Result<T> + Send + 'static,
) -> CmdResult<T> {
    let core = core.clone();
    tauri::async_runtime::spawn_blocking(move || f(&core))
        .await
        .map_err(text)?
        .map_err(text)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CoreInfo {
    version: &'static str,
    schema_version: i64,
}

#[tauri::command(async)]
fn core_info(core: CoreState<'_>) -> CmdResult<CoreInfo> {
    Ok(CoreInfo {
        version: app_core::version(),
        schema_version: core.schema_version().map_err(text)?,
    })
}

// ── Motore llama.cpp ────────────────────────────────────────────────────

#[tauri::command]
async fn engine_status(core: CoreState<'_>) -> CmdResult<EngineStatus> {
    Ok(core.engine_status().await)
}

#[tauri::command(async)]
fn set_engine_variant(core: CoreState<'_>, variant: Variant) -> CmdResult<()> {
    core.set_engine_variant(variant).map_err(text)
}

#[tauri::command]
async fn remove_engine(core: CoreState<'_>, dir_name: String) -> CmdResult<Vec<InstalledEngine>> {
    core.remove_engine(&dir_name).await.map_err(text)?;
    Ok(core.engine_status().await.engines)
}

#[tauri::command]
async fn install_engine(
    core: CoreState<'_>,
    variant: Variant,
    on_progress: Channel<Progress>,
) -> CmdResult<()> {
    // Un aggiornamento per ogni pacchetto di rete sarebbero migliaia di
    // messaggi al secondo: se ne manda uno ogni mezzo punto percentuale.
    let mut last_sent = 0;
    core.install_engine(variant, |progress| {
        let step = progress.total_bytes / 200;
        if progress.phase != Phase::Download || progress.done_bytes >= last_sent + step {
            last_sent = progress.done_bytes;
            let _ = on_progress.send(progress);
        }
    })
    .await
    .map_err(text)
}

// ── Modelli ─────────────────────────────────────────────────────────────

#[tauri::command(async)]
fn model_folders(core: CoreState<'_>) -> CmdResult<Vec<String>> {
    core.model_folders().map_err(text)
}

#[tauri::command(async)]
fn set_model_folders(core: CoreState<'_>, folders: Vec<String>) -> CmdResult<()> {
    core.set_model_folders(&folders).map_err(text)
}

#[tauri::command]
async fn list_models(core: CoreState<'_>) -> CmdResult<Vec<ModelInfo>> {
    blocking(&core, |core| core.list_models()).await
}

#[tauri::command(async)]
fn get_preset(core: CoreState<'_>, model_path: String) -> CmdResult<Preset> {
    core.preset(&model_path).map_err(text)
}

#[tauri::command(async)]
fn save_preset(core: CoreState<'_>, model_path: String, preset: Preset) -> CmdResult<()> {
    core.save_preset(&model_path, &preset).map_err(text)
}

#[tauri::command]
async fn check_load(
    core: CoreState<'_>,
    model_path: String,
    params: LoadParams,
) -> CmdResult<LoadCheck> {
    blocking(&core, move |core| Ok(core.check_load(&model_path, &params))).await
}

#[tauri::command]
async fn load_model(
    core: CoreState<'_>,
    model_path: String,
    params: LoadParams,
    force: bool,
) -> CmdResult<LoadedModel> {
    core.load_model(&model_path, &params, force)
        .await
        .map_err(text)
}

#[tauri::command]
async fn unload_model(core: CoreState<'_>) -> CmdResult<()> {
    core.unload_model().await;
    Ok(())
}

#[tauri::command]
async fn runtime_status(core: CoreState<'_>) -> CmdResult<RuntimeStatus> {
    Ok(core.status().await)
}

// ── Conversazioni ───────────────────────────────────────────────────────

#[tauri::command(async)]
fn list_conversations(core: CoreState<'_>, query: String) -> CmdResult<Vec<Conversation>> {
    core.list_conversations(&query).map_err(text)
}

#[tauri::command(async)]
fn create_conversation(core: CoreState<'_>) -> CmdResult<Conversation> {
    core.create_conversation().map_err(text)
}

#[tauri::command(async)]
fn rename_conversation(core: CoreState<'_>, id: i64, title: String) -> CmdResult<()> {
    core.rename_conversation(id, &title).map_err(text)
}

#[tauri::command(async)]
fn delete_conversation(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.delete_conversation(id).map_err(text)
}

#[tauri::command(async)]
fn get_messages(core: CoreState<'_>, conversation_id: i64) -> CmdResult<Vec<Message>> {
    core.messages_for_display(conversation_id).map_err(text)
}

/// Pezzi di risposta inviati all'interfaccia mentre il modello genera.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum StreamEvent {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    /// Nuovi messaggi in archivio: l'interfaccia li rilegge.
    Saved,
    /// Uno strumento aspetta la conferma dell'utente.
    ToolRequest {
        id: String,
        name: String,
        summary: String,
        arguments: serde_json::Value,
    },
}

/// Inoltra all'interfaccia gli eventi di una risposta in arrivo.
fn stream_to(channel: Channel<StreamEvent>) -> impl FnMut(ChatEvent) {
    move |event| {
        let event = match event {
            ChatEvent::Text(text) => StreamEvent::Text { text },
            ChatEvent::Reasoning(text) => StreamEvent::Reasoning { text },
            ChatEvent::Saved => StreamEvent::Saved,
            ChatEvent::ToolRequest {
                id,
                name,
                summary,
                arguments,
            } => StreamEvent::ToolRequest {
                id,
                name,
                summary,
                arguments,
            },
        };
        // Se la finestra è stata chiusa non c'è più nessuno ad ascoltare: la
        // risposta continua e finisce comunque nell'archivio.
        let _ = channel.send(event);
    }
}

/// Tauri passa ogni parametro a parte: raggrupparli complicherebbe l'interfaccia.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn send_message(
    core: CoreState<'_>,
    conversation_id: i64,
    text: String,
    attachments: Vec<String>,
    sampling: SamplingParams,
    system_prompt: String,
    model: ModelRef,
    on_event: Channel<StreamEvent>,
) -> CmdResult<SendOutcome> {
    core.send_message(
        conversation_id,
        &text,
        &attachments,
        &sampling,
        &system_prompt,
        &model,
        stream_to(on_event),
    )
    .await
    .map_err(self::text)
}

/// Modifica (con `text`) o ripete un messaggio dell'utente e rigenera la
/// risposta; i messaggi successivi si cancellano.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn resend_message(
    core: CoreState<'_>,
    conversation_id: i64,
    message_id: i64,
    text: Option<String>,
    sampling: SamplingParams,
    system_prompt: String,
    model: ModelRef,
    on_event: Channel<StreamEvent>,
) -> CmdResult<SendOutcome> {
    core.resend_message(
        conversation_id,
        message_id,
        text.as_deref(),
        &sampling,
        &system_prompt,
        &model,
        stream_to(on_event),
    )
    .await
    .map_err(self::text)
}

/// `path` arriva dalla finestra "Salva con nome" del sistema.
#[tauri::command]
async fn export_conversation(
    core: CoreState<'_>,
    conversation_id: i64,
    path: String,
    lang: String,
) -> CmdResult<()> {
    blocking(&core, move |core| {
        core.export_conversation(conversation_id, std::path::Path::new(&path), &lang)
    })
    .await
}

// ── Strumenti e permessi ────────────────────────────────────────────────

#[tauri::command(async)]
fn granted_folders(core: CoreState<'_>) -> CmdResult<Vec<String>> {
    core.granted_folders().map_err(text)
}

/// Le cartelle arrivano dalla finestra `"Scegli cartella`" del sistema.
#[tauri::command(async)]
fn set_granted_folders(core: CoreState<'_>, folders: Vec<String>) -> CmdResult<()> {
    core.set_granted_folders(&folders).map_err(text)
}

#[tauri::command(async)]
fn tool_defaults(core: CoreState<'_>) -> CmdResult<ToolSettings> {
    core.tool_defaults().map_err(text)
}

#[tauri::command(async)]
fn set_tool_defaults(core: CoreState<'_>, settings: ToolSettings) -> CmdResult<()> {
    core.set_tool_defaults(&settings).map_err(text)
}

#[tauri::command(async)]
fn conversation_tools(core: CoreState<'_>, conversation_id: i64) -> CmdResult<ToolSettings> {
    core.conversation_tools(conversation_id).map_err(text)
}

#[tauri::command(async)]
fn set_conversation_tools(
    core: CoreState<'_>,
    conversation_id: i64,
    settings: ToolSettings,
) -> CmdResult<()> {
    core.set_conversation_tools(conversation_id, &settings)
        .map_err(text)
}

/// Risposta dell'utente a una richiesta di conferma. Arriva solo da un clic
/// nell'interfaccia: il modello non ha modo di chiamare questo comando.
#[tauri::command]
fn answer_tool_request(core: CoreState<'_>, id: String, approval: Approval) {
    core.answer_tool_request(&id, approval);
}

#[tauri::command(async)]
fn list_actions(
    core: CoreState<'_>,
    limit: u32,
    session_id: Option<i64>,
) -> CmdResult<Vec<ActionRecord>> {
    core.actions(limit, session_id).map_err(text)
}

#[tauri::command(async)]
fn action_log_days(core: CoreState<'_>) -> CmdResult<u32> {
    core.action_log_days().map_err(text)
}

/// Restituisce quante voci sono state eliminate perché troppo vecchie.
#[tauri::command(async)]
fn set_action_log_days(core: CoreState<'_>, days: u32) -> CmdResult<usize> {
    core.set_action_log_days(days).map_err(text)
}

#[tauri::command(async)]
fn clear_actions(core: CoreState<'_>) -> CmdResult<usize> {
    core.clear_actions().map_err(text)
}

// ── Server MCP ──────────────────────────────────────────────────────────

#[tauri::command(async)]
fn list_mcp_servers(core: CoreState<'_>) -> CmdResult<Vec<McpServerConfig>> {
    core.mcp_servers().map_err(text)
}

#[tauri::command(async)]
fn save_mcp_server(core: CoreState<'_>, server: McpServerConfig) -> CmdResult<McpServerConfig> {
    core.save_mcp_server(server).map_err(text)
}

#[tauri::command(async)]
fn delete_mcp_server(core: CoreState<'_>, id: String) -> CmdResult<()> {
    core.delete_mcp_server(&id).map_err(text)
}

/// Avvia il server per un momento, solo per elencarne gli strumenti.
#[tauri::command]
async fn probe_mcp_server(
    core: CoreState<'_>,
    server: McpServerConfig,
) -> CmdResult<Vec<McpToolInfo>> {
    core.probe_mcp_server(&server).await.map_err(text)
}

// ── Agenti ──────────────────────────────────────────────────────────────

#[tauri::command(async)]
fn list_agents(core: CoreState<'_>) -> CmdResult<Vec<Agent>> {
    core.agents().map_err(text)
}

#[tauri::command(async)]
fn save_agent(core: CoreState<'_>, agent: Agent) -> CmdResult<Agent> {
    core.save_agent(agent).map_err(text)
}

#[tauri::command(async)]
fn duplicate_agent(core: CoreState<'_>, id: i64) -> CmdResult<Agent> {
    core.duplicate_agent(id).map_err(text)
}

#[tauri::command(async)]
fn delete_agent(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.delete_agent(id).map_err(text)
}

/// Fa lavorare una sessione in sottofondo: l'esito finisce nell'archivio e
/// l'interfaccia lo segue con `agent_session_live` e `list_agent_sessions`.
fn spawn_session(core: &Arc<Core>, id: i64) {
    let core = core.clone();
    tauri::async_runtime::spawn(async move {
        let _ = core.run_session(id).await;
    });
}

#[tauri::command(async)]
fn start_agent_session(
    core: CoreState<'_>,
    agent_id: i64,
    task: String,
) -> CmdResult<AgentSession> {
    let session = core.create_session(agent_id, &task).map_err(text)?;
    spawn_session(core.inner(), session.id);
    Ok(session)
}

#[tauri::command(async)]
fn reply_agent_session(core: CoreState<'_>, id: i64, text: String) -> CmdResult<AgentSession> {
    let session = core.reply_session(id, &text).map_err(self::text)?;
    spawn_session(core.inner(), id);
    Ok(session)
}

#[tauri::command(async)]
fn list_agent_sessions(core: CoreState<'_>) -> CmdResult<Vec<AgentSession>> {
    core.sessions().map_err(text)
}

#[tauri::command(async)]
fn get_agent_session(core: CoreState<'_>, id: i64) -> CmdResult<AgentSession> {
    core.session(id).map_err(text)
}

#[tauri::command]
fn agent_session_live(core: CoreState<'_>, id: i64) -> Option<LiveState> {
    core.session_live(id)
}

#[tauri::command(async)]
fn pause_agent_session(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.pause_session(id).map_err(text)
}

#[tauri::command(async)]
fn resume_agent_session(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.resume_session(id).map_err(text)
}

#[tauri::command]
fn stop_agent_session(core: CoreState<'_>, id: i64) {
    core.stop_session(id);
}

#[tauri::command(async)]
fn delete_agent_session(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.delete_session(id).map_err(text)
}

// ── Provider cloud ──────────────────────────────────────────────────────

#[tauri::command]
async fn list_providers(core: CoreState<'_>) -> CmdResult<Vec<ProviderInfo>> {
    blocking(&core, |core| core.providers()).await
}

/// `api_key` vuota o assente lascia invariata la chiave già salvata.
#[tauri::command]
async fn save_provider(
    core: CoreState<'_>,
    provider: Provider,
    api_key: Option<String>,
) -> CmdResult<ProviderInfo> {
    blocking(&core, move |core| {
        core.save_provider(provider, api_key.as_deref())
    })
    .await
}

#[tauri::command]
async fn delete_provider(core: CoreState<'_>, id: String) -> CmdResult<()> {
    blocking(&core, move |core| core.delete_provider(&id)).await
}

#[tauri::command]
async fn refresh_provider_models(core: CoreState<'_>, id: String) -> CmdResult<ProviderInfo> {
    core.refresh_provider_models(&id).await.map_err(text)
}

/// Senza `conversation_id` ferma tutte le risposte in corso in chat.
#[tauri::command]
fn stop_generation(core: CoreState<'_>, conversation_id: Option<i64>) {
    match conversation_id {
        Some(id) => core.stop_conversation(id),
        None => core.stop_generation(),
    }
}

// ── API locale ──────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiInfo {
    running: bool,
    port: Option<u16>,
    token: Option<String>,
    error: Option<String>,
}

#[tauri::command]
async fn api_info(api: ApiState<'_>) -> CmdResult<ApiInfo> {
    let handle = api.handle.lock().await;
    Ok(ApiInfo {
        running: handle.is_some(),
        port: handle.as_ref().map(ApiHandle::port),
        token: handle.as_ref().map(ApiHandle::token),
        error: api.error.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    })
}

/// Il token vecchio smette subito di valere: i programmi configurati a mano
/// vanno aggiornati; l'estensione VS Code rilegge da sola il file di blocco.
#[tauri::command]
async fn regenerate_api_token(api: ApiState<'_>) -> CmdResult<String> {
    let handle = api.handle.lock().await;
    let handle = handle.as_ref().ok_or("L'API locale non è attiva.")?;
    handle.regenerate_token()
}

/// Avvia l'API locale dentro l'app. Se l'estensione VS Code aveva avviato il
/// nucleo senza interfaccia, gli chiede di chiudersi: resta un nucleo solo.
fn spawn_local_api(core: Arc<Core>, data_dir: std::path::PathBuf, api: Arc<LocalApi>) {
    tauri::async_runtime::spawn(async move {
        let result = match app_server::replace_server(&data_dir).await {
            Ok(()) => {
                app_server::start(core, &data_dir, InstanceKind::App, app_server::DEFAULT_PORT)
                    .await
            }
            Err(e) => Err(e),
        };
        match result {
            Ok(handle) => *api.handle.lock().await = Some(handle),
            Err(e) => *api.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e),
        }
    });
}

// ── Preset con nome ─────────────────────────────────────────────────────

#[tauri::command]
async fn list_named_presets(core: CoreState<'_>) -> CmdResult<Vec<NamedPreset>> {
    blocking(&core, |core| Ok(core.named_presets())).await
}

#[tauri::command]
async fn save_named_preset(core: CoreState<'_>, preset: NamedPreset) -> CmdResult<NamedPreset> {
    blocking(&core, move |core| core.save_named_preset(&preset)).await
}

#[tauri::command]
async fn delete_named_preset(core: CoreState<'_>, name: String) -> CmdResult<()> {
    blocking(&core, move |core| core.delete_named_preset(&name)).await
}

/// `path` arriva dalla finestra "Salva con nome" del sistema.
#[tauri::command]
async fn export_named_preset(core: CoreState<'_>, name: String, path: String) -> CmdResult<()> {
    blocking(&core, move |core| {
        core.export_named_preset(&name, std::path::Path::new(&path))
    })
    .await
}

/// `path` arriva dalla finestra "Apri" del sistema.
#[tauri::command]
async fn import_named_preset(core: CoreState<'_>, path: String) -> CmdResult<NamedPreset> {
    blocking(&core, move |core| {
        core.import_named_preset(std::path::Path::new(&path))
    })
    .await
}

// ── Download dei modelli ────────────────────────────────────────────────

#[tauri::command]
async fn search_hub(core: CoreState<'_>, query: String) -> CmdResult<Vec<HubRepo>> {
    core.search_hub(&query).await.map_err(text)
}

#[tauri::command]
async fn hub_files(core: CoreState<'_>, repo: String) -> CmdResult<Vec<HubFile>> {
    core.hub_files(&repo).await.map_err(text)
}

/// Avvia il download e torna subito: l'avanzamento si legge con `download_status`,
/// così prosegue anche se l'utente cambia pagina.
#[tauri::command]
fn start_model_download(core: CoreState<'_>, repo: String, file: HubFile, folder: String) {
    let core = core.inner().clone();
    tauri::async_runtime::spawn(async move {
        // L'esito, errori compresi, finisce nello stato del download.
        let _ = core.download_model(&repo, &file, &folder).await;
    });
}

#[tauri::command(async)]
fn security_settings(core: CoreState<'_>) -> SecuritySettings {
    core.security_settings()
}

#[tauri::command(async)]
fn set_security_settings(core: CoreState<'_>, settings: SecuritySettings) -> CmdResult<()> {
    core.set_security_settings(&settings).map_err(text)
}

#[tauri::command(async)]
fn language(core: CoreState<'_>) -> CmdResult<Option<String>> {
    core.language().map_err(text)
}

#[tauri::command(async)]
fn set_language(app: AppHandle, core: CoreState<'_>, language: String) -> CmdResult<()> {
    core.set_language(&language).map_err(text)?;
    // Il menu dell'icona nell'area di notifica segue la lingua scelta.
    if let Some(tray) = app.tray_by_id(TRAY_ID)
        && let Ok(menu) = tray_menu(&app, &core)
    {
        let _ = tray.set_menu(Some(menu));
    }
    Ok(())
}

#[tauri::command(async)]
fn preferences(core: CoreState<'_>) -> Preferences {
    core.preferences()
}

#[tauri::command(async)]
fn set_preferences(core: CoreState<'_>, preferences: Preferences) -> CmdResult<()> {
    core.set_preferences(&preferences).map_err(text)
}

/// `true` se chiudendo la finestra il nucleo resta attivo nell'area di notifica.
#[tauri::command]
fn runs_in_tray(lifecycle: State<'_, Arc<Lifecycle>>) -> bool {
    lifecycle.tray.load(Ordering::Relaxed)
}

/// Apre un collegamento nel browser di sistema. Solo indirizzi web e di
/// posta: mai file o programmi, anche se il collegamento viene dal testo di
/// un modello.
#[tauri::command]
fn open_external(app: tauri::AppHandle, url: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let parsed = tauri::Url::parse(url.trim()).map_err(|_| "Indirizzo non valido.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err(format!(
            "Si aprono solo indirizzi web: {} non è ammesso.",
            parsed.scheme()
        ));
    }
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(text)
}

/// La finestra mostra solo l'interfaccia dell'app: un collegamento che porta
/// altrove (per esempio dentro una risposta) non deve sostituirla con un sito
/// esterno. I collegamenti si aprono nel browser con [`open_external`].
fn is_app_page(url: &tauri::Url) -> bool {
    match (url.scheme(), url.host_str()) {
        ("tauri", _) | ("http" | "https", Some("tauri.localhost")) => true,
        // Il server di sviluppo di Vite.
        ("http", Some("localhost")) => cfg!(debug_assertions),
        _ => false,
    }
}

#[tauri::command]
fn list_downloads(core: CoreState<'_>) -> Vec<DownloadState> {
    core.downloads()
}

#[tauri::command]
fn cancel_download(core: CoreState<'_>, id: String) {
    core.cancel_download(&id);
}

#[tauri::command]
fn dismiss_download(core: CoreState<'_>, id: String) {
    core.dismiss_download(&id);
}

// ── Finestra e area di notifica ─────────────────────────────────────────

/// Mostra la finestra principale, creandola di nuovo se era stata chiusa.
fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .cloned()
    else {
        return;
    };
    // Su Windows una finestra creata dentro un callback del thread principale
    // (menu, icona, seconda copia dell'app) non parte: si crea da un altro thread.
    let app = app.clone();
    std::thread::spawn(move || {
        match WebviewWindowBuilder::from_config(&app, &config).and_then(|b| b.build()) {
            Ok(window) => {
                let _ = window.set_focus();
            }
            Err(e) => eprintln!("finestra non riaperta: {e}"),
        }
    });
}

/// Chiude davvero l'app: finestra, nucleo e modello.
fn quit(app: &AppHandle) {
    if let Some(lifecycle) = app.try_state::<Arc<Lifecycle>>() {
        lifecycle.quitting.store(true, Ordering::Relaxed);
    }
    app.exit(0);
}

/// Il menu dell'icona, nella lingua dell'interfaccia.
fn tray_menu(app: &AppHandle, core: &Core) -> tauri::Result<Menu<tauri::Wry>> {
    let english = match core.language().ok().flatten() {
        Some(lang) => lang == "en",
        None => !app_core::system_language_is_italian(),
    };
    let (open, exit) = if english {
        ("Open PersonAIly Hub", "Quit")
    } else {
        ("Apri PersonAIly Hub", "Esci")
    };
    let open = MenuItem::with_id(app, "open", open, true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "quit", exit, true, None::<&str>)?;
    Menu::with_items(app, &[&open, &exit])
}

/// L'icona nell'area di notifica: un clic riapre la finestra, il menu
/// permette di uscire davvero.
fn create_tray(app: &AppHandle, core: &Core) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("PersonAIly Hub")
        .menu(&tray_menu(app, core)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Annota dove si trova `app-server.exe`, installato accanto all'app: così
/// l'estensione VS Code può avviarlo da sola ad app chiusa.
fn record_server_path(data_dir: &std::path::Path) {
    let name = if cfg!(windows) {
        "app-server.exe"
    } else {
        "app-server"
    };
    if let Ok(exe) = std::env::current_exe() {
        let server = exe.with_file_name(name);
        if server.is_file() {
            let _ = app_core::instance::write_install_info(data_dir, &server);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // Per primo: una seconda copia dell'app non parte, riapre la finestra
        // di quella già accesa (che magari è solo nell'area di notifica).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("navigation-guard")
                .on_navigation(|_, url| is_app_page(url))
                .build(),
        )
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let core = Arc::new(Core::new(&data_dir)?);
            let api = Arc::new(LocalApi::default());
            let lifecycle = Arc::new(Lifecycle::default());
            record_server_path(&data_dir);
            spawn_local_api(core.clone(), data_dir, api.clone());
            tauri::async_runtime::spawn(core.clone().maintenance());
            // Senza icona la finestra resta l'unico modo di usare l'app:
            // chiuderla chiude tutto, come prima.
            let tray = create_tray(app.handle(), &core).is_ok();
            lifecycle.tray.store(tray, Ordering::Relaxed);
            app.manage(core);
            app.manage(api);
            app.manage(lifecycle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            core_info,
            engine_status,
            set_engine_variant,
            remove_engine,
            install_engine,
            model_folders,
            set_model_folders,
            list_models,
            get_preset,
            save_preset,
            check_load,
            load_model,
            unload_model,
            runtime_status,
            list_conversations,
            create_conversation,
            rename_conversation,
            delete_conversation,
            get_messages,
            send_message,
            resend_message,
            export_conversation,
            stop_generation,
            api_info,
            regenerate_api_token,
            granted_folders,
            set_granted_folders,
            tool_defaults,
            set_tool_defaults,
            conversation_tools,
            set_conversation_tools,
            answer_tool_request,
            list_actions,
            action_log_days,
            set_action_log_days,
            clear_actions,
            list_mcp_servers,
            save_mcp_server,
            delete_mcp_server,
            probe_mcp_server,
            list_agents,
            save_agent,
            duplicate_agent,
            delete_agent,
            start_agent_session,
            reply_agent_session,
            list_agent_sessions,
            get_agent_session,
            agent_session_live,
            pause_agent_session,
            resume_agent_session,
            stop_agent_session,
            delete_agent_session,
            list_providers,
            save_provider,
            delete_provider,
            refresh_provider_models,
            list_named_presets,
            save_named_preset,
            delete_named_preset,
            export_named_preset,
            import_named_preset,
            search_hub,
            hub_files,
            start_model_download,
            list_downloads,
            cancel_download,
            dismiss_download,
            security_settings,
            set_security_settings,
            language,
            set_language,
            preferences,
            set_preferences,
            runs_in_tray,
            open_external,
        ])
        .build(tauri::generate_context!())
        .expect("errore durante l'avvio dell'app");

    app.run(|handle, event| {
        // Chiusa l'ultima finestra il processo resterebbe senza motivo di
        // vivere: se l'utente lo preferisce, resta invece nell'area di notifica.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = &event
            && let (Some(core), Some(lifecycle)) = (
                handle.try_state::<Arc<Core>>(),
                handle.try_state::<Arc<Lifecycle>>(),
            )
            && lifecycle.tray.load(Ordering::Relaxed)
            && !lifecycle.quitting.load(Ordering::Relaxed)
            && core.preferences().keep_running
        {
            api.prevent_exit();
            return;
        }
        // Alla chiusura il modello va scaricato, altrimenti llama-server
        // resterebbe in esecuzione con la memoria occupata.
        if let RunEvent::Exit = event
            && let Some(core) = handle.try_state::<Arc<Core>>()
        {
            // Prima si toglie il file di blocco: i client non devono trovare
            // un nucleo che sta per sparire.
            if let Some(api) = handle.try_state::<Arc<LocalApi>>()
                && let Some(running) = tauri::async_runtime::block_on(api.handle.lock()).take()
            {
                tauri::async_runtime::block_on(running.stop());
            }
            // Gli agenti al lavoro si fermano: comandi in corso compresi.
            core.stop_all_sessions();
            tauri::async_runtime::block_on(core.unload_model());
        }
    });
}
