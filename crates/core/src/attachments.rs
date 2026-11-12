//! Allegati a un messaggio: file di testo, PDF e immagini scelti dall'utente.
//!
//! Testo e PDF diventano testo dentro il messaggio, quindi funzionano con
//! qualsiasi modello. Le immagini viaggiano come tali e servono un modello
//! che le sappia leggere.

use std::path::Path;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::tools::files;

/// Quanti caratteri di un allegato di testo finiscono nel messaggio.
const MAX_TEXT_CHARS: usize = 150_000;
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Text,
    Image,
}

/// Un allegato come viene salvato con il messaggio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub name: String,
    pub kind: AttachmentKind,
    /// Il testo del file, oppure l'immagine codificata in base64.
    pub data: String,
    /// Tipo dell'immagine (`image/png`…); assente per il testo.
    pub media_type: Option<String>,
}

/// Ciò che l'interfaccia mostra di un allegato: non il contenuto.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub name: String,
    pub kind: AttachmentKind,
}

impl Attachment {
    pub fn info(&self) -> AttachmentInfo {
        AttachmentInfo {
            name: self.name.clone(),
            kind: self.kind,
        }
    }
}

fn image_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

/// Legge dal disco un file scelto dall'utente.
pub fn load(path: &Path) -> Result<Attachment> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| Error::msg("Allegato senza nome."))?;

    if let Some(media_type) = image_type(path) {
        let size = std::fs::metadata(path)?.len();
        if size > MAX_IMAGE_BYTES {
            return Err(Error::msg(format!(
                "{name} è un'immagine troppo grande (massimo 5 MB)."
            )));
        }
        return Ok(Attachment {
            name,
            kind: AttachmentKind::Image,
            data: base64::engine::general_purpose::STANDARD.encode(std::fs::read(path)?),
            media_type: Some(media_type.to_string()),
        });
    }

    let text = files::read_text(path)?;
    if text.trim().is_empty() {
        return Err(Error::msg(format!(
            "{name} non contiene testo leggibile (un PDF fatto di sole immagini?)."
        )));
    }
    Ok(Attachment {
        name,
        kind: AttachmentKind::Text,
        data: files::truncate(text, MAX_TEXT_CHARS),
        media_type: None,
    })
}

/// Il messaggio come lo legge il modello: gli allegati di testo prima, poi
/// ciò che ha scritto l'utente.
pub fn with_text_attachments(text: &str, attachments: &[Attachment]) -> String {
    let mut out = String::new();
    for a in attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Text)
    {
        out.push_str(&format!(
            "<allegato nome=\"{}\">\n{}\n</allegato>\n\n",
            a.name.replace('"', "'"),
            a.data
        ));
    }
    out.push_str(text);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carica_testo_e_immagini() {
        let dir = std::env::temp_dir().join(format!("allegati-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let note = dir.join("note.md");
        std::fs::write(&note, "# Titolo\nciao").unwrap();
        let text = load(&note).unwrap();
        assert_eq!(text.kind, AttachmentKind::Text);
        assert_eq!(text.data, "# Titolo\nciao");

        let image = dir.join("foto.PNG");
        std::fs::write(&image, [1u8, 2, 3]).unwrap();
        let picture = load(&image).unwrap();
        assert_eq!(picture.kind, AttachmentKind::Image);
        assert_eq!(picture.media_type.as_deref(), Some("image/png"));
        assert_eq!(picture.data, "AQID");

        let binary = dir.join("programma.exe");
        std::fs::write(&binary, [0xffu8, 0x00, 0xfe]).unwrap();
        assert!(load(&binary).is_err());

        let message = with_text_attachments("Riassumi.", &[text, picture]);
        assert_eq!(
            message,
            "<allegato nome=\"note.md\">\n# Titolo\nciao\n</allegato>\n\nRiassumi."
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
