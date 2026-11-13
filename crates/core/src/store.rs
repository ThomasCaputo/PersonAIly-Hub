//! Archivio SQLite: conversazioni, messaggi, impostazioni.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::agents::{Agent, AgentSession, SessionStatus};
use crate::attachments::{Attachment, AttachmentInfo};
use crate::error::{Error, Result};
use crate::tools::ToolCall;

/// Passi di aggiornamento dello schema, in ordine.
///
/// Per cambiare lo schema si **aggiunge** un elemento in fondo: quelli già
/// pubblicati non si modificano, perché sui PC degli utenti sono già stati applicati.
const MIGRATIONS: &[&str] = &[
    // 1 — conversazioni e messaggi
    "
    CREATE TABLE conversations (
        id          INTEGER PRIMARY KEY,
        title       TEXT    NOT NULL,
        created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
        updated_at  TEXT    NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE messages (
        id               INTEGER PRIMARY KEY,
        conversation_id  INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
        role             TEXT    NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
        content          TEXT    NOT NULL,
        model            TEXT,
        created_at       TEXT    NOT NULL DEFAULT (datetime('now'))
    );

    CREATE INDEX messages_by_conversation ON messages (conversation_id, id);
    ",
    // 2 — impostazioni (chiave → valore JSON)
    "
    CREATE TABLE settings (
        key    TEXT PRIMARY KEY,
        value  TEXT NOT NULL
    );
    ",
    // 3 — consumo di ogni risposta: token e costo stimato
    "
    ALTER TABLE messages ADD COLUMN prompt_tokens INTEGER;
    ALTER TABLE messages ADD COLUMN completion_tokens INTEGER;
    ALTER TABLE messages ADD COLUMN cost_usd REAL;
    ",
    // 4 — strumenti (chiamate e risultati, in JSON), allegati dei messaggi,
    //     strumenti e permessi di ogni conversazione
    "
    ALTER TABLE messages ADD COLUMN tool_data TEXT;
    ALTER TABLE messages ADD COLUMN attachments TEXT;
    ALTER TABLE conversations ADD COLUMN tools TEXT;
    ",
    // 5 — agenti, loro sessioni (ognuna con la sua conversazione) e registro
    //     delle azioni compiute dagli strumenti, in chat e dagli agenti
    "
    CREATE TABLE agents (
        id          INTEGER PRIMARY KEY,
        name        TEXT NOT NULL,
        definition  TEXT NOT NULL,
        created_at  TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE agent_sessions (
        id               INTEGER PRIMARY KEY,
        agent_id         INTEGER REFERENCES agents(id) ON DELETE SET NULL,
        agent_name       TEXT    NOT NULL,
        definition       TEXT    NOT NULL,
        conversation_id  INTEGER NOT NULL UNIQUE REFERENCES conversations(id) ON DELETE CASCADE,
        task             TEXT    NOT NULL,
        status           TEXT    NOT NULL,
        note             TEXT,
        created_at       TEXT    NOT NULL DEFAULT (datetime('now')),
        updated_at       TEXT    NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE actions (
        id               INTEGER PRIMARY KEY,
        created_at       TEXT    NOT NULL DEFAULT (datetime('now')),
        conversation_id  INTEGER REFERENCES conversations(id) ON DELETE SET NULL,
        session_id       INTEGER REFERENCES agent_sessions(id) ON DELETE SET NULL,
        kind             TEXT    NOT NULL CHECK (kind IN ('chat', 'agent')),
        source           TEXT    NOT NULL,
        tool             TEXT    NOT NULL,
        summary          TEXT    NOT NULL,
        approval         TEXT    NOT NULL,
        is_error         INTEGER NOT NULL,
        result           TEXT    NOT NULL
    );

    CREATE INDEX actions_by_time ON actions (id DESC);
    ",
];

/// Caratteri del risultato conservati nel registro delle azioni.
const ACTION_RESULT_CHARS: usize = 400;

/// Colonne lette per ogni sessione, nell'ordine atteso da `row_to_session`.
const SESSION_COLUMNS: &str = "id, agent_id, agent_name, conversation_id, task, status, note, \
     created_at, updated_at, definition";

/// Una voce del registro delle azioni (SI-6).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRecord {
    pub id: i64,
    pub created_at: String,
    pub conversation_id: Option<i64>,
    pub session_id: Option<i64>,
    /// `chat` o `agent`: resta anche quando conversazione o sessione non ci sono più.
    pub kind: String,
    /// Titolo della conversazione o nome dell'agente, com'era al momento dell'azione.
    pub source: String,
    pub tool: String,
    pub summary: String,
    /// `auto` (consentita dal livello di permesso), `user` (confermata),
    /// `denied` (negata), `refused` (fuori confine), `interrupted`.
    pub approval: String,
    pub is_error: bool,
    /// Inizio del risultato.
    pub result: String,
}

/// Una nuova voce del registro.
pub struct NewAction<'a> {
    pub conversation_id: i64,
    pub session_id: Option<i64>,
    pub source: &'a str,
    pub tool: &'a str,
    pub summary: &'a str,
    pub approval: &'a str,
    pub is_error: bool,
    pub result: &'a str,
}

/// Colonne lette per ogni messaggio, nell'ordine atteso da `row_to_message`.
const MESSAGE_COLUMNS: &str = "id, conversation_id, role, content, model, created_at, \
     prompt_tokens, completion_tokens, cost_usd, tool_data, attachments";

/// Ciò che un messaggio ha a che fare con gli strumenti, salvato in JSON.
///
/// Un messaggio `assistant` elenca le chiamate chieste dal modello (`calls`);
/// un messaggio `tool` è il risultato di una chiamata (`call_id`, `name`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolData {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<ToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub created_at: String,
    /// Token letti dal modello per produrre questa risposta (tutta la conversazione fin lì).
    pub prompt_tokens: Option<u64>,
    /// Token generati in questa risposta.
    pub completion_tokens: Option<u64>,
    /// Costo stimato della risposta in dollari; assente per i modelli locali
    /// o quando il prezzo non è noto.
    pub cost_usd: Option<f64>,
    /// Strumenti chiesti dal modello in questa risposta.
    pub tool_calls: Vec<ToolCall>,
    /// Per un messaggio `tool`: la chiamata di cui è il risultato.
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    /// Per un messaggio `tool`: lo strumento è fallito o è stato negato.
    pub is_error: bool,
    /// Nome e tipo degli allegati, per l'interfaccia.
    pub attachments: Vec<AttachmentInfo>,
    /// Contenuto degli allegati: serve al modello, non all'interfaccia.
    #[serde(skip)]
    pub attachment_data: Vec<Attachment>,
}

