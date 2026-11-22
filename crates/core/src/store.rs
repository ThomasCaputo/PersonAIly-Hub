//! Archivio SQLite: conversazioni, messaggi, impostazioni.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::agents::{Agent, AgentSession, SessionStatus};
use crate::attachments::{Attachment, AttachmentInfo};
use crate::error::{Error, Result};
use crate::snapshots::RestorePoint;
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
    // 6 — indice di ricerca nel testo dei messaggi. Il tokenizzatore `trigram`
    //     trova anche pezzi di parola, come faceva `LIKE '%…%'`, ma senza
    //     rileggere tutti i messaggi a ogni ricerca. Lo tengono aggiornato i trigger.
    "
    CREATE VIRTUAL TABLE messages_fts USING fts5(
        content, content = 'messages', content_rowid = 'id', tokenize = 'trigram'
    );
    INSERT INTO messages_fts (messages_fts) VALUES ('rebuild');

    CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
        INSERT INTO messages_fts (rowid, content) VALUES (new.id, new.content);
    END;
    CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
        INSERT INTO messages_fts (messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
    END;
    CREATE TRIGGER messages_fts_update AFTER UPDATE OF content ON messages BEGIN
        INSERT INTO messages_fts (messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
        INSERT INTO messages_fts (rowid, content) VALUES (new.id, new.content);
    END;
    ",
    // 7 — rami della conversazione. Ogni messaggio ricorda quello a cui
    //     risponde (`parent_id`); `active` segna il ramo che si vede e che va
    //     al modello. Modificare un messaggio non cancella più ciò che segue:
    //     lo mette da parte. Lungo un ramo gli id crescono sempre (un messaggio
    //     nasce dopo il suo genitore), quindi il ramo attivo letto `ORDER BY id`
    //     è nell'ordine giusto.
    "
    ALTER TABLE messages ADD COLUMN parent_id INTEGER REFERENCES messages(id) ON DELETE CASCADE;
    ALTER TABLE messages ADD COLUMN active INTEGER NOT NULL DEFAULT 1;
    UPDATE messages SET parent_id = (
        SELECT MAX(p.id) FROM messages p
        WHERE p.conversation_id = messages.conversation_id AND p.id < messages.id
    );
    CREATE INDEX messages_active ON messages (conversation_id, active, id);
    CREATE INDEX messages_by_parent ON messages (parent_id);
    ",
    // 8 — punti di ripristino delle sessioni degli agenti (JSON)
    "
    ALTER TABLE agent_sessions ADD COLUMN restore_points TEXT;
    ",
    // 9 — sessioni avviate da un altro agente (AG-8)
    "
    ALTER TABLE agent_sessions ADD COLUMN parent_session_id INTEGER
        REFERENCES agent_sessions(id) ON DELETE SET NULL;
    ",
    // 10 — documenti consultabili (RAG): file indicizzati e loro pezzi, con il
    //      vettore dell'embedding (f32 little-endian)
    "
    CREATE TABLE rag_files (
        id        INTEGER PRIMARY KEY,
        folder    TEXT    NOT NULL,
        path      TEXT    NOT NULL UNIQUE,
        modified  INTEGER NOT NULL,
        size      INTEGER NOT NULL
    );

    CREATE TABLE rag_chunks (
        id         INTEGER PRIMARY KEY,
        file_id    INTEGER NOT NULL REFERENCES rag_files(id) ON DELETE CASCADE,
        position   INTEGER NOT NULL,
        text       TEXT    NOT NULL,
        embedding  BLOB    NOT NULL
    );

    CREATE INDEX rag_chunks_by_file ON rag_chunks (file_id);
    ",
    // 11 — conversazioni temporanee (le colonne del confronto tra modelli):
    //      non si vedono nella cronologia finché l'utente non le salva, e
    //      si cancellano a ogni avvio
    "
    ALTER TABLE conversations ADD COLUMN temporary INTEGER NOT NULL DEFAULT 0;
    ",
];

/// Caratteri del risultato conservati nel registro delle azioni.
const ACTION_RESULT_CHARS: usize = 400;

/// Colonne lette per ogni sessione, nell'ordine atteso da `row_to_session`.
const SESSION_COLUMNS: &str = "id, agent_id, agent_name, conversation_id, task, status, note, \
     created_at, updated_at, definition, restore_points, parent_session_id";

/// Una voce del registro delle azioni (SI-6).
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
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
     prompt_tokens, completion_tokens, cost_usd, tool_data, attachments, parent_id";

/// Come [`MESSAGE_COLUMNS`], ma degli allegati solo nome e tipo: un'immagine
/// in base64 pesa megabyte, e l'interfaccia non la mostra.
const MESSAGE_COLUMNS_LIGHT: &str = "id, conversation_id, role, content, model, created_at, \
     prompt_tokens, completion_tokens, cost_usd, tool_data, \
     (SELECT json_group_array(json_object('name', json_extract(value, '$.name'), \
                                          'kind', json_extract(value, '$.kind'))) \
      FROM json_each(attachments)), parent_id";

/// Sotto questa lunghezza l'indice a trigrammi non serve: si cerca con `LIKE`.
const MIN_INDEXED_QUERY_CHARS: usize = 3;

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

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
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
    /// Il messaggio a cui risponde; `None` per il primo.
    pub parent_id: Option<i64>,
    /// Le versioni di questo messaggio nei vari rami (lui compreso), dalla più
    /// vecchia. Vuoto se ce n'è una sola. Solo nei messaggi da mostrare.
    pub siblings: Vec<i64>,
}

