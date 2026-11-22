//! Libreria dei modelli locali: cerca i file `.gguf` nelle cartelle scelte dall'utente.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::gguf;

/// Profondità massima di ricerca nelle sottocartelle.
const MAX_DEPTH: usize = 6;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
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
    /// Componente visivo (`mmproj-….gguf`) nella stessa cartella: con questo
    /// il modello può leggere le immagini.
    pub mmproj: Option<String>,
    pub mmproj_bytes: Option<u64>,
    /// In quanti file è diviso il modello (`-00001-of-00003.gguf`); 1 se è
    /// in un file solo. `path` è sempre la prima parte.
    pub parts: u32,
    /// Presente se il file non si è potuto leggere.
    pub error: Option<String>,
    /// Se e come si controlla il ragionamento ("thinking") del modello: con
    /// `switch` si accende e spegne, con `effort` (gpt-oss) si riduce.
    pub thinking: gguf::ThinkingControl,
    /// Dettagli per la stima della memoria: non servono all'interfaccia.
    #[serde(skip)]
    #[ts(skip)]
    pub layout: gguf::Layout,
}

/// Lunghezza del suffisso `-00001-of-00003.gguf` dei modelli divisi.
const SPLIT_SUFFIX_LEN: usize = 20;

/// Un modello diviso in più file: `nome-00001-of-00003.gguf`.
/// Restituisce la parte (da 1) e il totale.
pub fn split_part(file_name: &str) -> Option<(u32, u32)> {
    let stem = file_name.to_lowercase();
    let stem = stem.strip_suffix(".gguf")?;
    let (rest, total) = stem.rsplit_once("-of-")?;
    let (_, part) = rest.rsplit_once('-')?;
    let digits = |s: &str| s.len() == 5 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(part) || !digits(total) {
        return None;
    }
    let (part, total): (u32, u32) = (part.parse().ok()?, total.parse().ok()?);
    (part >= 1 && part <= total).then_some((part, total))
}

/// I file di tutte le parti di un modello diviso, a partire dalla prima.
/// Per un modello in un file solo, il file stesso.
pub fn split_files(first: &Path) -> Vec<PathBuf> {
    let name = first
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some((1, total)) = split_part(&name) else {
        return vec![first.to_path_buf()];
    };
    let base = &name[..name.len() - SPLIT_SUFFIX_LEN];
    (1..=total)
        .map(|part| first.with_file_name(format!("{base}-{part:05}-of-{total:05}.gguf")))
        .collect()
}

/// `true` per il componente visivo di un modello.
pub fn is_mmproj(file_name: &str) -> bool {
    let name = file_name.to_lowercase();
    name.ends_with(".gguf") && (name.starts_with("mmproj") || name.contains("-mmproj"))
}

/// Il componente visivo adatto a un modello, cercato nella sua cartella. Con
/// più candidati si prende quello col nome più simile a quello del modello
/// (`mmproj-Qwen2.5-VL-7B-f16.gguf` per `Qwen2.5-VL-7B-Q4_K_M.gguf`).
pub fn find_mmproj(model: &Path) -> Option<PathBuf> {
    let dir = model.parent()?;
    let model_name = model.file_name()?.to_string_lossy().to_lowercase();
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .is_some_and(|n| is_mmproj(&n.to_string_lossy()))
        })
        .collect();
    candidates.sort();
    let score = |p: &PathBuf| {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let name = name
            .trim_start_matches("mmproj")
            .trim_start_matches(['-', '_']);
        name.chars()
            .zip(model_name.chars())
            .take_while(|(a, b)| a == b)
            .count()
    };
    // A parità di somiglianza vince il primo in ordine alfabetico.
    let best = candidates.iter().map(score).max()?;
    candidates.into_iter().find(|p| score(p) == best)
}

/// Legge le informazioni di un modello. Per un modello diviso `path` è la
/// prima parte, e la dimensione è quella di tutte le parti.
pub fn inspect(path: &Path) -> ModelInfo {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let files = split_files(path);
    let parts = files.len() as u32;
    let missing = files.iter().filter(|f| !f.is_file()).count();
    let size_bytes = files
        .iter()
        .map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0))
        .sum();
    let (mut meta, mut error) = match gguf::read_meta(path) {
        Ok(meta) => (meta, None),
        Err(e) => (gguf::GgufMeta::default(), Some(e.to_string())),
    };
    if missing > 0 && error.is_none() {
        error = Some(format!(
            "il modello è diviso in {parts} file e ne mancano {missing}"
        ));
    }
    // In un modello diviso i tensori sono sparsi tra le parti: la stima
    // della memoria li vuole tutti. Se una parte non si legge resta quella
    // fatta con la prima, più prudente.
    if parts > 1 && error.is_none() {
        let all: Result<Vec<Vec<(String, u64)>>, _> =
            files.iter().map(|f| gguf::read_tensor_list(f)).collect();
        if let Ok(all) = all {
            meta.layout.set_tensors(&all.concat());
        }
    }
    let mmproj = find_mmproj(path);
    let mmproj_bytes = mmproj
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len());
    // Nel nome di un modello diviso il numero della parte non serve.
    let display_stem = match split_part(&file_name) {
        Some(_) => file_name[..file_name.len() - SPLIT_SUFFIX_LEN].to_string(),
        None => file_name.trim_end_matches(".gguf").to_string(),
    };

    // Il nome del file è più preciso del campo `general.file_type`: con le
    // quantizzazioni miste (le "UD" di Unsloth) quel campo dice spesso il
    // tipo sbagliato, per esempio Q8_0 per un file UD-Q6_K_XL.
    let quantization = quantization_from_name(&display_stem).or(meta.quantization);

    ModelInfo {
        path: path.to_string_lossy().into_owned(),
        name: meta
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(display_stem),
        file_name,
        size_bytes,
        architecture: meta.architecture,
        size_label: meta.size_label,
        quantization,
        context_length: meta.context_length,
        block_count: meta.block_count,
        embedding_length: meta.embedding_length,
        head_count: meta.head_count,
        head_count_kv: meta.head_count_kv,
        mmproj: mmproj.map(|p| p.to_string_lossy().into_owned()),
        mmproj_bytes,
        parts,
        error,
        thinking: meta.thinking,
        layout: meta.layout,
    }
}