/// Accesso all'archivio dell'app.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Apre (o crea) l'archivio nel file indicato e lo porta allo schema più recente.
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Archivio temporaneo in memoria, per i test.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;

        let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", index as i64 + 1)?;
            tx.commit()?;
        }

        Ok(Self { conn })
    }

    /// Numero dell'ultimo passo di aggiornamento applicato.
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    // ── Conversazioni ───────────────────────────────────────────────────

    pub fn create_conversation(&self, title: &str) -> Result<Conversation> {
        self.conn
            .execute("INSERT INTO conversations (title) VALUES (?1)", [title])?;
        self.conversation(self.conn.last_insert_rowid())
    }

    pub fn conversation(&self, id: i64) -> Result<Conversation> {
        Ok(self.conn.query_row(
            "SELECT id, title, created_at, updated_at FROM conversations WHERE id = ?1",
            [id],
            row_to_conversation,
        )?)
    }

    /// Le conversazioni, dalla più recente. Con `query` non vuota, solo quelle
    /// che contengono il testo nel titolo o in un messaggio (senza distinguere
    /// maiuscole e minuscole).
    pub fn list_conversations(&self, query: &str) -> Result<Vec<Conversation>> {
        // `%` e `_` sono caratteri jolly di LIKE: se l'utente li cerca vanno presi alla lettera.
        let escaped = query
            .trim()
            .replace('\\', r"\\")
            .replace('%', r"\%")
            .replace('_', r"\_");
        let pattern = format!("%{escaped}%");
        // Le conversazioni delle sessioni degli agenti si vedono nella pagina Agenti.
        let mut stmt = self.conn.prepare(
            r"SELECT id, title, created_at, updated_at FROM conversations c
              WHERE NOT EXISTS (SELECT 1 FROM agent_sessions s WHERE s.conversation_id = c.id)
                AND (c.title LIKE ?1 ESCAPE '\'
                     OR EXISTS (SELECT 1 FROM messages m
                                WHERE m.conversation_id = c.id AND m.content LIKE ?1 ESCAPE '\'))
              ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([pattern], row_to_conversation)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn rename_conversation(&self, id: i64, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE conversations SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(())
    }

    /// Elimina la conversazione e tutti i suoi messaggi.
    pub fn delete_conversation(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM conversations WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Strumenti e permessi della conversazione, in JSON; `None` se mai impostati.
    pub fn conversation_tools(&self, id: i64) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT tools FROM conversations WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    }

    pub fn set_conversation_tools(&self, id: i64, tools_json: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE conversations SET tools = ?2 WHERE id = ?1",
            params![id, tools_json],
        )?;
        Ok(())
    }

    // ── Messaggi ────────────────────────────────────────────────────────

    pub fn messages(&self, conversation_id: i64) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE conversation_id = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([conversation_id], row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Aggiunge un messaggio e segna la conversazione come appena aggiornata.
    pub fn add_message(
        &self,
        conversation_id: i64,
        role: &str,
        content: &str,
        model: Option<&str>,
    ) -> Result<Message> {
        self.add_message_with(conversation_id, role, content, model, None, &[])
    }

    /// Come [`Store::add_message`], con i dati sugli strumenti e gli allegati.
    pub fn add_message_with(
        &self,
        conversation_id: i64,
        role: &str,
        content: &str,
        model: Option<&str>,
        tool: Option<&ToolData>,
        attachments: &[Attachment],
    ) -> Result<Message> {
        let tool_json = tool.map(serde_json::to_string).transpose()?;
        let attachments_json = if attachments.is_empty() {
            None
        } else {
            Some(serde_json::to_string(attachments)?)
        };
        self.conn.execute(
            "INSERT INTO messages (conversation_id, role, content, model, tool_data, attachments)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                conversation_id,
                role,
                content,
                model,
                tool_json,
                attachments_json
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "UPDATE conversations SET updated_at = datetime('now') WHERE id = ?1",
            [conversation_id],
        )?;
        self.message(id)
    }

    fn message(&self, id: i64) -> Result<Message> {
        Ok(self.conn.query_row(
            &format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"),
            [id],
            row_to_message,
        )?)
    }

    /// Registra quanto è costata una risposta.
    pub fn set_message_usage(
        &self,
        id: i64,
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        cost_usd: Option<f64>,
    ) -> Result<Message> {
        self.conn.execute(
            "UPDATE messages SET prompt_tokens = ?2, completion_tokens = ?3, cost_usd = ?4
             WHERE id = ?1",
            params![
                id,
                prompt_tokens.map(|n| n as i64),
                completion_tokens.map(|n| n as i64),
                cost_usd
            ],
        )?;
        self.message(id)
    }

    // ── Agenti ──────────────────────────────────────────────────────────

    pub fn agents(&self) -> Result<Vec<Agent>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, definition FROM agents ORDER BY name COLLATE NOCASE, id")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut agents = Vec::new();
        for row in rows {
            let (id, json) = row?;
            // Una definizione illeggibile non deve nascondere le altre.
            if let Ok(agent) = serde_json::from_str::<Agent>(&json) {
                agents.push(Agent { id, ..agent });
            }
        }
        Ok(agents)
    }

    pub fn agent(&self, id: i64) -> Result<Agent> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT definition FROM agents WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        let json =
            json.ok_or_else(|| Error::msg("Agente non trovato: forse è stato eliminato."))?;
        Ok(Agent {
            id,
            ..serde_json::from_str(&json)?
        })
    }

    /// Crea l'agente se `id` è 0, altrimenti lo aggiorna.
    pub fn save_agent(&self, agent: &Agent) -> Result<Agent> {
        let json = serde_json::to_string(&Agent {
            id: 0,
            ..agent.clone()
        })?;
        let id = if agent.id == 0 {
            self.conn.execute(
                "INSERT INTO agents (name, definition) VALUES (?1, ?2)",
                params![agent.name, json],
            )?;
            self.conn.last_insert_rowid()
        } else {
            let changed = self.conn.execute(
                "UPDATE agents SET name = ?2, definition = ?3, updated_at = datetime('now')
                 WHERE id = ?1",
                params![agent.id, agent.name, json],
            )?;
            if changed == 0 {
                return Err(Error::msg("Agente non trovato: forse è stato eliminato."));
            }
            agent.id
        };
        self.agent(id)
    }

    /// Elimina l'agente. Le sue sessioni restano, con il nome che aveva.
    pub fn delete_agent(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM agents WHERE id = ?1", [id])?;
        Ok(())
    }

    // ── Sessioni degli agenti ───────────────────────────────────────────

    /// Crea una sessione con la sua conversazione e il compito come primo messaggio.
    pub fn create_session(&self, agent: &Agent, task: &str) -> Result<AgentSession> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO conversations (title, tools) VALUES (?1, ?2)",
            params![agent.name, serde_json::to_string(&agent.tools)?],
        )?;
        let conversation_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO messages (conversation_id, role, content) VALUES (?1, 'user', ?2)",
            params![conversation_id, task],
        )?;
        tx.execute(
            "INSERT INTO agent_sessions
                 (agent_id, agent_name, definition, conversation_id, task, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                (agent.id != 0).then_some(agent.id),
                agent.name,
                serde_json::to_string(agent)?,
                conversation_id,
                task,
                SessionStatus::Running.as_str()
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        self.session(id)
    }

    pub fn session(&self, id: i64) -> Result<AgentSession> {
        self.conn
            .query_row(
                &format!("SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE id = ?1"),
                [id],
                row_to_session,
            )
            .optional()?
            .ok_or_else(|| Error::msg("Sessione non trovata: forse è stata eliminata."))
    }

    /// Le sessioni, dalla più recente.
    pub fn sessions(&self) -> Result<Vec<AgentSession>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM agent_sessions ORDER BY updated_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map([], row_to_session)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_session_status(
        &self,
        id: i64,
        status: SessionStatus,
        note: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET status = ?2, note = ?3, updated_at = datetime('now')
             WHERE id = ?1",
            params![id, status.as_str(), note],
        )?;
        Ok(())
    }

    /// Le sessioni rimaste "al lavoro" quando l'app si è chiusa diventano interrotte.
    pub fn mark_sessions_interrupted(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET status = 'interrupted'
             WHERE status IN ('running', 'paused', 'waiting')",
            [],
        )?;
        Ok(())
    }

    /// Elimina la sessione insieme alla sua conversazione.
    pub fn delete_session(&self, id: i64) -> Result<()> {
        let session = self.session(id)?;
        self.delete_conversation(session.conversation_id)
    }

    // ── Registro delle azioni ───────────────────────────────────────────

    pub fn add_action(&self, action: &NewAction<'_>) -> Result<()> {
        let mut result: String = action.result.chars().take(ACTION_RESULT_CHARS).collect();
        if result.len() < action.result.len() {
            result.push('…');
        }
        self.conn.execute(
            "INSERT INTO actions
                 (conversation_id, session_id, kind, source, tool, summary, approval, is_error, result)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                action.conversation_id,
                action.session_id,
                if action.session_id.is_some() { "agent" } else { "chat" },
                action.source,
                action.tool,
                action.summary,
                action.approval,
                action.is_error,
                result
            ],
        )?;
        Ok(())
    }

    /// Le azioni più recenti, al massimo `limit`; con `session_id` solo quelle della sessione.
    pub fn actions(&self, limit: u32, session_id: Option<i64>) -> Result<Vec<ActionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, created_at, conversation_id, session_id, source, tool, summary,
                    approval, is_error, result, kind
             FROM actions WHERE ?2 IS NULL OR session_id = ?2
             ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit, session_id], |row| {
            Ok(ActionRecord {
                id: row.get(0)?,
                created_at: row.get(1)?,
                conversation_id: row.get(2)?,
                session_id: row.get(3)?,
                source: row.get(4)?,
                tool: row.get(5)?,
                summary: row.get(6)?,
                approval: row.get(7)?,
                is_error: row.get(8)?,
                result: row.get(9)?,
                kind: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Elimina le azioni più vecchie di `days` giorni; restituisce quante.
    pub fn prune_actions(&self, days: u32) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM actions WHERE created_at < datetime('now', ?1)",
            [format!("-{days} days")],
        )?)
    }

    /// Per quanti giorni si tengono le voci del registro; `0` = per sempre.
    pub fn action_log_days(&self) -> Result<u32> {
        Ok(self
            .setting(crate::SETTING_ACTION_LOG_DAYS)?
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(crate::DEFAULT_ACTION_LOG_DAYS))
    }

    /// Applica la durata scelta dall'utente al registro.
    pub fn prune_actions_by_setting(&self) -> Result<usize> {
        match self.action_log_days()? {
            0 => Ok(0),
            days => self.prune_actions(days),
        }
    }

    /// Svuota il registro delle azioni; restituisce quante voci c'erano.
    pub fn clear_actions(&self) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM actions", [])?)
    }

    // ── Impostazioni ────────────────────────────────────────────────────

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