/// Un messaggio di qualunque ramo, per l'esportazione completa in JSON.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeMessage {
    #[serde(flatten)]
    pub message: Message,
    /// Fa parte del ramo che si vede.
    pub active: bool,
}

/// Un messaggio visto solo come punto dell'albero dei rami.
struct Node {
    id: i64,
    parent_id: Option<i64>,
    active: bool,
}

/// Lo schema più recente: il numero di passi di [`MIGRATIONS`].
pub const LATEST_SCHEMA: i64 = MIGRATIONS.len() as i64;

/// Quante copie "prima dell'aggiornamento" si tengono in `backups/`.
const UPGRADE_BACKUPS_KEPT: usize = 3;

/// Lo schema di un archivio su disco, senza modificarlo.
pub fn schema_of(path: &Path) -> Result<i64> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let ok: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if ok != "ok" {
        return Err(Error::msg(format!("archivio rovinato: {ok}")));
    }
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Accesso all'archivio dell'app.
pub struct Store {
    conn: Connection,
}

/// Tiene solo le copie più recenti fatte prima di un aggiornamento.
fn prune_upgrade_backups(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut copies: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("data-schema") && n.ends_with(".sqlite"))
        })
        .collect();
    // Il nome finisce con data e ora: l'ordine alfabetico è quello cronologico.
    copies.sort_by_key(|p| p.file_name().map(|n| n.to_os_string()));
    let excess = copies.len().saturating_sub(UPGRADE_BACKUPS_KEPT);
    for old in &copies[..excess] {
        let _ = std::fs::remove_file(old);
    }
}

