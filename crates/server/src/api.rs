//! Endpoint `/api/...`: l'API propria dell'app, usata dall'estensione VS Code.
//!
//! Le risposte in streaming arrivano come Server-Sent Events con un nome:
//! `text`, `reasoning`, `saved`, `toolRequest`, e alla fine `done` (l'esito)
//! oppure `error`.

use std::sync::Arc;

use app_core::agents::{Agent, AgentSession, LiveState};
use app_core::chat::ChatEvent;
use app_core::instance::InstanceKind;
use app_core::local::SamplingParams;
use app_core::local::runtime::LoadedModel;
use app_core::models::ModelInfo;
use app_core::schedules::Schedule;
use app_core::store::{ActionRecord, Conversation, Message};
use app_core::tools::ToolSettings;
use app_core::{Approval, Core, ModelRef, ProviderInfo, RuntimeStatus};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;

use crate::{ApiError, ApiResult, ApiState, Outgoing, on_disconnect, sse_json, sse_stream};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/info", get(info))
        .route("/api/shutdown", post(shutdown))
        .route("/api/status", get(status))
        .route("/api/models", get(models))
        .route("/api/local/load", post(load_model))
        .route("/api/local/unload", post(unload_model))
        .route("/api/local/primary", post(set_primary_model))
        .route("/api/infill", post(infill))
        .route(
            "/api/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/api/conversations/{id}",
            axum::routing::patch(rename_conversation).delete(delete_conversation),
        )
        .route(
            "/api/conversations/{id}/messages",
            get(messages).post(send_message),
        )
        .route("/api/conversations/{id}/stop", post(stop_conversation))
        .route("/api/conversations/{id}/branch", post(switch_branch))
        .route(
            "/api/conversations/{id}/tools",
            get(conversation_tools).put(set_conversation_tools),
        )
        .route("/api/tool-requests", post(answer_tool_request))
        .route("/api/actions", get(actions))
        .route("/api/agents", get(agents))
        .route("/api/agents/{id}/sessions", post(start_session))
        .route("/api/sessions", get(sessions))
        .route("/api/sessions/{id}", get(session).delete(delete_session))
        .route("/api/sessions/{id}/live", get(session_live))
        .route("/api/sessions/{id}/restore", post(restore_session))
        .route("/api/sessions/{id}/pause", post(pause_session))
        .route("/api/sessions/{id}/resume", post(resume_session))
        .route("/api/sessions/{id}/stop", post(stop_session))
        .route("/api/sessions/{id}/reply", post(reply_session))
        .route("/api/schedules", get(schedules).post(save_schedule))
        .route(
            "/api/schedules/{id}",
            axum::routing::put(update_schedule).delete(delete_schedule),
        )
        .route("/api/schedules/{id}/run", post(run_schedule))
}

/// Esegue fuori dal ciclo asincrono un'operazione che legge il disco o il portachiavi.
pub(crate) async fn blocking<T: Send + 'static>(
    core: &Arc<Core>,
    f: impl FnOnce(&Core) -> app_core::error::Result<T> + Send + 'static,
) -> ApiResult<T> {
    let core = core.clone();
    tokio::task::spawn_blocking(move || f(&core))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(ApiError::from)
}

// ── Nucleo ──────────────────────────────────────────────────────────────

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Info {
    version: &'static str,
    kind: InstanceKind,
    pid: u32,
}

async fn info(State(state): State<ApiState>) -> Json<Info> {
    Json(Info {
        version: app_core::version(),
        kind: state.shared.kind,
        pid: std::process::id(),
    })
}

/// Chiude il nucleo senza interfaccia. L'app desktop invece resta aperta:
/// la chiude solo chi la usa.
async fn shutdown(State(state): State<ApiState>) -> ApiResult<StatusCode> {
    if state.shared.kind != InstanceKind::Server {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "Il nucleo è quello dell'app desktop: si chiude chiudendo l'app.",
        ));
    }
    state.shared.quit.notify_one();
    Ok(StatusCode::ACCEPTED)
}

async fn status(State(state): State<ApiState>) -> Json<RuntimeStatus> {
    Json(state.core.status().await)
}

