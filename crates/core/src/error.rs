//! Errore unico del nucleo.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("archivio: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("file: {0}")]
    Io(#[from] std::io::Error),
    #[error("rete: {0}")]
    Http(#[from] reqwest::Error),
    #[error("dati non validi: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Msg(String),
}

impl Error {
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Msg(text.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