impl Store {
    /// Apre (o crea) l'archivio nel file indicato e lo porta allo schema più recente.
    ///
    /// Se l'archivio esiste e va aggiornato, prima se ne fa una copia in
    /// `backups/` accanto al file: un passo di aggiornamento sbagliato non deve
    /// costare all'utente le sue conversazioni.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if current > 0
            && current < LATEST_SCHEMA
            && let Some(dir) = path.parent().map(|p| p.join("backups"))
        {
            std::fs::create_dir_all(&dir)?;
            let copy = dir.join(format!(
                "data-schema{current}-{}.sqlite",
                crate::log::file_stamp()
            ));
            conn.execute("VACUUM INTO ?1", [copy.to_string_lossy()])?;
            crate::log::info(format!(
                "archivio dallo schema {current} al {LATEST_SCHEMA}: copia in {}",
                copy.display()
            ));
            prune_upgrade_backups(&dir);
        }
        Self::init(conn)
    }

    /// Copia coerente dell'archivio in un file nuovo (anche mentre è in uso).
    pub fn vacuum_into(&self, dest: &Path) -> Result<()> {
        if dest.exists() {
            std::fs::remove_file(dest)?;
        }
        self.conn
            .execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
        Ok(())
    }

    /// Archivio temporaneo in memoria, per i test.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        // Scritture più rapide e letture che non aspettano le scritture.
        // Un archivio in memoria non ha un registro su disco: lì non serve.
        if conn.path().is_some_and(|p| !p.is_empty()) {
            let _: String =
                conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
        }
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", index as i64 + 1)?;
            tx.commit()?;
        }

        Ok(Self { conn })
    }

    /// Applica solo i primi `steps` passi: per provare l'aggiornamento di un
    /// archivio creato da una versione precedente.
    #[cfg(test)]
    fn open_at_schema(conn: Connection, steps: usize) -> Result<Connection> {
        for (index, sql) in MIGRATIONS.iter().enumerate().take(steps) {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", index as i64 + 1)?;
        }
        Ok(conn)
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

    /// Una conversazione che non compare nella cronologia finché non la si
    /// tiene con [`Store::keep_conversation`].
    pub fn create_temporary_conversation(&self, title: &str) -> Result<Conversation> {
        self.conn.execute(
            "INSERT INTO conversations (title, temporary) VALUES (?1, 1)",
            [title],
        )?;
        self.conversation(self.conn.last_insert_rowid())
    }

    /// Rende normale una conversazione temporanea: entra nella cronologia, in
    /// cima.
    pub fn keep_conversation(&self, id: i64) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE conversations SET temporary = 0, updated_at = datetime('now') WHERE id = ?1",
            [id],
        )?;
        if changed == 0 {
            return Err(Error::msg("Conversazione non trovata."));
        }
        Ok(())
    }

    /// Cancella le conversazioni temporanee rimaste (all'avvio: quelle di un
    /// confronto non salvato prima di chiudere l'app).
    pub fn delete_temporary_conversations(&self) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM conversations WHERE temporary = 1", [])?)
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
        let query = query.trim();
        // `%` e `_` sono caratteri jolly di LIKE: se l'utente li cerca vanno presi alla lettera.
        let escaped = query
            .replace('\\', r"\\")
            .replace('%', r"\%")
            .replace('_', r"\_");
        let pattern = format!("%{escaped}%");
        // Nel testo dei messaggi si cerca con l'indice: la frase tra virgolette
        // trova il testo esatto ovunque compaia, anche a metà parola.
        let in_messages = if query.chars().count() >= MIN_INDEXED_QUERY_CHARS {
            r"EXISTS (SELECT 1 FROM messages_fts f JOIN messages m ON m.id = f.rowid
                      WHERE messages_fts MATCH ?2 AND m.conversation_id = c.id)"
        } else {
            r"(?2 IS NOT NULL AND EXISTS (SELECT 1 FROM messages m
                      WHERE m.conversation_id = c.id AND m.content LIKE ?1 ESCAPE '\'))"
        };
        let phrase = format!("\"{}\"", query.replace('"', "\"\""));
        // Le conversazioni delle sessioni degli agenti si vedono nella pagina
        // Agenti; quelle temporanee (confronto non salvato) da nessuna parte.
        let mut stmt = self.conn.prepare(&format!(
            r"SELECT id, title, created_at, updated_at FROM conversations c
              WHERE c.temporary = 0
                AND NOT EXISTS (SELECT 1 FROM agent_sessions s WHERE s.conversation_id = c.id)
                AND (?3 = '' OR c.title LIKE ?1 ESCAPE '\' OR {in_messages})
              ORDER BY updated_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map(params![pattern, phrase, query], row_to_conversation)?;
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

    /// I messaggi con il contenuto degli allegati, per mandarli al modello.
    pub fn messages(&self, conversation_id: i64) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE conversation_id = ?1 AND active = 1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([conversation_id], row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// I messaggi da mostrare: degli allegati solo nome e tipo.
    pub fn messages_for_display(&self, conversation_id: i64) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {MESSAGE_COLUMNS_LIGHT} FROM messages WHERE conversation_id = ?1 AND active = 1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([conversation_id], row_to_message_light)?;
        let mut messages: Vec<Message> = rows.collect::<rusqlite::Result<_>>()?;
        // Le altre versioni di ogni messaggio: stesso genitore, altro ramo.
        let tree = self.tree(conversation_id)?;
        for m in &mut messages {
            let siblings: Vec<i64> = tree
                .iter()
                .filter(|n| n.parent_id == m.parent_id)
                .map(|n| n.id)
                .collect();
            if siblings.len() > 1 {
                m.siblings = siblings;
            }
        }
        Ok(messages)
    }

    /// La struttura dei rami: id, genitore e ramo attivo di ogni messaggio,
    /// dal più vecchio.
    fn tree(&self, conversation_id: i64) -> Result<Vec<Node>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, parent_id, active FROM messages WHERE conversation_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([conversation_id], |row| {
            Ok(Node {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                active: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Mostra un altro ramo: quello che passa per `message_id`. Da lì in giù
    /// si segue ogni volta la versione più recente.
    pub fn switch_branch(&self, conversation_id: i64, message_id: i64) -> Result<()> {
        let tree = self.tree(conversation_id)?;
        let target = tree
            .iter()
            .find(|n| n.id == message_id)
            .ok_or_else(|| Error::msg("Messaggio non trovato: forse è stato eliminato."))?;
        if target.active {
            return Ok(());
        }
        let mut path = vec![target.id];
        let mut last = target.id;
        while let Some(child) = tree.iter().rev().find(|n| n.parent_id == Some(last)) {
            path.push(child.id);
            last = child.id;
        }
        let tx = self.conn.unchecked_transaction()?;
        // Il ramo attivo da qui in giù: tutti i messaggi attivi dopo il genitore.
        tx.execute(
            "UPDATE messages SET active = 0
             WHERE conversation_id = ?1 AND active = 1 AND id > ?2",
            params![conversation_id, target.parent_id.unwrap_or(0)],
        )?;
        for id in path {
            tx.execute("UPDATE messages SET active = 1 WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Tutti i messaggi di tutti i rami, per l'esportazione in JSON.
    pub fn message_tree(&self, conversation_id: i64) -> Result<Vec<TreeMessage>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {MESSAGE_COLUMNS_LIGHT}, active FROM messages WHERE conversation_id = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([conversation_id], |row| {
            Ok(TreeMessage {
                message: row_to_message_light(row)?,
                active: row.get(12)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Costo stimato delle risposte dal primo del mese (ora UTC), ovunque:
    /// chat, agenti, rami messi da parte.
    pub fn spent_this_month(&self) -> Result<f64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0) FROM messages
             WHERE created_at >= datetime('now', 'start of month')",
            [],
            |row| row.get(0),
        )?)
    }

    /// Costo stimato delle risposte di una conversazione, rami compresi.
    pub fn spent_in_conversation(&self, conversation_id: i64) -> Result<f64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0) FROM messages WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )?)
    }

    /// La conversazione non ha ancora messaggi.
    pub fn has_no_messages(&self, conversation_id: i64) -> Result<bool> {
        Ok(!self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM messages WHERE conversation_id = ?1)",
            [conversation_id],
            |row| row.get::<_, bool>(0),
        )?)
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
        // Risponde all'ultimo messaggio del ramo che si vede.
        self.conn.execute(
            "INSERT INTO messages (conversation_id, role, content, model, tool_data, attachments, parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT MAX(id) FROM messages
                                              WHERE conversation_id = ?1 AND active = 1))",
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

    /// Un messaggio di una conversazione, con il contenuto degli allegati.
    pub fn message_in(&self, conversation_id: i64, id: i64) -> Result<Message> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1 AND conversation_id = ?2"
                ),
                params![id, conversation_id],
                row_to_message,
            )
            .optional()?
            .ok_or_else(|| Error::msg("Messaggio non trovato: forse è stato eliminato."))
    }

    /// Mette da parte un messaggio e tutti quelli che lo seguono nel ramo che
    /// si vede: restano un ramo da riaprire. Il prossimo messaggio diventa
    /// un'altra versione di `id`.
    pub fn branch_from(&self, conversation_id: i64, id: i64) -> Result<usize> {
        let deleted = self.conn.execute(
            "UPDATE messages SET active = 0 WHERE conversation_id = ?1 AND active = 1 AND id >= ?2",
            params![conversation_id, id],
        )?;
        self.conn.execute(
            "UPDATE conversations SET updated_at = datetime('now') WHERE id = ?1",
            [conversation_id],
        )?;
        Ok(deleted)
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
    /// Segna che la sessione è stata avviata da un'altra (AG-8).
    pub fn set_session_parent(&self, id: i64, parent: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET parent_session_id = ?2 WHERE id = ?1",
            params![id, parent],
        )?;
        Ok(())
    }

    /// Salva le foto delle cartelle scattate prima che l'agente lavorasse.
    pub fn set_session_restore_points(&self, id: i64, points: &[RestorePoint]) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET restore_points = ?2 WHERE id = ?1",
            params![id, serde_json::to_string(points)?],
        )?;
        Ok(())
    }

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

    // ── Documenti consultabili (RAG) ────────────────────────────────────

    /// Data di modifica e dimensione con cui un file è stato indicizzato.
    pub fn rag_stamp(&self, path: &str) -> Result<Option<(i64, i64)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT modified, size FROM rag_files WHERE path = ?1",
                [path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    /// Sostituisce i pezzi di un file (tutti insieme, o nessuno).
    pub fn rag_replace_file(
        &self,
        folder: &str,
        path: &str,
        modified: i64,
        size: i64,
        chunks: &[(String, Vec<f32>)],
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM rag_files WHERE path = ?1", [path])?;
        tx.execute(
            "INSERT INTO rag_files (folder, path, modified, size) VALUES (?1, ?2, ?3, ?4)",
            params![folder, path, modified, size],
        )?;
        let file_id = tx.last_insert_rowid();
        {
            let mut insert = tx.prepare(
                "INSERT INTO rag_chunks (file_id, position, text, embedding) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (position, (text, vector)) in chunks.iter().enumerate() {
                insert.execute(params![
                    file_id,
                    position as i64,
                    text,
                    crate::rag::to_bytes(vector)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Toglie dall'indice i file di `folder` che non ci sono più, e quelli
    /// delle cartelle non più scelte (`folders`).
    pub fn rag_prune(
        &self,
        folders: &[String],
        existing: &std::collections::HashSet<String>,
    ) -> Result<usize> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, folder, path FROM rag_files")?;
        let rows: Vec<(i64, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut removed = 0;
        for (id, folder, path) in rows {
            if !folders.contains(&folder) || !existing.contains(&path) {
                removed += self
                    .conn
                    .execute("DELETE FROM rag_files WHERE id = ?1", [id])?;
            }
        }
        Ok(removed)
    }

    /// Toglie dall'indice i file delle cartelle che non sono in `folders`:
    /// cartelle revocate ai modelli o tolte dai documenti consultabili.
    pub fn rag_keep_folders(&self, folders: &[String]) -> Result<usize> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT folder FROM rag_files")?;
        let indexed: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut removed = 0;
        for folder in indexed.iter().filter(|f| !folders.contains(f)) {
            removed += self
                .conn
                .execute("DELETE FROM rag_files WHERE folder = ?1", [folder])?;
        }
        Ok(removed)
    }

    /// Svuota l'indice (per esempio cambiando modello di embedding: i vettori
    /// di modelli diversi non si confrontano).
    pub fn rag_clear(&self) -> Result<()> {
        self.conn.execute("DELETE FROM rag_files", [])?;
        Ok(())
    }

    /// File e pezzi nell'indice.
    pub fn rag_counts(&self) -> Result<(u32, u32)> {
        Ok(self.conn.query_row(
            "SELECT (SELECT COUNT(*) FROM rag_files), (SELECT COUNT(*) FROM rag_chunks)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }

    /// Tutti i vettori dell'indice con la cartella del loro file, per
    /// costruire l'indice in memoria ([`crate::rag::VectorIndex`]).
    pub fn rag_vectors(&self) -> Result<Vec<(i64, String, Vec<f32>)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT c.id, f.folder, c.embedding FROM rag_chunks c JOIN rag_files f ON f.id = c.file_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                crate::rag::from_bytes(&row.get::<_, Vec<u8>>(2)?),
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// File e testo di alcuni pezzi, nell'ordine di `ids`; quelli non più
    /// nell'indice si saltano.
    pub fn rag_chunk_texts(&self, ids: &[i64]) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.path, c.text FROM rag_chunks c JOIN rag_files f ON f.id = c.file_id WHERE c.id = ?1",
        )?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(row) = stmt
                .query_row([id], |row| Ok((row.get(0)?, row.get(1)?)))
                .optional()?
            {
                out.push(row);
            }
        }
        Ok(out)
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
        restore_points: row
            .get::<_, Option<String>>(10)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
        parent_session_id: row.get(11)?,
    })
}

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    // Un JSON illeggibile non deve rendere illeggibile la conversazione.
    let attachment_data: Vec<Attachment> = row
        .get::<_, Option<String>>(10)?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let attachments = attachment_data.iter().map(Attachment::info).collect();
    build_message(row, attachments, attachment_data)
}

/// Per le righe lette con [`MESSAGE_COLUMNS_LIGHT`].
fn row_to_message_light(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let attachments: Vec<AttachmentInfo> = row
        .get::<_, Option<String>>(10)?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    build_message(row, attachments, Vec::new())
}

fn build_message(
    row: &rusqlite::Row<'_>,
    attachments: Vec<AttachmentInfo>,
    attachment_data: Vec<Attachment>,
) -> rusqlite::Result<Message> {
    let tool: ToolData = row
        .get::<_, Option<String>>(9)?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    Ok(Message {
        tool_calls: tool.calls,
        tool_call_id: tool.call_id,
        tool_name: tool.name,
        is_error: tool.is_error,
        attachments,
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
        parent_id: row.get(11)?,
        siblings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quanti passi aveva lo schema nella versione 0.1.0, la prima pubblicata.
    const SCHEMA_0_1_0: usize = 6;

    /// Un archivio scritto dalla 0.1.0, con i dati nella forma di allora,
    /// arriva all'ultimo schema senza perdere nulla e resta utilizzabile.
    #[test]
    fn archivio_della_0_1_0_si_aggiorna() {
        let dir = std::env::temp_dir().join(format!("app-core-v010-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.sqlite");
        let conn = Store::open_at_schema(Connection::open(&path).unwrap(), SCHEMA_0_1_0).unwrap();
        conn.execute_batch(
            r#"
            INSERT INTO conversations (id, title, tools) VALUES
                (1, 'Ricetta', '{"files":true,"web":false,"terminal":false,"permission":"ask"}'),
                (2, 'Sessione', NULL);
            INSERT INTO messages (conversation_id, role, content, model, prompt_tokens, completion_tokens, cost_usd) VALUES
                (1, 'user', 'Come si fa la carbonara?', NULL, NULL, NULL, NULL),
                (1, 'assistant', 'Con guanciale e pecorino.', 'qwen', 12, 8, 0.001),
                (1, 'user', 'E per quattro persone?', NULL, NULL, NULL, NULL),
                (2, 'user', 'Crea a.txt', NULL, NULL, NULL, NULL);
            INSERT INTO settings (key, value) VALUES ('language', '"it"');
            INSERT INTO agents (id, name, definition) VALUES (1, 'Scrittore',
                '{"id":1,"name":"Scrittore","description":"","instructions":"","model":{"kind":"local"},"folders":["C:\\prove"],"tools":{"files":true,"web":false,"terminal":true,"permission":"edits"},"mcpServers":[],"maxSteps":50}');
            INSERT INTO agent_sessions (agent_id, agent_name, definition, conversation_id, task, status) VALUES
                (1, 'Scrittore', (SELECT definition FROM agents WHERE id = 1), 2, 'Crea a.txt', 'done');
            INSERT INTO actions (conversation_id, kind, source, tool, summary, approval, is_error, result) VALUES
                (1, 'chat', 'Ricetta', 'read_file', 'Leggere il file a.txt', 'auto', 0, 'ciao');
            "#,
        )
        .unwrap();
        drop(conn);

        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA);
        // Prima dell'aggiornamento è stata fatta una copia.
        let copies: Vec<_> = std::fs::read_dir(dir.join("backups"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(schema_of(&copies[0].path()).unwrap(), SCHEMA_0_1_0 as i64);

        // I messaggi di allora formano un ramo unico, nell'ordine giusto.
        let messages = store.messages(1).unwrap();
        let texts: Vec<&str> = messages.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Come si fa la carbonara?",
                "Con guanciale e pecorino.",
                "E per quattro persone?"
            ]
        );
        assert_eq!(store.list_conversations("carbonara").unwrap().len(), 1);
        assert_eq!(
            store.setting("language").unwrap().as_deref(),
            Some("\"it\"")
        );

        // Agente e sessione si leggono con le strutture di oggi.
        let agent = store.agent(1).unwrap();
        assert_eq!(agent.name, "Scrittore");
        assert_eq!(agent.folders, [r"C:\prove"]);
        assert!(agent.tools.terminal);
        let session = store.session(1).unwrap();
        assert_eq!(session.status, SessionStatus::Done);

        // E si continua a scrivere: un nuovo messaggio segue l'ultimo.
        store
            .add_message(1, "assistant", "Raddoppia le dosi.", None)
            .unwrap();
        assert_eq!(store.messages(1).unwrap().len(), 4);
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
    fn le_conversazioni_temporanee_restano_fuori_finche_non_si_salvano() {
        let store = Store::open_in_memory().unwrap();
        let normale = store.create_conversation("normale").unwrap();
        let tenuta = store.create_temporary_conversation("confronto A").unwrap();
        let buttata = store.create_temporary_conversation("confronto B").unwrap();
        store
            .add_message(tenuta.id, "user", "carbonara", None)
            .unwrap();
        store
            .add_message(buttata.id, "user", "carbonara", None)
            .unwrap();

        // Né nella cronologia né nella ricerca.
        let ids = |q: &str| -> Vec<i64> {
            store
                .list_conversations(q)
                .unwrap()
                .iter()
                .map(|c| c.id)
                .collect()
        };
        assert_eq!(ids(""), [normale.id]);
        assert!(ids("carbonara").is_empty());

        // Salvata, entra in cima; le altre si cancellano all'avvio.
        store.keep_conversation(tenuta.id).unwrap();
        assert_eq!(ids("carbonara"), [tenuta.id]);
        assert_eq!(store.delete_temporary_conversations().unwrap(), 1);
        assert!(store.conversation(buttata.id).is_err());
        assert_eq!(ids("").len(), 2);
        assert!(store.keep_conversation(buttata.id).is_err());
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
        // L'indice trova anche pezzi di parola, senza badare alle maiuscole.
        assert_eq!(titoli("RBONA"), ["Ricette di pasta"]);
        assert_eq!(titoli("del 100"), ["Aiuto con Python"]);
        // Le virgolette cercate non rompono la ricerca.
        assert!(titoli("\"carbonara").is_empty());

        // Un messaggio eliminato (anche con la sua conversazione) esce dall'indice.
        store.delete_conversation(ricette.id).unwrap();
        assert!(titoli("carbonara").is_empty());
        let fts_rows: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH '\"carbonara\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fts_rows, 0);
    }

    #[test]
    fn i_messaggi_da_mostrare_non_portano_il_contenuto_degli_allegati() {
        use crate::attachments::AttachmentKind;
        let store = Store::open_in_memory().unwrap();
        let conv = store.create_conversation("allegati").unwrap();
        assert!(store.has_no_messages(conv.id).unwrap());
        let image = Attachment {
            name: "foto.png".into(),
            kind: AttachmentKind::Image,
            data: "QUFB".repeat(1000),
            media_type: Some("image/png".into()),
        };
        store
            .add_message_with(conv.id, "user", "guarda", None, None, &[image])
            .unwrap();
        store
            .add_message(conv.id, "assistant", "vista", None)
            .unwrap();
        assert!(!store.has_no_messages(conv.id).unwrap());

        let shown = store.messages_for_display(conv.id).unwrap();
        assert_eq!(shown[0].attachments[0].name, "foto.png");
        assert_eq!(shown[0].attachments[0].kind, AttachmentKind::Image);
        assert!(shown[0].attachment_data.is_empty());
        assert!(shown[1].attachments.is_empty());
        let full = store.messages(conv.id).unwrap();
        assert_eq!(full[0].attachment_data[0].data.len(), 4000);
    }

    #[test]
    fn un_archivio_su_disco_usa_il_registro_wal() {
        let dir = std::env::temp_dir().join(format!("archivio-wal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.sqlite");
        {
            let store = Store::open(&path).unwrap();
            let mode: String = store
                .conn
                .pragma_query_value(None, "journal_mode", |row| row.get(0))
                .unwrap();
            assert_eq!(mode.to_lowercase(), "wal");
        }
        let _ = std::fs::remove_dir_all(&dir);
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

    fn contents(store: &Store, conversation_id: i64) -> Vec<String> {
        store
            .messages(conversation_id)
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .collect()
    }

    #[test]
    fn modificare_un_messaggio_apre_un_ramo() {
        let store = Store::open_in_memory().unwrap();
        let c = store.create_conversation("Rami").unwrap().id;
        let q1 = store.add_message(c, "user", "domanda", None).unwrap();
        let a1 = store.add_message(c, "assistant", "risposta", None).unwrap();
        assert_eq!(a1.parent_id, Some(q1.id));

        // Modifica: il vecchio messaggio e la risposta restano da parte.
        store.branch_from(c, q1.id).unwrap();
        let q2 = store.add_message(c, "user", "domanda nuova", None).unwrap();
        store
            .add_message(c, "assistant", "risposta nuova", None)
            .unwrap();
        assert_eq!(q2.parent_id, None);
        assert_eq!(contents(&store, c), ["domanda nuova", "risposta nuova"]);

        let shown = store.messages_for_display(c).unwrap();
        assert_eq!(shown[0].siblings, [q1.id, q2.id]);
        assert!(shown[1].siblings.is_empty());

        // Si torna al primo ramo, con la sua risposta.
        store.switch_branch(c, q1.id).unwrap();
        assert_eq!(contents(&store, c), ["domanda", "risposta"]);
        store.switch_branch(c, q2.id).unwrap();
        assert_eq!(contents(&store, c), ["domanda nuova", "risposta nuova"]);

        // Tutti e quattro restano nell'esportazione completa.
        let tree = store.message_tree(c).unwrap();
        assert_eq!(tree.len(), 4);
        assert_eq!(tree.iter().filter(|m| m.active).count(), 2);
    }

    #[test]
    fn i_rami_si_annidano() {
        let store = Store::open_in_memory().unwrap();
        let c = store.create_conversation("Rami").unwrap().id;
        store.add_message(c, "user", "a", None).unwrap();
        let b = store.add_message(c, "assistant", "b", None).unwrap();
        let x = store.add_message(c, "user", "x", None).unwrap();
        store.add_message(c, "assistant", "x1", None).unwrap();
        // Altra risposta a "x".
        store.branch_from(c, x.id + 1).unwrap();
        store.add_message(c, "assistant", "x2", None).unwrap();
        // Altra versione di "x".
        store.branch_from(c, x.id).unwrap();
        let y = store.add_message(c, "user", "y", None).unwrap();
        assert_eq!(y.parent_id, Some(b.id));
        assert_eq!(contents(&store, c), ["a", "b", "y"]);

        // Tornando a "x" si segue la sua risposta più recente.
        store.switch_branch(c, x.id).unwrap();
        assert_eq!(contents(&store, c), ["a", "b", "x", "x2"]);
        let x1 = x.id + 1;
        store.switch_branch(c, x1).unwrap();
        assert_eq!(contents(&store, c), ["a", "b", "x", "x1"]);
        // Un messaggio già visibile non cambia nulla.
        store.switch_branch(c, b.id).unwrap();
        assert_eq!(contents(&store, c), ["a", "b", "x", "x1"]);
    }

    #[test]
    fn i_vecchi_messaggi_ricevono_il_genitore() {
        let conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..6] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 6).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations (id, title) VALUES (1, 'vecchia');
             INSERT INTO messages (conversation_id, role, content) VALUES (1, 'user', 'uno');
             INSERT INTO messages (conversation_id, role, content) VALUES (1, 'assistant', 'due');",
        )
        .unwrap();
        let store = Store::init(conn).unwrap();
        let messages = store.messages(1).unwrap();
        assert_eq!(messages[0].parent_id, None);
        assert_eq!(messages[1].parent_id, Some(messages[0].id));
    }
}
