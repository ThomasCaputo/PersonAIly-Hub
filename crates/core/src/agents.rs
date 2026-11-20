//! Agenti: definizioni e sessioni (AG-1 … AG-6).
//!
//! Un **agente** è una ricetta salvata: modello, istruzioni, cartelle su cui
//! lavora, strumenti e permessi. Una **sessione** è un agente al lavoro su un
//! compito: riusa lo stesso ciclo modello → strumenti della chat (vedi
//! `Core::run_rounds`), ma va avanti da solo, passo dopo passo, finché il
//! modello non risponde senza chiedere altri strumenti. I messaggi della
//! sessione stanno in una conversazione propria, che non compare in chat.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

use crate::ModelRef;
use crate::snapshots::RestorePoint;
use crate::tools::{Risk, ToolDef, ToolSettings};

/// Passi (giri modello → strumenti) predefiniti per una sessione.
pub const DEFAULT_MAX_STEPS: u32 = 50;
/// Nessun agente può fare più passi di così senza che l'utente risponda.
pub const MAX_STEPS_LIMIT: u32 = 500;

/// Lo strumento con cui un agente affida un compito a un altro (AG-8).
pub const DELEGATE_TOOL: &str = "delegate_task";
/// Quanti livelli di deleghe: l'agente avviato dall'utente è il livello 0,
/// quelli a cui affida lavoro il livello 1, i loro aiutanti il 2. Oltre, lo
/// strumento non viene offerto.
pub const MAX_DELEGATION_DEPTH: u32 = 2;

/// Lo strumento di delega, con gli agenti che si possono scegliere.
pub fn delegate_tool(agents: &[Agent]) -> ToolDef {
    let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    let list = agents
        .iter()
        .map(|a| {
            if a.description.is_empty() {
                format!("- {}", a.name)
            } else {
                format!("- {}: {}", a.name, a.description)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    ToolDef {
        name: DELEGATE_TOOL.into(),
        description: format!(
            "Affida una parte del lavoro a un altro agente e aspetta il suo resoconto. \
             Descrivi il compito in modo completo: l'altro agente non vede questa \
             conversazione. Agenti disponibili:\n{list}"
        )
        .into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "agent": { "type": "string", "enum": names, "description": "Nome dell'agente" },
                "task": { "type": "string", "description": "Il compito, scritto per intero" }
            },
            "required": ["agent", "task"]
        }),
        // Avvia un altro agente autonomo: conta come eseguire qualcosa.
        risk: Risk::Execute,
    }
}

/// Istruzioni comuni a tutti gli agenti, prima di quelle scritte dall'utente.
pub const AGENT_PREAMBLE: &str = "Sei un agente che lavora in autonomia su un compito \
    assegnato dall'utente. Procedi un passo alla volta usando gli strumenti: guarda prima \
    com'è fatta la cartella, poi agisci, poi verifica il risultato. Non chiedere conferme \
    per ogni passo: è l'app a chiederle all'utente quando servono. Quando hai finito, \
    rispondi con un breve resoconto di ciò che hai fatto e dei file che hai toccato. Se ti \
    manca un'informazione indispensabile, fermati e chiedila.";

/// La definizione di un agente (AG-1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Agent {
    /// 0 per un agente non ancora salvato.
    pub id: i64,
    pub name: String,
    pub description: String,
    /// Istruzioni dell'utente, aggiunte al prompt di sistema.
    pub instructions: String,
    pub model: ModelRef,
    /// Le cartelle su cui l'agente lavora: sono le sue cartelle concesse.
    pub folders: Vec<String>,
    pub tools: ToolSettings,
    /// Server MCP (identificatori) avviati con ogni sessione.
    pub mcp_servers: Vec<String>,
    /// Passi massimi prima di fermarsi e aspettare l'utente.
    pub max_steps: u32,
    /// Prima di ogni sessione fotografa le cartelle, per poter annullare
    /// tutto ciò che l'agente ha cambiato (vedi [`crate::snapshots`]).
    pub restore_points: bool,
    /// Gli agenti (id) a cui questo può affidare parte del lavoro (AG-8).
    pub delegates: Vec<i64>,
}

impl Default for Agent {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            description: String::new(),
            instructions: String::new(),
            model: ModelRef::Local,
            folders: Vec::new(),
            tools: ToolSettings {
                files: true,
                web: false,
                terminal: true,
                ..Default::default()
            },
            mcp_servers: Vec::new(),
            max_steps: DEFAULT_MAX_STEPS,
            restore_points: true,
            delegates: Vec::new(),
        }
    }
}

impl Agent {
    /// Ripulisce i campi scritti dall'utente.
    pub fn normalized(mut self) -> Self {
        self.name = self.name.trim().to_string();
        self.description = self.description.trim().to_string();
        self.instructions = self.instructions.trim().to_string();
        self.folders.retain(|f| !f.trim().is_empty());
        self.folders.dedup();
        self.mcp_servers.dedup();
        self.delegates.sort_unstable();
        self.delegates.dedup();
        self.delegates.retain(|&id| id != self.id);
        self.max_steps = self.max_steps.clamp(1, MAX_STEPS_LIMIT);
        self
    }
}

