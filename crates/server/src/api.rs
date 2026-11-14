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
        .route("/api/sessions/{id}/pause", post(pause_session))
        .route("/api/sessions/{id}/resume", post(resume_session))
        .route("/api/sessions/{id}/stop", post(stop_session))
        .route("/api/sessions/{id}/reply", post(reply_session))
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Info {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalModels {
    loaded: Option<LoadedModel>,
    loading: bool,
    library: Vec<ModelInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Models {
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadBody {
    path: String,
    /// Carica anche se la stima dice che non entra in memoria.
    #[serde(default)]
    force: bool,
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
        .load_model(&body.path, &params, body.force)
        .await?;
    Ok(Json(loaded))
}

async fn unload_model(State(state): State<ApiState>) -> StatusCode {
    state.core.unload_model().await;
    StatusCode::NO_CONTENT
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

#[derive(Deserialize)]
struct RenameBody {
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendBody {
    text: String,
    /// Percorsi di file da allegare.
    #[serde(default)]
    attachments: Vec<String>,
    model: ModelRef,
    /// Assenti: si usano quelli del preset del modello.
    sampling: Option<SamplingParams>,
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
    let preset = blocking(&state.core, move |core| core.preset_for(&model)).await?;
    let sampling = body.sampling.unwrap_or(preset.sampling);
    let system_prompt = body.system_prompt.unwrap_or(preset.system_prompt);

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
            Err(e) => sse_json(Some("error"), &json!({ "message": e.to_string() })),
        };
        let _ = tx.send(Outgoing { event, last: true });
    });
    Ok(Sse::new(sse_stream(rx)).keep_alive(KeepAlive::default()))
}

fn chat_event(event: ChatEvent) -> axum::response::sse::Event {
    match event {
        ChatEvent::Text(text) => sse_json(Some("text"), &json!({ "text": text })),
        ChatEvent::Reasoning(text) => sse_json(Some("reasoning"), &json!({ "text": text })),
        ChatEvent::Saved => sse_json(Some("saved"), &json!({})),
        ChatEvent::ToolRequest {
            id,
            name,
            summary,
            arguments,
        } => sse_json(
            Some("toolRequest"),
            &json!({ "id": id, "name": name, "summary": summary, "arguments": arguments }),
        ),
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

#[derive(Deserialize)]
struct AnswerBody {
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

#[derive(Deserialize)]
struct TaskBody {
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

#[derive(Deserialize)]
struct ReplyBody {
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

async fn delete_session(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    blocking(&state.core, move |core| core.delete_session(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
