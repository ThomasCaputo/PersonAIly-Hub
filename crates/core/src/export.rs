//! Esportazione di una conversazione in Markdown (CH-9) o in JSON.

use serde::Serialize;

use crate::error::Result;
use crate::store::{Conversation, Message, TreeMessage};

/// Versione del formato JSON: cresce se cambia in modo incompatibile.
const JSON_FORMAT: u32 = 1;

/// La conversazione in JSON, con tutti i messaggi di tutti i rami: ognuno ha
/// `parentId` (il messaggio a cui risponde) e `active` (fa parte del ramo
/// che si vede). Degli allegati solo nome e tipo.
pub fn json(conversation: &Conversation, messages: &[TreeMessage]) -> Result<String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Export<'a> {
        format: &'static str,
        version: u32,
        conversation: &'a Conversation,
        messages: &'a [TreeMessage],
    }
    Ok(serde_json::to_string_pretty(&Export {
        format: "personaily-hub-conversation",
        version: JSON_FORMAT,
        conversation,
        messages,
    })?)
}

/// Caratteri del risultato di uno strumento riportati nell'esportazione:
/// una pagina web letta intera renderebbe il file illeggibile.
const TOOL_RESULT_CHARS: usize = 2000;

/// Il testo Markdown della conversazione, con le intestazioni in italiano o
/// in inglese.
pub fn markdown(conversation: &Conversation, messages: &[Message], english: bool) -> String {
    let label = |it: &'static str, en: &'static str| if english { en } else { it };
    let mut out = format!("# {}\n\n", conversation.title.trim());
    out.push_str(&format!(
        "_{} · {} {}_\n",
        label(
            "Conversazione esportata da PersonAIly Hub",
            "Conversation exported from PersonAIly Hub"
        ),
        label("iniziata il", "started on"),
        conversation.created_at
    ));

    for m in messages {
        match m.role.as_str() {
            "user" => {
                out.push_str(&format!("\n## {}\n\n", label("Tu", "You")));
                if !m.attachments.is_empty() {
                    let names: Vec<&str> = m.attachments.iter().map(|a| a.name.as_str()).collect();
                    out.push_str(&format!(
                        "_{}: {}_\n\n",
                        label("Allegati", "Attachments"),
                        names.join(", ")
                    ));
                }
                out.push_str(m.content.trim_end());
                out.push('\n');
            }
            "assistant" => {
                if m.content.trim().is_empty() && m.tool_calls.is_empty() {
                    continue;
                }
                let model = m.model.as_deref().unwrap_or(label("Modello", "Model"));
                out.push_str(&format!("\n## {model}\n\n"));
                if !m.content.trim().is_empty() {
                    out.push_str(m.content.trim_end());
                    out.push('\n');
                }
                if !m.tool_calls.is_empty() {
                    let names: Vec<&str> = m.tool_calls.iter().map(|c| c.name.as_str()).collect();
                    if !m.content.trim().is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&format!(
                        "_{}: {}_\n",
                        label("Strumenti usati", "Tools used"),
                        names.join(", ")
                    ));
                }
            }
            "tool" => {
                let name = m.tool_name.as_deref().unwrap_or("?");
                let title = if m.is_error {
                    format!("{} {name}", label("Errore di", "Error from"))
                } else {
                    format!("{} {name}", label("Risultato di", "Result of"))
                };
                let mut content: String = m.content.chars().take(TOOL_RESULT_CHARS).collect();
                if m.content.chars().count() > TOOL_RESULT_CHARS {
                    content.push('…');
                }
                let fence = fence_for(&content);
                out.push_str(&format!(
                    "\n<details><summary>{title}</summary>\n\n{fence}\n{}\n{fence}\n\n</details>\n",
                    content.trim_end()
                ));
            }
            _ => {}
        }
    }
    out
}

/// Una recinzione di codice più lunga di qualunque fila di apici inversi nel
/// testo, così il contenuto non la può chiudere.
fn fence_for(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(longest.max(2) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn esporta_messaggi_e_strumenti() {
        let store = Store::open_in_memory().unwrap();
        let conversation = store.create_conversation("Ricetta").unwrap();
        store
            .add_message(conversation.id, "user", "Come si fa la carbonara?", None)
            .unwrap();
        store
            .add_message(
                conversation.id,
                "assistant",
                "Con uova e guanciale.",
                Some("Qwen"),
            )
            .unwrap();
        store
            .add_message(conversation.id, "tool", "```codice```", None)
            .unwrap();
        let messages = store.messages_for_display(conversation.id).unwrap();

        let it = markdown(&conversation, &messages, false);
        assert!(it.starts_with("# Ricetta\n"));
        assert!(it.contains("\n## Tu\n\nCome si fa la carbonara?\n"));
        assert!(it.contains("\n## Qwen\n\nCon uova e guanciale.\n"));
        // Il risultato contiene tre apici: la recinzione ne ha quattro.
        assert!(it.contains("````\n```codice```\n````"));

        let en = markdown(&conversation, &messages, true);
        assert!(en.contains("\n## You\n"));
    }

    #[test]
    fn recinzione_sempre_piu_lunga_del_contenuto() {
        assert_eq!(fence_for("niente"), "```");
        assert_eq!(fence_for("a ```` b"), "`````");
    }
}
