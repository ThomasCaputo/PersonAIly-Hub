//! Guscio dell'app desktop: crea la finestra e inoltra i comandi al nucleo.
//! Qui non va logica propria: sta tutta in `app-core`.

use std::sync::Arc;

use app_core::DownloadState;
use app_core::agents::{Agent, AgentSession, LiveState};
use app_core::chat::ChatEvent;
use app_core::hub::{HubFile, HubRepo};
use app_core::local::install::{Phase, Progress, Variant};
use app_core::local::runtime::LoadedModel;
use app_core::local::{LoadParams, Preset, SamplingParams};
use app_core::mcp::{McpServerConfig, McpToolInfo};
use app_core::models::ModelInfo;
use app_core::named_presets::NamedPreset;
use app_core::providers::Provider;
use app_core::store::{ActionRecord, Conversation, Message};
use app_core::tools::ToolSettings;
use app_core::{
    Approval, Core, EngineStatus, LoadCheck, ModelRef, ProviderInfo, RuntimeStatus, SendOutcome,
};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, RunEvent, State};

type CoreState<'a> = State<'a, Arc<Core>>;
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

#[tauri::command]
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

#[tauri::command]
fn model_folders(core: CoreState<'_>) -> CmdResult<Vec<String>> {
    core.model_folders().map_err(text)
}

#[tauri::command]
fn set_model_folders(core: CoreState<'_>, folders: Vec<String>) -> CmdResult<()> {
    core.set_model_folders(&folders).map_err(text)
}

#[tauri::command]
async fn list_models(core: CoreState<'_>) -> CmdResult<Vec<ModelInfo>> {
    blocking(&core, |core| core.list_models()).await
}

#[tauri::command]
fn get_preset(core: CoreState<'_>, model_path: String) -> CmdResult<Preset> {
    core.preset(&model_path).map_err(text)
}

#[tauri::command]
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

#[tauri::command]
fn list_conversations(core: CoreState<'_>, query: String) -> CmdResult<Vec<Conversation>> {
    core.list_conversations(&query).map_err(text)
}

#[tauri::command]
fn create_conversation(core: CoreState<'_>) -> CmdResult<Conversation> {
    core.create_conversation().map_err(text)
}

#[tauri::command]
fn rename_conversation(core: CoreState<'_>, id: i64, title: String) -> CmdResult<()> {
    core.rename_conversation(id, &title).map_err(text)
}

#[tauri::command]
fn delete_conversation(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.delete_conversation(id).map_err(text)
}

#[tauri::command]
fn get_messages(core: CoreState<'_>, conversation_id: i64) -> CmdResult<Vec<Message>> {
    core.messages(conversation_id).map_err(text)
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
    let on_chunk = |event| {
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
        // Se la finestra è stata chiusa non c'è più nessuno ad ascoltare.
        let _ = on_event.send(event);
    };
    core.send_message(
        conversation_id,
        &text,
        &attachments,
        &sampling,
        &system_prompt,
        &model,
        on_chunk,
    )
    .await
    .map_err(self::text)
}

// ── Strumenti e permessi ────────────────────────────────────────────────

#[tauri::command]
fn granted_folders(core: CoreState<'_>) -> CmdResult<Vec<String>> {
    core.granted_folders().map_err(text)
}

/// Le cartelle arrivano dalla finestra `"Scegli cartella`" del sistema.
#[tauri::command]
fn set_granted_folders(core: CoreState<'_>, folders: Vec<String>) -> CmdResult<()> {
    core.set_granted_folders(&folders).map_err(text)
}

#[tauri::command]
fn tool_defaults(core: CoreState<'_>) -> CmdResult<ToolSettings> {
    core.tool_defaults().map_err(text)
}

#[tauri::command]
fn set_tool_defaults(core: CoreState<'_>, settings: ToolSettings) -> CmdResult<()> {
    core.set_tool_defaults(&settings).map_err(text)
}

#[tauri::command]
fn conversation_tools(core: CoreState<'_>, conversation_id: i64) -> CmdResult<ToolSettings> {
    core.conversation_tools(conversation_id).map_err(text)
}

