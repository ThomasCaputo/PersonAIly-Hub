//! Archivio SQLite: conversazioni, messaggi e, più avanti, agenti e impostazioni.

use std::path::Path;

use rusqlite::Connection;

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
];

/// Accesso all'archivio dell'app.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Apre (o crea) l'archivio nel file indicato e lo porta allo schema più recente.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Archivio temporaneo in memoria, per i test.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> rusqlite::Result<Self> {
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
    pub fn schema_version(&self) -> rusqlite::Result<i64> {
        self.conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
    }
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
    fn eliminare_una_conversazione_elimina_i_suoi_messaggi() {
        let store = Store::open_in_memory().unwrap();
        store
            .conn
            .execute_batch(
                "INSERT INTO conversations (id, title) VALUES (1, 'prova');
                 INSERT INTO messages (conversation_id, role, content) VALUES (1, 'user', 'ciao');
                 DELETE FROM conversations WHERE id = 1;",
            )
            .unwrap();
        let rimasti: i64 = store
            .conn
            .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rimasti, 0);
    }
}
