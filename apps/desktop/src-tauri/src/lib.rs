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
use app_core::mcp_oauth::{LoginStart, OAuthStatus};
use app_core::models::ModelInfo;
use app_core::named_presets::NamedPreset;
use app_core::providers::Provider;
use app_core::schedules::{Schedule, ScheduleEvent, ScheduleOutcome};
use app_core::skills::{ImportOutcome, Skill, SkillDraft};
use app_core::store::{ActionRecord, Conversation, Message};
use app_core::tools::ToolSettings;
use app_core::{
    Approval, ConversationSetup, Core, EngineStatus, LoadCheck, ModelRef, Preferences,
    ProviderInfo, RuntimeStatus, SecuritySettings, SendOutcome, SpendLimits, Spending,
};
use app_server::ApiHandle;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, State, WebviewWindowBuilder};

const MAIN_WINDOW: &str = "main";
/// Argomento con cui Windows avvia l'app all'accesso ("Avvia con Windows"):
/// parte nell'area di notifica, senza finestra (ADR 0028).
const TRAY_ARG: &str = "--tray";
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

/// Ogni errore mostrato all'interfaccia finisce anche nel registro
/// diagnostico, così una segnalazione dice che cosa è successo.
fn text(e: impl std::fmt::Display) -> String {
    let message = e.to_string();
    app_core::log::warn(&message);
    message
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
    replace: Option<bool>,
) -> CmdResult<LoadedModel> {
    core.load_model_with(
        &model_path,
        &params,
        app_core::LoadOptions {
            force,
            replace_others: replace.unwrap_or(false),
            make_primary: true,
        },
    )
    .await
    .map_err(text)
}

/// Scarica un modello, o tutti senza `model_path`.
#[tauri::command]
async fn unload_model(core: CoreState<'_>, model_path: Option<String>) -> CmdResult<()> {
    match model_path {
        Some(path) => core.unload_local(&path).await,
        None => core.unload_model().await,
    }
    Ok(())
}