/// La quantizzazione scritta nel nome del file (`…-Q4_K_M`, `…-UD-Q6_K_XL`,
/// `….IQ3_XXS`, `…-BF16`), se c'è.
pub fn quantization_from_name(stem: &str) -> Option<String> {
    let upper = stem.to_ascii_uppercase();
    let tokens: Vec<&str> = upper.split(['-', '.']).collect();
    let is_quant = |t: &str| {
        if matches!(t, "F16" | "F32" | "BF16" | "MXFP4") {
            return true;
        }
        let mut parts = t.split('_');
        let head = parts.next().unwrap_or("");
        let base = head.strip_prefix('I').unwrap_or(head);
        let Some(digits) = base.strip_prefix('Q') else {
            return false;
        };
        !digits.is_empty()
            && digits.chars().all(|c| c.is_ascii_digit())
            && parts.all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()))
    };
    let i = tokens.iter().rposition(|t| is_quant(t))?;
    let quant = tokens[i];
    Some(match i.checked_sub(1).map(|j| tokens[j]) {
        Some("UD") => format!("UD-{quant}"),
        _ => quant.to_string(),
    })
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
    // I file "mmproj" sono il componente visivo di un modello, non un modello
    // a sé; di un modello diviso in più file si elenca solo la prima parte.
    name.ends_with(".gguf")
        && !is_mmproj(&name)
        && split_part(&name).is_none_or(|(part, _)| part == 1)
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
        assert!(
            models[0]
                .mmproj
                .as_deref()
                .unwrap()
                .ends_with("mmproj-prova.gguf")
        );
    }

    #[test]
    fn legge_la_quantizzazione_dal_nome() {
        let q = |s: &str| quantization_from_name(s);
        assert_eq!(
            q("NVIDIA-Nemotron-3.5-Bolt-E4B-UD-Q6_K_XL").as_deref(),
            Some("UD-Q6_K_XL")
        );
        assert_eq!(q("Qwen2.5-7B-Instruct-Q4_K_M").as_deref(), Some("Q4_K_M"));
        assert_eq!(q("gemma-3-1b-it-UD-Q4_K_XL").as_deref(), Some("UD-Q4_K_XL"));
        assert_eq!(q("Llama-3.1-8B.IQ3_XXS").as_deref(), Some("IQ3_XXS"));
        assert_eq!(q("phi-4-bf16").as_deref(), Some("BF16"));
        assert_eq!(q("qwen2.5-coder-7b-instruct-q8_0").as_deref(), Some("Q8_0"));
        assert_eq!(q("Qwen2.5-7B-Instruct"), None);
        assert_eq!(q("modello"), None);
    }

    #[test]
    fn riconosce_i_modelli_divisi() {
        assert_eq!(
            split_part("Grande-Q4_K_M-00001-of-00003.gguf"),
            Some((1, 3))
        );
        assert_eq!(split_part("grande-00003-of-00003.gguf"), Some((3, 3)));
        assert_eq!(split_part("grande-00004-of-00003.gguf"), None);
        assert_eq!(split_part("modello-Q4_K_M.gguf"), None);
        assert_eq!(split_part("v-1-of-2.gguf"), None);
        let files = split_files(Path::new("c/Grande-00001-of-00002.gguf"));
        assert_eq!(
            files,
            [
                PathBuf::from("c/Grande-00001-of-00002.gguf"),
                PathBuf::from("c/Grande-00002-of-00002.gguf")
            ]
        );
        assert!(!is_model_file(Path::new("c/Grande-00002-of-00002.gguf")));
        assert!(is_model_file(Path::new("c/Grande-00001-of-00002.gguf")));
    }

    #[test]
    fn un_modello_diviso_si_misura_su_tutte_le_parti() {
        let dir = std::env::temp_dir().join(format!("models-split-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("Grande-00001-of-00002.gguf");
        std::fs::write(
            &first,
            gguf::tests::gguf_with_tensors(&[("token_embd.weight", 640)]),
        )
        .unwrap();
        std::fs::write(
            dir.join("Grande-00002-of-00002.gguf"),
            gguf::tests::gguf_with_tensors(&[("output.weight", 640)]),
        )
        .unwrap();
        let model = inspect(&first);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(model.parts, 2);
        assert_eq!(model.layout.token_embd_bytes, 640);
        // `output.weight` sta nella seconda parte: il modello non riusa gli
        // embedding in uscita, quindi niente copia in più sulla GPU.
        assert!(!model.layout.tied_output);
    }

    #[test]
    fn sceglie_il_componente_visivo_piu_simile() {
        let root = std::env::temp_dir().join(format!("mmproj-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for name in [
            "mmproj-gemma-3-4b-f16.gguf",
            "mmproj-qwen2.5-vl-7b-f16.gguf",
            "Qwen2.5-VL-7B-Q4_K_M.gguf",
        ] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        let found = find_mmproj(&root.join("Qwen2.5-VL-7B-Q4_K_M.gguf"));
        let _ = std::fs::remove_dir_all(&root);
        assert!(found.unwrap().ends_with("mmproj-qwen2.5-vl-7b-f16.gguf"));
    }
}