/// Stato di una sessione.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Al lavoro.
    Running,
    /// In pausa: si ferma prima del passo successivo e aspetta.
    Paused,
    /// Aspetta che l'utente confermi uno strumento.
    Waiting,
    /// Il modello ha risposto senza chiedere altri strumenti.
    Done,
    /// Fermata dall'utente, o arrivata al limite di passi.
    Stopped,
    /// Finita con un errore (modello non raggiungibile, servizio che rifiuta…).
    Failed,
    /// L'app è stata chiusa mentre lavorava.
    Interrupted,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Waiting => "waiting",
            Self::Done => "done",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "running" => Self::Running,
            "paused" => Self::Paused,
            "waiting" => Self::Waiting,
            "done" => Self::Done,
            "stopped" => Self::Stopped,
            "failed" => Self::Failed,
            _ => Self::Interrupted,
        }
    }

    /// La sessione sta ancora lavorando (o aspetta di poterlo fare).
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Paused | Self::Waiting)
    }
}

/// Una sessione come la vede l'interfaccia (AG-4).
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: i64,
    /// `None` se l'agente è stato eliminato nel frattempo.
    pub agent_id: Option<i64>,
    pub agent_name: String,
    pub conversation_id: i64,
    pub task: String,
    pub status: SessionStatus,
    /// Errore o avviso con cui la sessione si è fermata.
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Copia della definizione dell'agente al momento dell'avvio: modificare
    /// l'agente non cambia una sessione già partita.
    #[serde(skip)]
    pub definition: Option<Agent>,
    /// Le cartelle fotografate prima che l'agente lavorasse; vuoto se non ce
    /// ne sono (opzione spenta, git mancante o foto non riuscita).
    pub restore_points: Vec<RestorePoint>,
    /// La sessione che ha affidato questo lavoro, se l'ha avviata un altro agente.
    pub parent_session_id: Option<i64>,
}

/// Uno strumento in attesa di conferma, per l'interfaccia.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingRequest {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub arguments: Value,
}

/// Ciò che una sessione sta facendo in questo momento: il testo in arrivo e
/// l'eventuale richiesta di conferma. L'interfaccia lo legge a intervalli.
#[derive(Debug, Clone, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LiveState {
    pub status: Option<SessionStatus>,
    pub streaming: String,
    pub reasoning: String,
    pub request: Option<PendingRequest>,
    /// Cresce a ogni nuovo messaggio salvato: se cambia, l'interfaccia rilegge.
    pub revision: u64,
}

/// Comandi di un ciclo in corso: interruzione e pausa.
#[derive(Debug, Default)]
pub struct RunControl {
    /// Si risveglia quando l'utente interrompe. È condiviso con lo streaming.
    pub cancel: Arc<Notify>,
    stopped: AtomicBool,
    paused: AtomicBool,
    resume: Notify,
}

impl RunControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.cancel.notify_one();
        // Chi è in pausa deve accorgersi dell'arresto.
        self.resume.notify_one();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.resume.notify_one();
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Punto di controllo tra un passo e l'altro: se è stata chiesta una
    /// pausa aspetta di riprendere. `false` se nel frattempo è stato fermato.
    pub async fn checkpoint(&self) -> bool {
        while self.is_paused() && !self.is_stopped() {
            self.resume.notified().await;
        }
        !self.is_stopped()
    }
}

/// Una sessione in esecuzione.
#[derive(Debug, Default)]
pub struct LiveSession {
    pub control: RunControl,
    state: Mutex<LiveState>,
}

impl LiveSession {
    pub fn new() -> Self {
        let live = Self::default();
        live.state().status = Some(SessionStatus::Running);
        live
    }

    pub fn state(&self) -> MutexGuard<'_, LiveState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Lo stato da mostrare, tenendo conto della pausa chiesta.
    pub fn snapshot(&self) -> LiveState {
        let mut state = self.state().clone();
        if state.request.is_none() && self.control.is_paused() {
            state.status = Some(SessionStatus::Paused);
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gli_stati_si_salvano_e_si_rileggono() {
        for status in [
            SessionStatus::Running,
            SessionStatus::Paused,
            SessionStatus::Waiting,
            SessionStatus::Done,
            SessionStatus::Stopped,
            SessionStatus::Failed,
            SessionStatus::Interrupted,
        ] {
            assert_eq!(SessionStatus::parse(status.as_str()), status);
        }
        assert_eq!(SessionStatus::parse("???"), SessionStatus::Interrupted);
    }

    #[test]
    fn la_definizione_viene_ripulita() {
        let agent = Agent {
            name: "  Revisore  ".into(),
            folders: vec!["C:\\a".into(), "C:\\a".into(), " ".into()],
            max_steps: 0,
            ..Default::default()
        }
        .normalized();
        assert_eq!(agent.name, "Revisore");
        assert_eq!(agent.folders, ["C:\\a"]);
        assert_eq!(agent.max_steps, 1);
    }

    #[tokio::test]
    async fn la_pausa_aspetta_e_l_arresto_la_sblocca() {
        let control = Arc::new(RunControl::new());
        assert!(control.checkpoint().await);

        control.pause();
        let waiting = {
            let control = control.clone();
            tokio::spawn(async move { control.checkpoint().await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!waiting.is_finished());
        control.resume();
        assert!(waiting.await.unwrap());

        control.pause();
        let waiting = {
            let control = control.clone();
            tokio::spawn(async move { control.checkpoint().await })
        };
        control.stop();
        assert!(!waiting.await.unwrap());
    }
}