#[tauri::command(async)]
fn set_primary_model(core: CoreState<'_>, model_path: String) -> CmdResult<()> {
    core.set_primary_model(&model_path).map_err(text)
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
fn create_temporary_conversation(core: CoreState<'_>) -> CmdResult<Conversation> {
    core.create_temporary_conversation().map_err(text)
}

#[tauri::command(async)]
fn keep_conversation(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.keep_conversation(id).map_err(text)
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
    /// Il modello cloud ha dato errore: risponde un modello di riserva.
    ModelSwitch {
        from: String,
        to: String,
        reason: String,
    },
}

impl From<ChatEvent> for StreamEvent {
    fn from(event: ChatEvent) -> Self {
        match event {
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
            ChatEvent::ModelSwitch { from, to, reason } => {
                StreamEvent::ModelSwitch { from, to, reason }
            }
        }
    }
}

/// Inoltra all'interfaccia gli eventi di una risposta in arrivo.
fn stream_to(channel: Channel<StreamEvent>) -> impl FnMut(ChatEvent) {
    move |event| {
        // Se la finestra è stata chiusa non c'è più nessuno ad ascoltare: la
        // risposta continua e finisce comunque nell'archivio.
        let _ = channel.send(StreamEvent::from(event));
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

/// Mostra il ramo della conversazione che passa per `message_id`.
#[tauri::command]
async fn switch_branch(
    core: CoreState<'_>,
    conversation_id: i64,
    message_id: i64,
) -> CmdResult<()> {
    blocking(&core, move |core| {
        core.switch_branch(conversation_id, message_id)
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

#[tauri::command(async)]
fn conversation_setup(core: CoreState<'_>, conversation_id: i64) -> CmdResult<ConversationSetup> {
    core.conversation_setup(conversation_id).map_err(text)
}

#[tauri::command(async)]
fn set_conversation_model(
    core: CoreState<'_>,
    conversation_id: i64,
    model: ModelRef,
) -> CmdResult<()> {
    core.set_conversation_model(conversation_id, &model)
        .map_err(text)
}

#[tauri::command(async)]
fn set_conversation_system_prompt(
    core: CoreState<'_>,
    conversation_id: i64,
    prompt: String,
) -> CmdResult<()> {
    core.set_conversation_system_prompt(conversation_id, &prompt)
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

// ── Plugin (ADR 0026) ───────────────────────────────────────────────────

#[tauri::command(async)]
fn list_plugins(core: CoreState<'_>) -> CmdResult<Vec<app_core::plugins::PluginRecord>> {
    core.list_plugins().map_err(text)
}

/// Importa un plugin da una cartella o da un `.zip`; senza `replace` non
/// sostituisce un plugin con lo stesso nome e lo dice.
#[tauri::command(async)]
fn import_plugin(
    core: CoreState<'_>,
    path: String,
    replace: bool,
) -> CmdResult<app_core::plugins::PluginInstall> {
    core.import_plugin(std::path::Path::new(&path), replace)
        .map_err(text)
}

#[tauri::command(async)]
fn uninstall_plugin(core: CoreState<'_>, name: String) -> CmdResult<()> {
    core.uninstall_plugin(&name).map_err(text)
}

#[tauri::command(async)]
fn plugin_commands(core: CoreState<'_>) -> CmdResult<Vec<app_core::PluginCommandInfo>> {
    core.plugin_commands().map_err(text)
}

#[tauri::command(async)]
fn plugin_command_text(
    core: CoreState<'_>,
    plugin: String,
    name: String,
    arguments: String,
) -> CmdResult<String> {
    core.plugin_command_text(&plugin, &name, &arguments)
        .map_err(text)
}

/// I prompt dei server MCP della conversazione, per i comandi `/` (ADR 0025).
#[tauri::command]
async fn conversation_prompts(
    core: CoreState<'_>,
    conversation_id: i64,
) -> CmdResult<Vec<app_core::mcp::McpPrompt>> {
    core.conversation_prompts(conversation_id)
        .await
        .map_err(text)
}

/// Il testo di un prompt, con gli argomenti scritti dall'utente.
#[tauri::command]
async fn conversation_prompt(
    core: CoreState<'_>,
    conversation_id: i64,
    server: usize,
    name: String,
    arguments: std::collections::HashMap<String, String>,
) -> CmdResult<String> {
    core.conversation_prompt(conversation_id, server, &name, &arguments)
        .await
        .map_err(text)
}

/// I valori suggeriti per un argomento di un prompt MCP, mentre lo si compila.
#[tauri::command]
async fn conversation_prompt_completion(
    core: CoreState<'_>,
    conversation_id: i64,
    server: usize,
    name: String,
    argument: String,
    value: String,
    context: std::collections::HashMap<String, String>,
) -> CmdResult<Vec<String>> {
    core.conversation_prompt_completion(conversation_id, server, &name, &argument, &value, &context)
        .await
        .map_err(text)
}

/// Prepara l'accesso OAuth a un server salvato: la pagina da aprire nel browser.
#[tauri::command]
async fn mcp_oauth_start(core: CoreState<'_>, id: String) -> CmdResult<LoginStart> {
    core.mcp_oauth_start(&id).await.map_err(text)
}

/// Aspetta che l'utente completi l'accesso nel browser.
#[tauri::command]
async fn mcp_oauth_finish(core: CoreState<'_>, id: String) -> CmdResult<OAuthStatus> {
    core.mcp_oauth_finish(&id).await.map_err(text)
}

#[tauri::command]
fn mcp_oauth_cancel(core: CoreState<'_>, id: String) {
    core.mcp_oauth_cancel(&id);
}

#[tauri::command(async)]
fn mcp_oauth_status(core: CoreState<'_>, id: String) -> CmdResult<OAuthStatus> {
    core.mcp_oauth_status(&id).map_err(text)
}

#[tauri::command(async)]
fn mcp_oauth_logout(core: CoreState<'_>, id: String) -> CmdResult<()> {
    core.mcp_oauth_logout(&id).map_err(text)
}

// ── Skill (ADR 0023) ────────────────────────────────────────────────────

#[tauri::command(async)]
fn list_skills(core: CoreState<'_>) -> CmdResult<Vec<Skill>> {
    core.list_skills().map_err(text)
}

/// Importa una cartella o un `.zip`; senza `replace` non sostituisce una skill
/// con lo stesso nome e lo dice.
#[tauri::command(async)]
fn import_skill(core: CoreState<'_>, path: String, replace: bool) -> CmdResult<ImportOutcome> {
    core.import_skill(std::path::Path::new(&path), replace)
        .map_err(text)
}

#[tauri::command(async)]
fn save_skill(
    core: CoreState<'_>,
    draft: SkillDraft,
    original: Option<String>,
) -> CmdResult<Skill> {
    core.save_skill(&draft, original.as_deref()).map_err(text)
}

#[tauri::command(async)]
fn set_skill_enabled(core: CoreState<'_>, name: String, enabled: bool) -> CmdResult<()> {
    core.set_skill_enabled(&name, enabled).map_err(text)
}

#[tauri::command(async)]
fn delete_skill(core: CoreState<'_>, name: String) -> CmdResult<()> {
    core.delete_skill(&name).map_err(text)
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

// ── Pianificazioni (ADR 0028) ───────────────────────────────────────────

#[tauri::command(async)]
fn list_schedules(core: CoreState<'_>) -> CmdResult<Vec<Schedule>> {
    core.schedules().map_err(text)
}

#[tauri::command(async)]
fn save_schedule(core: CoreState<'_>, schedule: Schedule) -> CmdResult<Schedule> {
    core.save_schedule(schedule).map_err(text)
}

#[tauri::command(async)]
fn delete_schedule(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.delete_schedule(id).map_err(text)
}

/// Esegue subito il compito, con le regole di un'esecuzione pianificata.
#[tauri::command]
async fn run_schedule_now(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.run_schedule_now(id).map_err(text)
}

/// L'app parte da sola all'accesso a Windows, nell'area di notifica.
#[tauri::command]
fn autostart_enabled(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> CmdResult<()> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(text)
}

/// Titolo e testo della notifica per l'esito di un'esecuzione pianificata.
fn schedule_notice(english: bool, event: &ScheduleEvent) -> (String, String) {
    let name = &event.agent_name;
    let title = match (event.outcome, english) {
        (ScheduleOutcome::Done, false) => format!("{name} ha finito"),
        (ScheduleOutcome::Done, true) => format!("{name} finished"),
        (ScheduleOutcome::Failed, false) => format!("{name}: lavoro non riuscito"),
        (ScheduleOutcome::Failed, true) => format!("{name} failed"),
        (ScheduleOutcome::Stopped, false) => format!("{name} si è fermato"),
        (ScheduleOutcome::Stopped, true) => format!("{name} stopped"),
        (ScheduleOutcome::Skipped, false) => format!("{name}: esecuzione saltata"),
        (ScheduleOutcome::Skipped, true) => format!("{name}: run skipped"),
    };
    let mut lines = Vec::new();
    if let Some(note) = &event.note {
        lines.push(if english {
            english_note(note)
        } else {
            note.clone()
        });
    }
    if !event.summary.is_empty() {
        lines.push(event.summary.clone());
    }
    if event.refused > 0 {
        lines.push(match (english, event.refused) {
            (false, 1) => "1 azione rifiutata: chiedeva una conferma.".to_string(),
            (false, n) => format!("{n} azioni rifiutate: chiedevano una conferma."),
            (true, 1) => "1 action refused: it needed confirmation.".to_string(),
            (true, n) => format!("{n} actions refused: they needed confirmation."),
        });
    }
    (title, lines.join("\n"))
}

/// Le note fisse del nucleo in inglese; le altre (errori dei servizi) restano come sono.
fn english_note(note: &str) -> String {
    const NOTES: &[(&str, &str)] = &[
        (
            "Esecuzione persa:",
            "Run missed: the app was closed or the PC off at the scheduled time.",
        ),
        (
            "Il lavoro della volta precedente",
            "The previous run had not finished yet.",
        ),
        (
            "Era caricato un altro modello",
            "Another model was loaded and nobody unloaded it within an hour.",
        ),
        (
            "Il modello locale è rimasto occupato",
            "The local model stayed busy for an hour.",
        ),
        ("Fermata dopo ", "Stopped at the maximum duration."),
    ];
    NOTES
        .iter()
        .find(|(it, _)| note.starts_with(it))
        .map(|(_, en)| en.to_string())
        .unwrap_or_else(|| note.to_string())
}

/// Mostra una notifica di Windows per ogni esecuzione pianificata conclusa o
/// saltata, e avvisa l'interfaccia (se è aperta) di rileggere.
fn spawn_schedule_notifications(app: AppHandle, core: Arc<Core>) {
    use tauri::Emitter;
    use tauri_plugin_notification::NotificationExt;
    let mut events = core.schedule_events();
    tauri::async_runtime::spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            if event.notify {
                let english = matches!(core.language().ok().flatten().as_deref(), Some("en"));
                let (title, body) = schedule_notice(english, &event);
                if let Err(e) = app.notification().builder().title(title).body(body).show() {
                    app_core::log::warn(format!("notifica non mostrata: {e}"));
                }
            }
            let _ = app.emit("schedule-finished", event.schedule_id);
        }
    });
}

/// Riporta le cartelle dell'agente a prima della sessione.
#[tauri::command(async)]
fn restore_agent_session(core: CoreState<'_>, id: i64) -> CmdResult<AgentSession> {
    core.restore_session(id).map_err(text)
}

#[tauri::command(async)]
fn restore_points_available(core: CoreState<'_>) -> bool {
    core.restore_points_available()
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

/// Avvia l'API locale dentro l'app. `replaced` è l'esito della richiesta di
/// chiusura al nucleo senza interfaccia, fatta prima di aprire il nucleo
/// dell'app: se non si è chiuso, l'API resta la sua.
fn spawn_local_api(
    core: Arc<Core>,
    data_dir: std::path::PathBuf,
    api: Arc<LocalApi>,
    replaced: Result<(), String>,
) {
    tauri::async_runtime::spawn(async move {
        let result = match replaced {
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

// ── Documenti consultabili (RAG) ────────────────────────────────────────

#[tauri::command(async)]
fn rag_status(core: CoreState<'_>) -> CmdResult<app_core::rag::RagStatus> {
    core.rag_status().map_err(text)
}

#[tauri::command(async)]
fn set_rag_settings(core: CoreState<'_>, settings: app_core::rag::RagSettings) -> CmdResult<()> {
    core.set_rag_settings(&settings).map_err(text)
}

/// Avvia l'indicizzazione e torna subito: l'avanzamento si legge con `rag_status`.
#[tauri::command]
fn index_documents(core: CoreState<'_>) {
    let core = core.inner().clone();
    tauri::async_runtime::spawn(async move {
        // L'esito, errori compresi, finisce nello stato dell'indice.
        let _ = core.index_documents().await;
    });
}

/// Una ricerca di prova dalle Impostazioni: i brani e il loro punteggio.
#[tauri::command]
async fn search_documents(
    core: CoreState<'_>,
    query: String,
) -> CmdResult<Vec<(String, String, f32)>> {
    let hits = core.search_documents(&query).await.map_err(text)?;
    Ok(hits
        .into_iter()
        .map(|h| (h.path, h.text, h.score))
        .collect())
}

#[tauri::command(async)]
fn security_settings(core: CoreState<'_>) -> SecuritySettings {
    core.security_settings()
}

#[tauri::command(async)]
fn set_security_settings(core: CoreState<'_>, settings: SecuritySettings) -> CmdResult<()> {
    core.set_security_settings(&settings).map_err(text)
}

/// Spesa stimata del mese e limiti per i modelli cloud.
#[tauri::command(async)]
fn spending(core: CoreState<'_>) -> CmdResult<Spending> {
    core.spending().map_err(text)
}

#[tauri::command(async)]
fn set_spend_limits(core: CoreState<'_>, limits: SpendLimits) -> CmdResult<()> {
    core.set_spend_limits(&limits).map_err(text)
}

/// La coda dei modelli cloud di riserva, in ordine.
#[tauri::command]
fn fallback_models(core: CoreState<'_>) -> Vec<ModelRef> {
    core.fallback_models()
}

#[tauri::command]
fn set_fallback_models(core: CoreState<'_>, models: Vec<ModelRef>) -> CmdResult<()> {
    core.set_fallback_models(&models).map_err(text)
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
    let parsed = external_url(&url)?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(text)
}

/// Un indirizzo che si può aprire nel browser: web o posta, nient'altro.
fn external_url(url: &str) -> CmdResult<tauri::Url> {
    let parsed = tauri::Url::parse(url.trim()).map_err(|_| "Indirizzo non valido.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err(format!(
            "Si aprono solo indirizzi web: {} non è ammesso.",
            parsed.scheme()
        ));
    }
    Ok(parsed)
}

/// L'aggiornamento trovato dall'ultimo controllo, in attesa di installazione.
#[derive(Default)]
struct Updates {
    pending: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
}

/// Una versione nuova dell'app.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    version: String,
    current_version: String,
    /// Note di rilascio, in Markdown.
    notes: Option<String>,
    /// Data di pubblicazione, `2026-11-20`.
    date: Option<String>,
    /// `false` per l'exe portatile: lì non c'è un installatore da avviare,
    /// la versione nuova si scarica a mano.
    installable: bool,
}

/// L'app è stata installata con `setup.exe` (accanto all'eseguibile c'è il
/// programma di disinstallazione), non è l'exe portatile.
fn installed_with_setup() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("uninstall.exe").is_file()))
        .unwrap_or(false)
}

/// Chiede a GitHub se c'è una versione più recente di questa.
#[tauri::command]
async fn check_update(
    app: tauri::AppHandle,
    updates: State<'_, Arc<Updates>>,
) -> CmdResult<Option<UpdateInfo>> {
    use tauri_plugin_updater::UpdaterExt;
    let failed = |e: tauri_plugin_updater::Error| {
        text(format!("Controllo degli aggiornamenti non riuscito: {e}"))
    };
    let update = app
        .updater()
        .map_err(failed)?
        .check()
        .await
        .map_err(failed)?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.date().to_string()),
        installable: installed_with_setup(),
    });
    if let Some(info) = &info {
        app_core::log::info(format!("disponibile la versione {}", info.version));
    }
    *updates.pending.lock().await = update;
    Ok(info)
}

/// Avanzamento del download di un aggiornamento.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    done_bytes: u64,
    total_bytes: Option<u64>,
}