#[tauri::command]
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

#[tauri::command]
fn list_actions(
    core: CoreState<'_>,
    limit: u32,
    session_id: Option<i64>,
) -> CmdResult<Vec<ActionRecord>> {
    core.actions(limit, session_id).map_err(text)
}

// ── Server MCP ──────────────────────────────────────────────────────────

#[tauri::command]
fn list_mcp_servers(core: CoreState<'_>) -> CmdResult<Vec<McpServerConfig>> {
    core.mcp_servers().map_err(text)
}

#[tauri::command]
fn save_mcp_server(core: CoreState<'_>, server: McpServerConfig) -> CmdResult<McpServerConfig> {
    core.save_mcp_server(server).map_err(text)
}

#[tauri::command]
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

#[tauri::command]
fn list_agents(core: CoreState<'_>) -> CmdResult<Vec<Agent>> {
    core.agents().map_err(text)
}

#[tauri::command]
fn save_agent(core: CoreState<'_>, agent: Agent) -> CmdResult<Agent> {
    core.save_agent(agent).map_err(text)
}

#[tauri::command]
fn duplicate_agent(core: CoreState<'_>, id: i64) -> CmdResult<Agent> {
    core.duplicate_agent(id).map_err(text)
}

#[tauri::command]
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

#[tauri::command]
fn start_agent_session(
    core: CoreState<'_>,
    agent_id: i64,
    task: String,
) -> CmdResult<AgentSession> {
    let session = core.create_session(agent_id, &task).map_err(text)?;
    spawn_session(core.inner(), session.id);
    Ok(session)
}

#[tauri::command]
fn reply_agent_session(core: CoreState<'_>, id: i64, text: String) -> CmdResult<AgentSession> {
    let session = core.reply_session(id, &text).map_err(self::text)?;
    spawn_session(core.inner(), id);
    Ok(session)
}

#[tauri::command]
fn list_agent_sessions(core: CoreState<'_>) -> CmdResult<Vec<AgentSession>> {
    core.sessions().map_err(text)
}

#[tauri::command]
fn get_agent_session(core: CoreState<'_>, id: i64) -> CmdResult<AgentSession> {
    core.session(id).map_err(text)
}

#[tauri::command]
fn agent_session_live(core: CoreState<'_>, id: i64) -> Option<LiveState> {
    core.session_live(id)
}

#[tauri::command]
fn pause_agent_session(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.pause_session(id).map_err(text)
}

#[tauri::command]
fn resume_agent_session(core: CoreState<'_>, id: i64) -> CmdResult<()> {
    core.resume_session(id).map_err(text)
}

#[tauri::command]
fn stop_agent_session(core: CoreState<'_>, id: i64) {
    core.stop_session(id);
}

#[tauri::command]
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

#[tauri::command]
fn stop_generation(core: CoreState<'_>) {
    core.stop_generation();
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

#[tauri::command]
fn download_status(core: CoreState<'_>) -> Option<DownloadState> {
    core.download_status()
}

#[tauri::command]
fn cancel_download(core: CoreState<'_>) {
    core.cancel_download();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let core = Core::new(&app.path().app_data_dir()?)?;
            app.manage(Arc::new(core));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            core_info,
            engine_status,
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
            stop_generation,
            granted_folders,
            set_granted_folders,
            tool_defaults,
            set_tool_defaults,
            conversation_tools,
            set_conversation_tools,
            answer_tool_request,
            list_actions,
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
            download_status,
            cancel_download,
        ])
        .build(tauri::generate_context!())
        .expect("errore durante l'avvio dell'app");

    app.run(|handle, event| {
        // Alla chiusura il modello va scaricato, altrimenti llama-server
        // resterebbe in esecuzione con la memoria occupata.
        if let RunEvent::Exit = event
            && let Some(core) = handle.try_state::<Arc<Core>>()
        {
            // Gli agenti al lavoro si fermano: comandi in corso compresi.
            core.stop_all_sessions();
            tauri::async_runtime::block_on(core.unload_model());
        }
    });
}
