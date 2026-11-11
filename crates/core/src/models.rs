//! Libreria dei modelli locali: cerca i file `.gguf` nelle cartelle scelte dall'utente.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::gguf;

/// Profondità massima di ricerca nelle sottocartelle.
const MAX_DEPTH: usize = 6;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub path: String,
    pub file_name: String,
    /// Nome leggibile: quello scritto nel file, altrimenti il nome del file.
    pub name: String,
    pub size_bytes: u64,
    pub architecture: Option<String>,
    pub size_label: Option<String>,
    pub quantization: Option<String>,
    pub context_length: Option<u64>,
    pub block_count: Option<u64>,
    pub embedding_length: Option<u64>,
    pub head_count: Option<u64>,
    pub head_count_kv: Option<u64>,
    /// Presente se il file non si è potuto leggere.
    pub error: Option<String>,
}

/// Legge le informazioni di un singolo file.
pub fn inspect(path: &Path) -> ModelInfo {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let (meta, error) = match gguf::read_meta(path) {
        Ok(meta) => (meta, None),
        Err(e) => (gguf::GgufMeta::default(), Some(e.to_string())),
    };

    ModelInfo {
        path: path.to_string_lossy().into_owned(),
        name: meta
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| file_name.trim_end_matches(".gguf").to_string()),
        file_name,
        size_bytes,
        architecture: meta.architecture,
        size_label: meta.size_label,
        quantization: meta.quantization,
        context_length: meta.context_length,
        block_count: meta.block_count,
        embedding_length: meta.embedding_length,
        head_count: meta.head_count,
        head_count_kv: meta.head_count_kv,
        error,
    }
}

/// Cerca tutti i modelli nelle cartelle indicate, in ordine alfabetico.
pub fn scan(folders: &[PathBuf]) -> Vec<ModelInfo> {
    let mut files = Vec::new();
    for folder in folders {
        collect(folder, 0, &mut files);
    }
    files.sort();
    files.dedup();

    let mut models: Vec<ModelInfo> = files.iter().map(|p| inspect(p)).collect();
    models.sort_by_key(|m| m.name.to_lowercase());
    models
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < MAX_DEPTH {
                collect(&path, depth + 1, out);
            }
        } else if is_model_file(&path) {
            out.push(path);
        }
    }
}

fn is_model_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // I file "mmproj" sono il componente visivo di un modello, non un modello a sé.
    name.ends_with(".gguf") && !name.starts_with("mmproj")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trova_i_modelli_nelle_sottocartelle() {
        let root = std::env::temp_dir().join(format!("models-test-{}", std::process::id()));
        let sub = root.join("autore").join("modello");
        std::fs::create_dir_all(&sub).unwrap();
        gguf::tests::write_sample(&sub.join("prova.gguf"));
        gguf::tests::write_sample(&sub.join("mmproj-prova.gguf"));
        std::fs::write(root.join("note.txt"), "non un modello").unwrap();

        let models = scan(std::slice::from_ref(&root));
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].file_name, "prova.gguf");
        assert_eq!(models[0].name, "Modello di prova");
        assert_eq!(models[0].error, None);
    }
}