/// Scarica l'aggiornamento trovato da [`check_update`], ne controlla la firma
/// e avvia l'installatore. Prima ferma tutto come alla chiusura dell'app:
/// l'installatore la chiude e la riapre aggiornata.
#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    core: CoreState<'_>,
    api: ApiState<'_>,
    updates: State<'_, Arc<Updates>>,
    on_progress: Channel<UpdateProgress>,
) -> CmdResult<()> {
    let Some(update) = updates.pending.lock().await.take() else {
        return Err(text(
            "Nessun aggiornamento da installare: controlla di nuovo.",
        ));
    };
    if !installed_with_setup() {
        return Err(text(
            "Questa è la versione portatile: scarica la nuova dalla pagina delle release.",
        ));
    }
    let mut done = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                let _ = on_progress.send(UpdateProgress {
                    done_bytes: done,
                    total_bytes: total,
                });
            },
            || {},
        )
        .await
        .map_err(|e| text(format!("Download dell'aggiornamento non riuscito: {e}")))?;
    app_core::log::info(format!("installazione della versione {}", update.version));
    if let Some(lifecycle) = app.try_state::<Arc<Lifecycle>>() {
        lifecycle.quitting.store(true, Ordering::Relaxed);
    }
    // Come alla chiusura: via il file di blocco, fermi gli agenti, libera
    // la memoria del modello.
    if let Some(running) = api.handle.lock().await.take() {
        running.stop().await;
    }
    core.stop_all_sessions();
    core.unload_model().await;
    update.install(bytes).map_err(|e| {
        text(format!(
            "Installazione dell'aggiornamento non riuscita: {e}"
        ))
    })
}