fn row_to_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentSession> {
    let definition: Option<Agent> = row
        .get::<_, String>(9)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok());
    Ok(AgentSession {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        agent_name: row.get(2)?,
        conversation_id: row.get(3)?,
        task: row.get(4)?,
        status: SessionStatus::parse(&row.get::<_, String>(5)?),
        note: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        definition,
    })
}

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    // Un JSON illeggibile non deve rendere illeggibile la conversazione.
    let tool: ToolData = row
        .get::<_, Option<String>>(9)?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let attachment_data: Vec<Attachment> = row
        .get::<_, Option<String>>(10)?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    Ok(Message {
        tool_calls: tool.calls,
        tool_call_id: tool.call_id,
        tool_name: tool.name,
        is_error: tool.is_error,
        attachments: attachment_data.iter().map(Attachment::info).collect(),
        attachment_data,
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        model: row.get(4)?,
        created_at: row.get(5)?,
        prompt_tokens: row.get::<_, Option<i64>>(6)?.map(|n| n.max(0) as u64),
        completion_tokens: row.get::<_, Option<i64>>(7)?.map(|n| n.max(0) as u64),
        cost_usd: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nuovo_archivio_arriva_all_ultimo_schema() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), MIGRATIONS.len() as i64);
    }

    #[test]
    fn riaprire_un_archivio_non_riapplica_i_passi() {
        let file =
            std::env::temp_dir().join(format!("app-core-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&file);

        drop(Store::open(&file).unwrap());
        let store = Store::open(&file).unwrap();
        assert_eq!(store.schema_version().unwrap(), MIGRATIONS.len() as i64);

        drop(store);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn messaggi_salvati_e_riletti_in_ordine() {
        let store = Store::open_in_memory().unwrap();
        let conv = store.create_conversation("prova").unwrap();
        store.add_message(conv.id, "user", "ciao", None).unwrap();
        store
            .add_message(conv.id, "assistant", "salve", Some("modello"))
            .unwrap();

        let messages = store.messages(conv.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "ciao");
        assert_eq!(messages[1].model.as_deref(), Some("modello"));
    }

    #[test]
    fn eliminare_una_conversazione_elimina_i_suoi_messaggi() {
        let store = Store::open_in_memory().unwrap();
        let conv = store.create_conversation("prova").unwrap();
        store.add_message(conv.id, "user", "ciao", None).unwrap();
        store.delete_conversation(conv.id).unwrap();

        assert!(store.list_conversations("").unwrap().is_empty());
        let rimasti: i64 = store
            .conn
            .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rimasti, 0);
    }

    #[test]
    fn un_archivio_vecchio_viene_aggiornato_senza_perdere_i_messaggi() {
        // Archivio fermo al secondo passo, come quello di chi ha usato una versione precedente.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch(MIGRATIONS[1]).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations (id, title) VALUES (1, 'vecchia');
             INSERT INTO messages (conversation_id, role, content) VALUES (1, 'user', 'ciao');",
        )
        .unwrap();

        let store = Store::init(conn).unwrap();
        assert_eq!(store.schema_version().unwrap(), MIGRATIONS.len() as i64);
        let messages = store.messages(1).unwrap();
        assert_eq!(messages[0].content, "ciao");
        assert_eq!(messages[0].cost_usd, None);
    }

    #[test]
    fn il_consumo_di_una_risposta_viene_salvato() {
        let store = Store::open_in_memory().unwrap();
        let conv = store.create_conversation("prova").unwrap();
        let reply = store
            .add_message(conv.id, "assistant", "ecco", Some("modello"))
            .unwrap();
        assert_eq!(reply.prompt_tokens, None);

        let updated = store
            .set_message_usage(reply.id, Some(1200), Some(300), Some(0.0108))
            .unwrap();
        assert_eq!(updated.prompt_tokens, Some(1200));
        assert_eq!(updated.completion_tokens, Some(300));
        assert_eq!(updated.cost_usd, Some(0.0108));
        assert_eq!(store.messages(conv.id).unwrap()[0].cost_usd, Some(0.0108));
    }

    #[test]
    fn la_ricerca_guarda_titoli_e_messaggi() {
        let store = Store::open_in_memory().unwrap();
        let ricette = store.create_conversation("Ricette di pasta").unwrap();
        store
            .add_message(ricette.id, "user", "Come si fa la carbonara?", None)
            .unwrap();
        let codice = store.create_conversation("Aiuto con Python").unwrap();
        store
            .add_message(codice.id, "user", "Sconto del 100% sul totale", None)
            .unwrap();

        let titoli = |query: &str| -> Vec<String> {
            store
                .list_conversations(query)
                .unwrap()
                .into_iter()
                .map(|c| c.title)
                .collect()
        };

        assert_eq!(titoli("").len(), 2);
        assert_eq!(titoli("PASTA"), ["Ricette di pasta"]);
        assert_eq!(titoli("carbonara"), ["Ricette di pasta"]);
        assert_eq!(titoli("100%"), ["Aiuto con Python"]);
        // Il segno % cercato da solo non deve valere "qualsiasi cosa".
        assert_eq!(titoli("%"), ["Aiuto con Python"]);
        assert!(titoli("inesistente").is_empty());
    }

    #[test]
    fn agenti_e_sessioni_si_salvano_e_le_sessioni_non_finiscono_in_chat() {
        let store = Store::open_in_memory().unwrap();
        let chat = store.create_conversation("Chat normale").unwrap();
        let agent = store
            .save_agent(&Agent {
                name: "Revisore".into(),
                folders: vec!["C:\\progetto".into()],
                ..Default::default()
            })
            .unwrap();
        assert!(agent.id > 0);
        assert_eq!(store.agents().unwrap(), std::slice::from_ref(&agent));

        let session = store.create_session(&agent, "Rivedi il codice").unwrap();
        assert_eq!(session.status, SessionStatus::Running);
        assert_eq!(session.definition.as_ref().unwrap().name, "Revisore");
        let messages = store.messages(session.conversation_id).unwrap();
        assert_eq!(messages[0].content, "Rivedi il codice");
        // Gli strumenti della sessione sono quelli dell'agente.
        let tools = store.conversation_tools(session.conversation_id).unwrap();
        assert!(tools.unwrap().contains("\"terminal\":true"));

        // La conversazione della sessione non compare in chat, nemmeno cercandola.
        let ids: Vec<i64> = store
            .list_conversations("")
            .unwrap()
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, [chat.id]);
        assert!(store.list_conversations("Rivedi").unwrap().is_empty());

        // Rimasta "al lavoro" a una chiusura dell'app, diventa interrotta.
        store.mark_sessions_interrupted().unwrap();
        assert_eq!(
            store.session(session.id).unwrap().status,
            SessionStatus::Interrupted
        );

        // Eliminando l'agente la sessione resta, senza collegamento.
        store.delete_agent(agent.id).unwrap();
        let kept = store.session(session.id).unwrap();
        assert_eq!(kept.agent_id, None);
        assert_eq!(kept.agent_name, "Revisore");

        store.delete_session(session.id).unwrap();
        assert!(store.sessions().unwrap().is_empty());
        assert!(store.messages(session.conversation_id).unwrap().is_empty());
    }

    #[test]
    fn il_registro_delle_azioni_sopravvive_alla_conversazione() {
        let store = Store::open_in_memory().unwrap();
        let conv = store.create_conversation("prova").unwrap();
        let long = "r".repeat(1000);
        store
            .add_action(&NewAction {
                conversation_id: conv.id,
                session_id: None,
                source: "prova",
                tool: "write_file",
                summary: "Scrivere il file a.txt",
                approval: "user",
                is_error: false,
                result: &long,
            })
            .unwrap();
        store.delete_conversation(conv.id).unwrap();

        let actions = store.actions(10, None).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].conversation_id, None);
        assert_eq!(actions[0].source, "prova");
        assert_eq!(actions[0].kind, "chat");
        assert!(actions[0].result.chars().count() <= ACTION_RESULT_CHARS + 1);
        assert!(store.actions(10, Some(99)).unwrap().is_empty());
    }

    #[test]
    fn la_pulizia_del_registro_toglie_solo_le_voci_vecchie() {
        let store = Store::open_in_memory().unwrap();
        let conv = store.create_conversation("prova").unwrap();
        for _ in 0..2 {
            store
                .add_action(&NewAction {
                    conversation_id: conv.id,
                    session_id: None,
                    source: "prova",
                    tool: "read_file",
                    summary: "Leggere a.txt",
                    approval: "auto",
                    is_error: false,
                    result: "ok",
                })
                .unwrap();
        }
        // La prima voce risale a 40 giorni fa.
        store
            .conn
            .execute(
                "UPDATE actions SET created_at = datetime('now', '-40 days') WHERE id = 1",
                [],
            )
            .unwrap();

        assert_eq!(store.prune_actions(60).unwrap(), 0);
        assert_eq!(store.prune_actions(30).unwrap(), 1);
        let left = store.actions(10, None).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, 2);

        assert_eq!(store.clear_actions().unwrap(), 1);
        assert!(store.actions(10, None).unwrap().is_empty());
    }

    #[test]
    fn le_impostazioni_si_sovrascrivono() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.setting("lingua").unwrap(), None);
        store.set_setting("lingua", "it").unwrap();
        store.set_setting("lingua", "en").unwrap();
        assert_eq!(store.setting("lingua").unwrap().as_deref(), Some("en"));
    }
}
