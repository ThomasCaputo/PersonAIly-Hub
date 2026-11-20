//! Documenti personali consultabili (RAG locale, ADR 0017).
//!
//! I file delle cartelle scelte si dividono in pezzi di testo; un modello di
//! embedding GGUF, eseguito da un secondo `llama-server` sulla CPU, trasforma
//! ogni pezzo in un vettore. I vettori stanno nell'archivio SQLite. Per
//! cercare si trasforma anche la domanda e si prendono i pezzi più vicini
//! (somiglianza del coseno): con qualche decina di migliaia di pezzi basta
//! scorrerli tutti.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};
use crate::tools::{Risk, ToolDef};

/// Lo strumento con cui il modello cerca nei documenti.
pub const DOCUMENTS_TOOL: &str = "search_documents";

/// Lunghezza indicativa di un pezzo, in caratteri, e quanto si sovrappone al
/// successivo (perché una frase a cavallo non si perda).
const CHUNK_CHARS: usize = 1200;
const CHUNK_OVERLAP: usize = 200;
/// File più grandi di così non si indicizzano.
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Pezzi restituiti per ogni ricerca.
pub const RESULTS: usize = 6;
/// Pezzi mandati al modello di embedding in una sola richiesta.
pub const EMBED_BATCH: usize = 16;

/// Estensioni dei file che si indicizzano.
const EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "rst", "csv", "tsv", "json", "yaml", "yml", "toml", "xml", "html",
    "htm", "tex", "pdf", "log", "ini", "rs", "py", "js", "ts", "tsx", "jsx", "java", "c", "h",
    "cpp", "cs", "go", "rb", "php", "sql", "sh", "ps1",
];
/// Cartelle che non si esplorano.
const SKIPPED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".idea",
    ".vscode",
];

/// Impostazioni dei documenti consultabili.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", default)]
pub struct RagSettings {
    /// Il modello GGUF che produce gli embedding (per esempio
    /// `nomic-embed-text` o `multilingual-e5`): un file della libreria.
    pub embedding_model: Option<String>,
    /// Le cartelle indicizzate: devono essere tra quelle concesse.
    pub folders: Vec<String>,
}

/// Stato dell'indice, per l'interfaccia.
#[derive(Debug, Clone, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RagStatus {
    pub settings: RagSettings,
    pub files: u32,
    pub chunks: u32,
    /// Un'indicizzazione è in corso.
    pub indexing: bool,
    /// File esaminati e da esaminare nell'indicizzazione in corso (o l'ultima).
    pub done_files: u32,
    pub total_files: u32,
    /// Errore dell'ultima indicizzazione, se c'è stato.
    pub error: Option<String>,
    /// File che non si sono potuti leggere nell'ultima indicizzazione.
    pub skipped: Vec<String>,
}

/// Un pezzo trovato da una ricerca.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub path: String,
    pub text: String,
    pub score: f32,
}

/// Divide un testo in pezzi di circa [`CHUNK_CHARS`] caratteri, tagliando di
/// preferenza tra un paragrafo e l'altro, poi tra una riga e l'altra; pezzi
/// consecutivi si sovrappongono di circa [`CHUNK_OVERLAP`] caratteri.
pub fn chunk_text(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + CHUNK_CHARS).min(chars.len());
        if end < chars.len() {
            // Il punto di taglio migliore nell'ultimo terzo del pezzo.
            let window = &chars[start + CHUNK_CHARS * 2 / 3..end];
            let cut = find_last(window, &['\n', '\n'])
                .or_else(|| window.iter().rposition(|&c| c == '\n'))
                .or_else(|| window.iter().rposition(|&c| c == '.' || c == ' '));
            if let Some(cut) = cut {
                end = start + CHUNK_CHARS * 2 / 3 + cut + 1;
            }
        }
        let chunk: String = chars[start..end].iter().collect();
        if !chunk.trim().is_empty() {
            chunks.push(chunk.trim().to_string());
        }
        if end >= chars.len() {
            break;
        }
        start = end.saturating_sub(CHUNK_OVERLAP).max(start + 1);
    }
    chunks
}

/// Posizione dell'ultima occorrenza di `pattern` (qui: una riga vuota).
fn find_last(haystack: &[char], pattern: &[char]) -> Option<usize> {
    haystack
        .windows(pattern.len())
        .rposition(|w| w == pattern)
        .map(|i| i + pattern.len() - 1)
}