/// Salva un backup dei dati nel file scelto dall'utente.
#[tauri::command(async)]
fn create_backup(core: CoreState<'_>, path: String) -> CmdResult<app_core::backup::BackupInfo> {
    core.create_backup(std::path::Path::new(&path))
        .map_err(text)
}

/// Prepara il ripristino di un backup e riavvia l'app, che lo applica
/// all'avvio, prima di aprire l'archivio.
#[tauri::command(async)]
fn restore_backup(app: tauri::AppHandle, core: CoreState<'_>, path: String) -> CmdResult<()> {
    core.stage_restore(std::path::Path::new(&path))
        .map_err(text)?;
    if let Some(lifecycle) = app.try_state::<Arc<Lifecycle>>() {
        lifecycle.quitting.store(true, Ordering::Relaxed);
    }
    app.request_restart();
    Ok(())
}

/// Informazioni da allegare a una segnalazione (vedi [`Core::diagnostics`]).
#[tauri::command(async)]
fn diagnostics(core: CoreState<'_>) -> String {
    core.diagnostics()
}

/// Apre in Esplora file la cartella dei registri: sempre quella, mai un
/// percorso scelto dall'interfaccia.
#[tauri::command]
fn open_logs_folder(app: tauri::AppHandle, core: CoreState<'_>) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = core.logs_dir();
    std::fs::create_dir_all(&dir).map_err(text)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
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
    let (open, exit) = tray_labels(english);
    let open = MenuItem::with_id(app, "open", open, true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "quit", exit, true, None::<&str>)?;
    Menu::with_items(app, &[&open, &exit])
}