// ── Modelli ─────────────────────────────────────────────────────────────

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalModels {
    loaded: Option<LoadedModel>,
    loading: bool,
    library: Vec<ModelInfo>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Models {
    local: LocalModels,
    cloud: Vec<ProviderInfo>,
}

/// Tutti i modelli utilizzabili: quello locale caricato, la libreria e i
/// provider cloud con i loro modelli.
async fn models(State(state): State<ApiState>) -> ApiResult<Json<Models>> {
    let status = state.core.status().await;
    let (library, cloud) = blocking(&state.core, |core| {
        Ok((core.list_models()?, core.providers()?))
    })
    .await?;
    Ok(Json(Models {
        local: LocalModels {
            loaded: status.loaded,
            loading: status.loading,
            library: library.into_iter().filter(|m| m.error.is_none()).collect(),
        },
        cloud,
    }))
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoadBody {
    path: String,
    /// Carica anche se la stima dice che non entra in memoria.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    force: bool,
    /// Scarica prima gli altri modelli, anche con "Più modelli locali insieme"
    /// acceso (ADR 0029). Spento, il modello va accanto agli altri se entra.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    replace: bool,
}

/// Un modello locale caricato, indicato dal suo file.
#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PathBody {
    path: String,
}

/// Carica un modello della libreria con i parametri del suo preset.
async fn load_model(
    State(state): State<ApiState>,
    Json(body): Json<LoadBody>,
) -> ApiResult<Json<LoadedModel>> {
    let path = body.path.clone();
    let known = blocking(&state.core, move |core| {
        Ok(core.list_models()?.iter().any(|m| m.path == path))
    })
    .await?;
    // Solo i file della libreria: l'API non apre percorsi qualsiasi.
    if !known {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Il modello non è nella libreria: aggiungi la sua cartella nella pagina Modelli dell'app.",
        ));
    }
    let path = body.path.clone();
    let params = blocking(&state.core, move |core| core.preset(&path))
        .await?
        .load;
    let loaded = state
        .core
        .load_model_with(
            &body.path,
            &params,
            app_core::LoadOptions {
                force: body.force,
                replace_others: body.replace,
                make_primary: true,
            },
        )
        .await?;
    Ok(Json(loaded))
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InfillBody {
    /// Il testo prima del cursore.
    prefix: String,
    /// Il testo dopo il cursore.
    suffix: String,
    /// Token massimi da generare (predefinito 128, al massimo 512).
    #[serde(default)]
    #[ts(as = "Option<u32>", optional)]
    max_tokens: Option<u32>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InfillReply {
    /// Il testo da inserire al cursore.
    text: String,
}

/// Completamento del codice in linea (VS-6) con il modello locale.
async fn infill(
    State(state): State<ApiState>,
    Json(body): Json<InfillBody>,
) -> ApiResult<Json<InfillReply>> {
    let text = state
        .core
        .infill(&body.prefix, &body.suffix, body.max_tokens.unwrap_or(128))
        .await?;
    Ok(Json(InfillReply { text }))
}

/// Scarica un modello (`{"path": …}`) o, senza corpo, tutti.
async fn unload_model(
    State(state): State<ApiState>,
    body: axum::body::Bytes,
) -> ApiResult<StatusCode> {
    if body.iter().all(u8::is_ascii_whitespace) {
        state.core.unload_model().await;
    } else {
        let body: PathBody = serde_json::from_slice(&body)
            .map_err(|e| ApiError::bad_request(format!("corpo non valido: {e}")))?;
        state.core.unload_local(&body.path).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Rende principale un modello già caricato.
async fn set_primary_model(
    State(state): State<ApiState>,
    Json(body): Json<PathBody>,
) -> ApiResult<StatusCode> {
    state.core.set_primary_model(&body.path)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Conversazioni ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    query: String,
}

async fn list_conversations(
    State(state): State<ApiState>,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Vec<Conversation>>> {
    let query = q.query;
    Ok(Json(
        blocking(&state.core, move |core| core.list_conversations(&query)).await?,
    ))
}

async fn create_conversation(State(state): State<ApiState>) -> ApiResult<Json<Conversation>> {
    Ok(Json(
        blocking(&state.core, |core| core.create_conversation()).await?,
    ))
}

#[derive(Deserialize, ts_rs::TS)]
pub(crate) struct RenameBody {
    title: String,
}

async fn rename_conversation(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(body): Json<RenameBody>,
) -> ApiResult<StatusCode> {
    blocking(&state.core, move |core| {
        core.rename_conversation(id, &body.title)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_conversation(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    blocking(&state.core, move |core| core.delete_conversation(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn messages(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<Message>>> {
    Ok(Json(
        blocking(&state.core, move |core| core.messages_for_display(id)).await?,
    ))
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchBody {
    /// Un messaggio del ramo da mostrare.
    message_id: i64,
}

/// Mostra un altro ramo della conversazione; restituisce i messaggi da mostrare.
async fn switch_branch(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(body): Json<BranchBody>,
) -> ApiResult<Json<Vec<Message>>> {
    Ok(Json(
        blocking(&state.core, move |core| {
            core.switch_branch(id, body.message_id)?;
            core.messages_for_display(id)
        })
        .await?,
    ))
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendBody {
    text: String,
    /// Percorsi di file da allegare.
    #[serde(default)]
    #[ts(as = "Option<Vec<String>>", optional)]
    attachments: Vec<String>,
    model: ModelRef,
    /// Assenti: si usano quelli del preset del modello.
    #[ts(optional)]
    sampling: Option<SamplingParams>,
    /// Assente: quello della conversazione (vuoto se non ne ha).
    #[ts(optional)]
    system_prompt: Option<String>,
}

/// Invia un messaggio e segue la risposta, strumenti compresi. Se il client
/// si scollega, la risposta si interrompe.
async fn send_message(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(body): Json<SendBody>,
) -> ApiResult<
    Sse<
        impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
> {
    // Controlla subito che il modello sia raggiungibile: l'errore arriva
    // come risposta HTTP e non a flusso già aperto.
    // Legge il preset e, per un modello cloud, la chiave dal portachiavi.
    let model = body.model.clone();
    let (preset, setup) = blocking(&state.core, move |core| {
        Ok((core.preset_for(&model)?, core.conversation_setup(id)?))
    })
    .await?;
    let sampling = body.sampling.unwrap_or(preset.sampling);
    // Il prompt di sistema è della conversazione, come nell'app.
    let system_prompt = body.system_prompt.unwrap_or(setup.system_prompt);

    let (tx, rx) = mpsc::unbounded_channel::<Outgoing>();
    let core = state.core.clone();
    let watcher = {
        let core = core.clone();
        on_disconnect(&tx, move || core.stop_conversation(id))
    };
    tokio::spawn(async move {
        let events = tx.clone();
        let result = core
            .send_message(
                id,
                &body.text,
                &body.attachments,
                &sampling,
                &system_prompt,
                &body.model,
                move |event| {
                    let _ = events.send(Outgoing {
                        event: chat_event(event),
                        last: false,
                    });
                },
            )
            .await;
        watcher.abort();
        let event = match result {
            Ok(outcome) => sse_json(Some("done"), &outcome),
            Err(e) => sse_json(
                Some("error"),
                &ErrorEvent {
                    message: e.to_string(),
                },
            ),
        };
        let _ = tx.send(Outgoing { event, last: true });
    });
    Ok(Sse::new(sse_stream(rx)).keep_alive(KeepAlive::default()))
}

/// Dati degli eventi `text` e `reasoning` del flusso di una risposta.
#[derive(Serialize, ts_rs::TS)]
pub(crate) struct TextEvent {
    pub text: String,
}

/// Dati dell'evento `toolRequest`: uno strumento aspetta la conferma
/// dell'utente (si risponde con `POST /api/tool-requests`).
#[derive(Serialize, ts_rs::TS)]
pub(crate) struct ToolRequestEvent {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub arguments: serde_json::Value,
}

/// Dati dell'evento `modelSwitch`: il modello cloud scelto ha dato errore e la
/// risposta riparte da un modello della coda di riserva. Il testo arrivato prima
/// è già stato scartato da un evento `saved`.
#[derive(Serialize, ts_rs::TS)]
pub(crate) struct ModelSwitchEvent {
    pub from: String,
    pub to: String,
    pub reason: String,
}

/// Dati dell'evento `error`, che chiude il flusso. (L'evento `done` porta un
/// `SendOutcome`; `saved` nessun dato.)
#[derive(Serialize, ts_rs::TS)]
pub(crate) struct ErrorEvent {
    pub message: String,
}

fn chat_event(event: ChatEvent) -> axum::response::sse::Event {
    match event {
        ChatEvent::Text(text) => sse_json(Some("text"), &TextEvent { text }),
        ChatEvent::Reasoning(text) => sse_json(Some("reasoning"), &TextEvent { text }),
        ChatEvent::Saved => sse_json(Some("saved"), &json!({})),
        ChatEvent::ToolRequest {
            id,
            name,
            summary,
            arguments,
        } => sse_json(
            Some("toolRequest"),
            &ToolRequestEvent {
                id,
                name,
                summary,
                arguments,
            },
        ),
        ChatEvent::ModelSwitch { from, to, reason } => {
            sse_json(Some("modelSwitch"), &ModelSwitchEvent { from, to, reason })
        }
    }
}

async fn stop_conversation(State(state): State<ApiState>, Path(id): Path<i64>) -> StatusCode {
    state.core.stop_conversation(id);
    StatusCode::NO_CONTENT
}

async fn conversation_tools(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<ToolSettings>> {
    Ok(Json(
        blocking(&state.core, move |core| core.conversation_tools(id)).await?,
    ))
}

async fn set_conversation_tools(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(settings): Json<ToolSettings>,
) -> ApiResult<StatusCode> {
    blocking(&state.core, move |core| {
        core.set_conversation_tools(id, &settings)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ts_rs::TS)]
pub(crate) struct AnswerBody {
    id: String,
    approval: Approval,
}

/// La risposta dell'utente a una richiesta di conferma (evento `toolRequest`).
/// Arriva da un clic nell'estensione: il modello non ha modo di chiamarla.
async fn answer_tool_request(
    State(state): State<ApiState>,
    Json(body): Json<AnswerBody>,
) -> StatusCode {
    state.core.answer_tool_request(&body.id, body.approval);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionsQuery {
    #[serde(default = "default_limit")]
    limit: u32,
    session_id: Option<i64>,
}

fn default_limit() -> u32 {
    200
}

async fn actions(
    State(state): State<ApiState>,
    Query(q): Query<ActionsQuery>,
) -> ApiResult<Json<Vec<ActionRecord>>> {
    Ok(Json(
        blocking(&state.core, move |core| core.actions(q.limit, q.session_id)).await?,
    ))
}

// ── Agenti ──────────────────────────────────────────────────────────────

async fn agents(State(state): State<ApiState>) -> ApiResult<Json<Vec<Agent>>> {
    Ok(Json(blocking(&state.core, |core| core.agents()).await?))
}

fn spawn_session(core: &Arc<Core>, id: i64) {
    let core = core.clone();
    tokio::spawn(async move {
        // L'esito, errori compresi, finisce nell'archivio.
        let _ = core.run_session(id).await;
    });
}

#[derive(Deserialize, ts_rs::TS)]
pub(crate) struct TaskBody {
    task: String,
}

async fn start_session(
    State(state): State<ApiState>,
    Path(agent_id): Path<i64>,
    Json(body): Json<TaskBody>,
) -> ApiResult<Json<AgentSession>> {
    let session = blocking(&state.core, move |core| {
        core.create_session(agent_id, &body.task)
    })
    .await?;
    spawn_session(&state.core, session.id);
    Ok(Json(session))
}

async fn sessions(State(state): State<ApiState>) -> ApiResult<Json<Vec<AgentSession>>> {
    Ok(Json(blocking(&state.core, |core| core.sessions()).await?))
}

async fn session(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<AgentSession>> {
    Ok(Json(
        blocking(&state.core, move |core| core.session(id)).await?,
    ))
}

async fn session_live(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> Json<Option<LiveState>> {
    Json(state.core.session_live(id))
}

async fn pause_session(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    blocking(&state.core, move |core| core.pause_session(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resume_session(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    blocking(&state.core, move |core| core.resume_session(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn stop_session(State(state): State<ApiState>, Path(id): Path<i64>) -> StatusCode {
    state.core.stop_session(id);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize, ts_rs::TS)]
pub(crate) struct ReplyBody {
    text: String,
}

async fn reply_session(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(body): Json<ReplyBody>,
) -> ApiResult<Json<AgentSession>> {
    let session = blocking(&state.core, move |core| core.reply_session(id, &body.text)).await?;
    spawn_session(&state.core, id);
    Ok(Json(session))
}

/// Annulla le modifiche dell'agente: cartelle come prima della sessione.
async fn restore_session(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<AgentSession>> {
    Ok(Json(
        blocking(&state.core, move |core| core.restore_session(id)).await?,
    ))
}

async fn delete_session(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    blocking(&state.core, move |core| core.delete_session(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Pianificazioni (ADR 0028) ───────────────────────────────────────────

async fn schedules(State(state): State<ApiState>) -> ApiResult<Json<Vec<Schedule>>> {
    Ok(Json(blocking(&state.core, |core| core.schedules()).await?))
}

/// Crea una pianificazione (l'`id` del corpo si ignora).
async fn save_schedule(
    State(state): State<ApiState>,
    Json(body): Json<Schedule>,
) -> ApiResult<Json<Schedule>> {
    let body = Schedule { id: 0, ..body };
    Ok(Json(
        blocking(&state.core, move |core| core.save_schedule(body)).await?,
    ))
}

async fn update_schedule(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(body): Json<Schedule>,
) -> ApiResult<Json<Schedule>> {
    if id == 0 {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Pianificazione non trovata.",
        ));
    }
    let body = Schedule { id, ..body };
    Ok(Json(
        blocking(&state.core, move |core| core.save_schedule(body)).await?,
    ))
}

async fn delete_schedule(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    blocking(&state.core, move |core| core.delete_schedule(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Esegue subito il compito, con le regole di un'esecuzione pianificata.
async fn run_schedule(State(state): State<ApiState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    state.core.run_schedule_now(id)?;
    Ok(StatusCode::ACCEPTED)
}
