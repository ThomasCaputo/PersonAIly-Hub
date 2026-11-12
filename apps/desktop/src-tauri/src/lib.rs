//! Guscio dell'app desktop: crea la finestra e inoltra i comandi al nucleo.
//! Qui non va logica propria: sta tutta in `app-core`.

use std::sync::Arc;

use app_core::DownloadState;
use app_core::chat::ChatEvent;
use app_core::hub::{HubFile, HubRepo};
use app_core::local::install::{Phase, Progress, Variant};
use app_core::local::runtime::LoadedModel;
use app_core::local::{LoadParams, Preset, SamplingParams};
use app_core::models::ModelInfo;
use app_core::named_presets::NamedPreset;
use app_core::providers::Provider;
use app_core::store::{Conversation, Message};
use app_core::{Core, EngineStatus, LoadCheck, ModelRef, ProviderInfo, RuntimeStatus, SendOutcome};
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
    Text { text: String },
    Reasoning { text: String },
}

#[tauri::command]
async fn send_message(
    core: CoreState<'_>,
    conversation_id: i64,
    text: String,
    sampling: SamplingParams,
    system_prompt: String,
    model: ModelRef,
    on_event: Channel<StreamEvent>,
) -> CmdResult<SendOutcome> {
    let on_chunk = |event| {
        let event = match event {
            ChatEvent::Text(text) => StreamEvent::Text { text },
            ChatEvent::Reasoning(text) => StreamEvent::Reasoning { text },
        };
        // Se la finestra è stata chiusa non c'è più nessuno ad ascoltare.
        let _ = on_event.send(event);
    };
    core.send_message(
        conversation_id,
        &text,
        &sampling,
        &system_prompt,
        &model,
        on_chunk,
    )
    .await
    .map_err(self::text)
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
            tauri::async_runtime::block_on(core.unload_model());
        }
    });
}
