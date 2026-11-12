//! Archivio SQLite: conversazioni, messaggi, impostazioni.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::Result;

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
];

/// Colonne lette per ogni messaggio, nell'ordine atteso da `row_to_message`.
const MESSAGE_COLUMNS: &str = "id, conversation_id, role, content, model, created_at, \
     prompt_tokens, completion_tokens, cost_usd";

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
        let mut stmt = self.conn.prepare(
            r"SELECT id, title, created_at, updated_at FROM conversations c
              WHERE c.title LIKE ?1 ESCAPE '\'
                 OR EXISTS (SELECT 1 FROM messages m
                            WHERE m.conversation_id = c.id AND m.content LIKE ?1 ESCAPE '\')
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
        self.conn.execute(
            "INSERT INTO messages (conversation_id, role, content, model) VALUES (?1, ?2, ?3, ?4)",
            params![conversation_id, role, content, model],
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

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
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
    fn le_impostazioni_si_sovrascrivono() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.setting("lingua").unwrap(), None);
        store.set_setting("lingua", "it").unwrap();
        store.set_setting("lingua", "en").unwrap();
        assert_eq!(store.setting("lingua").unwrap().as_deref(), Some("en"));
    }
}
