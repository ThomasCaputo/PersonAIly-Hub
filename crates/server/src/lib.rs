//! API locale del nucleo (VS-1, SI-4; [ADR 0004](../../../docs/adr/0004-nucleo-condiviso-api-locale.md),
//! [ADR 0010](../../../docs/adr/0010-api-locale-e-vscode.md)).
//!
//! La usano l'eseguibile `app-server` (nucleo senza interfaccia) e l'app
//! desktop, che la avvia dentro il proprio processo. Due famiglie di endpoint:
//! `/v1/...` compatibile con OpenAI e `/api/...` propria dell'app.
//!
//! Protezioni: ascolto solo su `127.0.0.1`, token obbligatorio, rifiuto delle
//! richieste che arrivano da pagine web (intestazione `Origin`) e di quelle
//! indirizzate a un nome diverso da `127.0.0.1`/`localhost` (DNS rebinding).

mod api;
mod openai;

use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use app_core::Core;
use app_core::instance::{self, InstanceKind, LockInfo};
use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use futures_util::Stream;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::{Notify, mpsc};

/// Porta preferita: fissa, così i programmi configurati a mano la ritrovano.
/// Se è occupata se ne usa una libera, indicata nel file di blocco.
pub const DEFAULT_PORT: u16 = 41241;
/// I file aperti in VS Code viaggiano nel corpo delle richieste.
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct ApiState {
    pub core: Arc<Core>,
    pub shared: Arc<Shared>,
}

pub(crate) struct Shared {
    token: RwLock<String>,
    kind: InstanceKind,
    port: u16,
    quit: Notify,
    last_request: Mutex<Instant>,
}

impl Shared {
    fn token(&self) -> String {
        self.token.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

// ── Errori ──────────────────────────────────────────────────────────────

/// Errore restituito ai client, nel formato di OpenAI:
/// `{"error": {"message": "...", "type": "..."}}`.
#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
}

impl From<app_core::error::Error> for ApiError {
    fn from(e: app_core::error::Error) -> Self {
        Self::bad_request(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let kind = match self.status {
            StatusCode::UNAUTHORIZED => "authentication_error",
            StatusCode::FORBIDDEN => "permission_error",
            StatusCode::NOT_FOUND => "not_found_error",
            s if s.is_server_error() => "server_error",
            _ => "invalid_request_error",
        };
        let body = serde_json::json!({ "error": { "message": self.message, "type": kind } });
        (self.status, axum::Json(body)).into_response()
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;

// ── Server-Sent Events ──────────────────────────────────────────────────

/// Un evento da mandare al client; `last` chiude il flusso.
pub(crate) struct Outgoing {
    pub event: Event,
    pub last: bool,
}

pub(crate) fn sse_json(name: Option<&str>, value: &impl Serialize) -> Event {
    let data = serde_json::to_string(value).unwrap_or_else(|_| "{}".into());
    let event = Event::default().data(data);
    match name {
        Some(name) => event.event(name),
        None => event,
    }
}

/// Il flusso SSE alimentato da un canale; finisce dopo l'evento con `last`.
pub(crate) fn sse_stream(
    rx: mpsc::UnboundedReceiver<Outgoing>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    futures_util::stream::unfold((rx, false), |(mut rx, ended)| async move {
        if ended {
            return None;
        }
        let out = rx.recv().await?;
        Some((Ok(out.event), (rx, out.last)))
    })
}

/// Chiama `on_close` se il client si scollega prima della fine. Il compito
/// restituito va interrotto quando il lavoro finisce da sé.
pub(crate) fn on_disconnect(
    tx: &mpsc::UnboundedSender<Outgoing>,
    on_close: impl FnOnce() + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    let tx = tx.clone();
    tokio::spawn(async move {
        tx.closed().await;
        on_close();
    })
}

// ── Protezioni ──────────────────────────────────────────────────────────

async fn guard(State(state): State<ApiState>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    // I browser aggiungono sempre `Origin` alle richieste fatte da una pagina:
    // nessuna pagina web deve poter usare l'API, nemmeno indovinando il token.
    if headers.contains_key(header::ORIGIN) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "Le pagine web non possono usare l'API locale.",
        )
        .into_response();
    }
    let port = state.shared.port;
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|host| {
            host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
        });
    if !host_ok {
        return ApiError::new(StatusCode::FORBIDDEN, "Indirizzo non ammesso.").into_response();
    }
    let token_ok = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .is_some_and(|given| instance::tokens_match(given.trim(), &state.shared.token()));
    if !token_ok {
        return ApiError::new(
            StatusCode::UNAUTHORIZED,
            "Token mancante o sbagliato: lo trovi nell'app, in Impostazioni → API locale.",
        )
        .into_response();
    }
    *state
        .shared
        .last_request
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Instant::now();
    next.run(request).await
}

fn router(state: ApiState) -> Router {
    Router::new()
        .merge(api::routes())
        .merge(openai::routes())
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "Indirizzo sconosciuto.") })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