/// Somiglianza del coseno tra due vettori della stessa lunghezza.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Vettore salvato come byte (f32 little-endian) e ritorno.
pub fn to_bytes(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub fn from_bytes(bytes: &[u8]) -> Vec<f32> {
    let (words, _) = bytes.as_chunks::<4>();
    words.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

/// Un file da indicizzare: percorso, data di modifica (secondi) e dimensione.
#[derive(Debug, Clone, PartialEq)]
pub struct FileStamp {
    pub path: PathBuf,
    pub modified: i64,
    pub size: i64,
}

/// I file indicizzabili di una cartella, sottocartelle comprese.
pub fn scan(folder: &Path) -> Vec<FileStamp> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            // I collegamenti non si seguono: potrebbero uscire dalla cartella.
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                if (!name.starts_with('.') || name == ".github")
                    && !SKIPPED_DIRS.contains(&name.as_str())
                {
                    pending.push(path);
                }
                continue;
            }
            let indexable = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
            if !indexable || meta.len() == 0 || meta.len() > MAX_FILE_BYTES {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            found.push(FileStamp {
                path,
                modified,
                size: meta.len() as i64,
            });
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// Argomenti di `llama-server` per il modello di embedding: sulla CPU (la
/// VRAM resta al modello di chat) e con lotti grandi quanto il contesto,
/// perché un pezzo si elabora tutto in una volta. Con `mean_pooling` il
/// vettore è la media dei token: serve ai modelli che non dichiarano come
/// riassumere il testo (quelli di chat), mentre i veri modelli di embedding
/// usano il loro.
pub fn embedding_args(model_path: &str, port: u16, mean_pooling: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "--model",
        model_path,
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--embeddings",
        "--ctx-size",
        "2048",
        "--batch-size",
        "2048",
        "--ubatch-size",
        "2048",
        "--parallel",
        "1",
        "--device",
        "none",
        "--gpu-layers",
        "0",
        "--no-webui",
        "--no-slots",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if mean_pooling {
        args.extend(["--pooling".to_string(), "mean".to_string()]);
    }
    args
}

/// Chiede i vettori di alcuni testi al `llama-server` degli embedding.
pub async fn embed(
    http: &reqwest::Client,
    port: u16,
    key: &str,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    let response = http
        .post(format!("http://127.0.0.1:{port}/v1/embeddings"))
        .bearer_auth(key)
        .json(&json!({ "input": texts, "model": "embedding" }))
        .send()
        .await
        .map_err(|e| Error::msg(format!("rete: {e}")))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| Error::msg(format!("rete: {e}")))?;
    if !status.is_success() {
        let message = body["error"]["message"]
            .as_str()
            .unwrap_or("errore sconosciuto");
        return Err(Error::msg(format!("modello di embedding: {message}")));
    }
    let mut data: Vec<(usize, Vec<f32>)> = body["data"]
        .as_array()
        .ok_or_else(|| Error::msg("modello di embedding: risposta senza vettori"))?
        .iter()
        .map(|item| {
            let index = item["index"].as_u64().unwrap_or(0) as usize;
            let vector = item["embedding"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(|x| x.as_f64())
                        .map(|x| x as f32)
                        .collect()
                })
                .unwrap_or_default();
            (index, vector)
        })
        .collect();
    data.sort_by_key(|(i, _)| *i);
    if data.len() != texts.len() {
        return Err(Error::msg(
            "modello di embedding: numero di vettori sbagliato",
        ));
    }
    Ok(data.into_iter().map(|(_, v)| v).collect())
}

/// Lo strumento come lo vede il modello.
pub fn tool_def() -> ToolDef {
    ToolDef {
        name: DOCUMENTS_TOOL.into(),
        description: "Cerca nei documenti personali dell'utente (le cartelle che ha reso \
            consultabili) e restituisce i brani più pertinenti, ciascuno con il file da cui \
            viene. Scrivi la domanda con parole simili a quelle che ci si aspetta nei \
            documenti. Cita i file da cui prendi le informazioni."
            .into(),
        schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Che cosa cercare" }
            },
            "required": ["query"]
        }),
        risk: Risk::Read,
    }
}

/// Il risultato per il modello: i brani, con il loro file.
pub fn format_hits(hits: &[Hit]) -> String {
    if hits.is_empty() {
        return "Nessun brano pertinente nei documenti.".to_string();
    }
    hits.iter()
        .enumerate()
        .map(|(i, hit)| format!("[{}] {}\n{}", i + 1, hit.path, hit.text))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pezzi_che_si_sovrappongono_e_coprono_tutto() {
        let paragraph = "Una frase di prova sul mare e sulle barche. ".repeat(20);
        let text = format!("{paragraph}\n\n{paragraph}\n\n{paragraph}");
        let chunks = chunk_text(&text);
        assert!(chunks.len() >= 3, "{}", chunks.len());
        assert!(chunks.iter().all(|c| c.chars().count() <= CHUNK_CHARS));
        // Ogni parola del testo sta in almeno un pezzo.
        assert!(chunks.concat().contains("barche"));
        assert!(chunk_text("   ").is_empty());
        assert_eq!(chunk_text("breve"), ["breve"]);
    }

    #[test]
    fn coseno_e_byte() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        let v = vec![0.5f32, -1.25, 3.0];
        assert_eq!(from_bytes(&to_bytes(&v)), v);
    }

    #[test]
    fn la_scansione_salta_cartelle_pesanti_e_file_estranei() {
        let dir = std::env::temp_dir().join(format!("rag-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::create_dir_all(dir.join("note")).unwrap();
        std::fs::write(dir.join("a.md"), "ciao").unwrap();
        std::fs::write(dir.join("note").join("b.txt"), "ciao").unwrap();
        std::fs::write(dir.join("node_modules").join("c.js"), "x").unwrap();
        std::fs::write(dir.join("foto.png"), "x").unwrap();
        std::fs::write(dir.join("vuoto.txt"), "").unwrap();
        let names: Vec<String> = scan(&dir)
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.md", "b.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