/// Le voci del menu dell'area di notifica, nella lingua dell'interfaccia.
fn tray_labels(english: bool) -> (&'static str, &'static str) {
    if english {
        ("Open PersonAIly Hub", "Quit")
    } else {
        ("Apri PersonAIly Hub", "Esci")
    }
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![TRAY_ARG]),
        ))
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("navigation-guard")
                .on_navigation(|_, url| is_app_page(url))
                .build(),
        )
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            app_core::log::init(&data_dir, "app");
            app_core::log::install_panic_hook();
            // Il nucleo senza interfaccia (avviato da VS Code) va chiuso prima
            // di aprire il nucleo dell'app: `Core::new` applica un ripristino in
            // sospeso, segna come interrotte le sessioni e toglie le conversazioni
            // temporanee, e lo farebbe sotto i piedi di un nucleo ancora al lavoro.
            let replaced = tauri::async_runtime::block_on(app_server::replace_server(&data_dir));
            let core = Arc::new(Core::new(&data_dir).inspect_err(|e| {
                app_core::log::error(format!("avvio del nucleo non riuscito: {e}"));
            })?);
            let api = Arc::new(LocalApi::default());
            let lifecycle = Arc::new(Lifecycle::default());
            record_server_path(&data_dir);
            spawn_local_api(core.clone(), data_dir, api.clone(), replaced);
            tauri::async_runtime::spawn(core.clone().maintenance());
            // Senza icona la finestra resta l'unico modo di usare l'app:
            // chiuderla chiude tutto, come prima.
            let tray = create_tray(app.handle(), &core).is_ok();
            lifecycle.tray.store(tray, Ordering::Relaxed);
            spawn_schedule_notifications(app.handle().clone(), core.clone());
            // Avviata da Windows all'accesso: niente finestra, solo l'icona.
            // Senza icona, o se chiudere la finestra chiude tutto, la finestra resta.
            let hidden =
                tray && core.preferences().keep_running && std::env::args().any(|a| a == TRAY_ARG);
            app.manage(core);
            app.manage(api);
            app.manage(lifecycle);
            app.manage(Arc::new(Updates::default()));
            if hidden && let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                app_core::log::info("avvio nell'area di notifica, senza finestra");
                let _ = window.destroy();
            }
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
            create_temporary_conversation,
            keep_conversation,
            rename_conversation,
            delete_conversation,
            get_messages,
            send_message,
            resend_message,
            export_conversation,
            switch_branch,
            stop_generation,
            api_info,
            regenerate_api_token,
            granted_folders,
            set_granted_folders,
            tool_defaults,
            set_tool_defaults,
            conversation_tools,
            set_conversation_tools,
            conversation_setup,
            set_conversation_model,
            set_conversation_system_prompt,
            answer_tool_request,
            list_actions,
            action_log_days,
            set_action_log_days,
            clear_actions,
            list_mcp_servers,
            save_mcp_server,
            delete_mcp_server,
            probe_mcp_server,
            conversation_prompts,
            conversation_prompt,
            conversation_prompt_completion,
            list_plugins,
            import_plugin,
            uninstall_plugin,
            plugin_commands,
            plugin_command_text,
            mcp_oauth_start,
            mcp_oauth_finish,
            mcp_oauth_cancel,
            mcp_oauth_status,
            mcp_oauth_logout,
            list_skills,
            import_skill,
            save_skill,
            set_skill_enabled,
            delete_skill,
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
            restore_agent_session,
            set_primary_model,
            list_schedules,
            save_schedule,
            delete_schedule,
            run_schedule_now,
            autostart_enabled,
            set_autostart,
            restore_points_available,
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
            spending,
            rag_status,
            set_rag_settings,
            index_documents,
            search_documents,
            set_spend_limits,
            fallback_models,
            set_fallback_models,
            language,
            set_language,
            preferences,
            set_preferences,
            runs_in_tray,
            open_external,
            diagnostics,
            open_logs_folder,
            create_backup,
            restore_backup,
            check_update,
            install_update,
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
            app_core::log::info("chiusura dell'app");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn si_aprono_solo_indirizzi_web_e_di_posta() {
        assert!(external_url(" https://esempio.it/pagina ").is_ok());
        assert!(external_url("http://esempio.it").is_ok());
        assert!(external_url("mailto:qualcuno@esempio.it").is_ok());
        for refused in [
            "file:///C:/Windows/System32/cmd.exe",
            "javascript:alert(1)",
            "ms-settings:privacy",
            "C:\\Windows\\notepad.exe",
            "non è un indirizzo",
        ] {
            assert!(external_url(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn la_finestra_mostra_solo_le_pagine_dell_app() {
        let url = |u: &str| tauri::Url::parse(u).unwrap();
        assert!(is_app_page(&url("tauri://localhost/index.html")));
        assert!(is_app_page(&url("http://tauri.localhost/")));
        assert!(is_app_page(&url("https://tauri.localhost/assets/a.js")));
        assert!(!is_app_page(&url("https://esempio.it/")));
        assert!(!is_app_page(&url("https://tauri.localhost.esempio.it/")));
        assert!(!is_app_page(&url("file:///C:/Users/")));
        // Il server di Vite solo durante lo sviluppo.
        assert_eq!(
            is_app_page(&url("http://localhost:1420/")),
            cfg!(debug_assertions)
        );
    }

    #[test]
    fn gli_eventi_arrivano_all_interfaccia_nella_forma_attesa() {
        let json = |event: ChatEvent| serde_json::to_value(StreamEvent::from(event)).unwrap();
        assert_eq!(
            json(ChatEvent::Text("ciao".into())),
            serde_json::json!({"type": "text", "text": "ciao"})
        );
        assert_eq!(
            json(ChatEvent::Reasoning("penso".into())),
            serde_json::json!({"type": "reasoning", "text": "penso"})
        );
        assert_eq!(json(ChatEvent::Saved), serde_json::json!({"type": "saved"}));
        assert_eq!(
            json(ChatEvent::ToolRequest {
                id: "c1:x".into(),
                name: "write_file".into(),
                summary: "Scrivere".into(),
                arguments: serde_json::json!({"path": "a.txt"}),
            }),
            serde_json::json!({"type": "toolRequest", "id": "c1:x", "name": "write_file",
                               "summary": "Scrivere", "arguments": {"path": "a.txt"}})
        );
        assert_eq!(
            json(ChatEvent::ModelSwitch {
                from: "gemini-pro".into(),
                to: "gemini-flash".into(),
                reason: "occupato".into(),
            }),
            serde_json::json!({"type": "modelSwitch", "from": "gemini-pro",
                               "to": "gemini-flash", "reason": "occupato"})
        );
    }

    #[test]
    fn la_notifica_di_un_esecuzione_pianificata() {
        let event = ScheduleEvent {
            schedule_id: 1,
            agent_name: "Notturno".into(),
            outcome: ScheduleOutcome::Done,
            note: None,
            session_id: Some(3),
            summary: "Ho riassunto 4 file.".into(),
            refused: 2,
            notify: true,
        };
        assert_eq!(
            schedule_notice(false, &event),
            (
                "Notturno ha finito".to_string(),
                "Ho riassunto 4 file.\n2 azioni rifiutate: chiedevano una conferma.".to_string()
            )
        );
        let skipped = ScheduleEvent {
            outcome: ScheduleOutcome::Skipped,
            note: Some(
                "Esecuzione persa: all'ora stabilita l'app era chiusa o il PC spento.".into(),
            ),
            summary: String::new(),
            refused: 0,
            ..event
        };
        let (title, body) = schedule_notice(true, &skipped);
        assert_eq!(title, "Notturno: run skipped");
        assert!(body.starts_with("Run missed"), "{body}");
    }

    #[test]
    fn il_menu_dell_area_di_notifica_segue_la_lingua() {
        assert_eq!(tray_labels(true), ("Open PersonAIly Hub", "Quit"));
        assert_eq!(tray_labels(false), ("Apri PersonAIly Hub", "Esci"));
    }

    /// Ogni comando che l'interfaccia chiama (`invoke("nome")` in `api.ts`)
    /// deve essere registrato qui: altrimenti fallisce solo quando l'utente
    /// tocca il pulsante.
    #[test]
    fn i_comandi_dell_interfaccia_esistono() {
        let source = include_str!("lib.rs");
        let start = source
            .find("generate_handler![")
            .expect("elenco dei comandi");
        let end = start + source[start..].find(']').unwrap();
        let registered: Vec<&str> = source[start + "generate_handler![".len()..end]
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect();
        let api = include_str!("../../src/api.ts");
        let mut called = Vec::new();
        for (i, _) in api.match_indices("invoke<") {
            let rest = &api[i..];
            let Some(open) = rest.find("(\"") else {
                continue;
            };
            let name: String = rest[open + 2..].chars().take_while(|&c| c != '"').collect();
            called.push(name);
        }
        assert!(
            called.len() > 50,
            "trovati solo {} comandi in api.ts",
            called.len()
        );
        let missing: Vec<&String> = called
            .iter()
            .filter(|name| !registered.contains(&name.as_str()))
            .collect();
        assert!(missing.is_empty(), "comandi non registrati: {missing:?}");
    }
}