// ── Avvio e arresto ─────────────────────────────────────────────────────

/// L'API in esecuzione.
pub struct ApiHandle {
    shared: Arc<Shared>,
    data_dir: PathBuf,
    stop: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}

impl ApiHandle {
    pub fn port(&self) -> u16 {
        self.shared.port
    }

    pub fn token(&self) -> String {
        self.shared.token()
    }

    /// Crea un token nuovo: quello vecchio smette subito di valere.
    pub fn regenerate_token(&self) -> Result<String, String> {
        let token = app_core::secrets::regenerate_api_token().map_err(|e| e.to_string())?;
        *self.shared.token.write().unwrap_or_else(|e| e.into_inner()) = token.clone();
        instance::write_lock(&self.data_dir, &self.lock_info()).map_err(|e| e.to_string())?;
        Ok(token)
    }

    fn lock_info(&self) -> LockInfo {
        LockInfo {
            port: self.shared.port,
            token: self.shared.token(),
            pid: std::process::id(),
            kind: self.shared.kind,
            version: app_core::version().to_string(),
        }
    }

    /// Si risveglia quando un client chiede la chiusura (`POST /api/shutdown`).
    pub async fn quit_requested(&self) {
        self.shared.quit.notified().await;
    }

    /// Da quanto tempo non arrivano richieste.
    pub fn idle_for(&self) -> Duration {
        self.shared
            .last_request
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed()
    }

    /// Ferma l'API e toglie il file di blocco.
    pub async fn stop(self) {
        instance::remove_lock(&self.data_dir);
        self.stop.notify_one();
        // I flussi ancora aperti non devono trattenere la chiusura per sempre.
        let mut task = self.task;
        if tokio::time::timeout(Duration::from_secs(3), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
    }
}

/// Avvia l'API su `127.0.0.1` e scrive il file di blocco. Con `port` a 0 la
/// porta la sceglie il sistema. Fallisce se un altro nucleo è già attivo.
pub async fn start(
    core: Arc<Core>,
    data_dir: &Path,
    kind: InstanceKind,
    port: u16,
) -> Result<ApiHandle, String> {
    let token = tokio::task::spawn_blocking(app_core::secrets::api_token)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    start_with_token(core, data_dir, kind, port, token).await
}

/// Come [`start`], con un token dato invece di quello del portachiavi.
pub async fn start_with_token(
    core: Arc<Core>,
    data_dir: &Path,
    kind: InstanceKind,
    port: u16,
    token: String,
) -> Result<ApiHandle, String> {
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
        Ok(listener) => listener,
        Err(_) if port != 0 => TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|e| format!("impossibile aprire una porta per l'API locale: {e}"))?,
        Err(e) => {
            return Err(format!(
                "impossibile aprire una porta per l'API locale: {e}"
            ));
        }
    };
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let shared = Arc::new(Shared {
        token: RwLock::new(token),
        kind,
        port,
        quit: Notify::new(),
        last_request: Mutex::new(Instant::now()),
    });
    let stop = Arc::new(Notify::new());
    let handle = ApiHandle {
        shared: shared.clone(),
        data_dir: data_dir.to_path_buf(),
        stop: stop.clone(),
        task: tokio::spawn(async {}),
    };
    if let Some(other) =
        instance::claim_lock(data_dir, &handle.lock_info()).map_err(|e| e.to_string())?
    {
        return Err(format!(
            "Un altro nucleo è già attivo (processo {}, porta {}).",
            other.pid, other.port
        ));
    }

    let app = router(ApiState { core, shared });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move { stop.notified().await })
            .await;
    });
    Ok(ApiHandle { task, ..handle })
}

/// Se è in esecuzione il nucleo senza interfaccia, gli chiede di chiudersi e
/// aspetta che lo faccia: l'app desktop prende il suo posto e i client si
/// ricollegano leggendo il nuovo file di blocco.
pub async fn replace_server(data_dir: &Path) -> Result<(), String> {
    let Some(lock) = instance::running_instance(data_dir) else {
        return Ok(());
    };
    if lock.kind != InstanceKind::Server {
        return Err("Un'altra copia dell'app è già aperta: l'API locale è la sua.".into());
    }
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    // Se non risponde si aspetta comunque: forse si sta già chiudendo.
    let _ = http
        .post(format!("http://127.0.0.1:{}/api/shutdown", lock.port))
        .bearer_auth(&lock.token)
        .send()
        .await;
    // Chiudersi può voler dire scaricare un modello: qualche secondo.
    for _ in 0..200 {
        if !instance::process_alive(lock.pid) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!(
        "Il nucleo senza interfaccia (processo {}) non si è chiuso.",
        lock.pid
    ))
}
