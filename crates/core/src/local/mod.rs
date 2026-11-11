//! Modelli locali: parametri, argomenti di `llama-server`, stima della memoria.
//!
//! Tutto ciò che dipende dalla versione di llama.cpp sta in questo modulo
//! (vedi `docs/adr/0002-motore-locale-llama-server.md`).

pub mod install;
pub mod runtime;
pub mod sysmon;

use serde::{Deserialize, Serialize};

use crate::models::ModelInfo;

/// Versione di llama.cpp con cui gli argomenti qui sotto sono stati provati.
pub const LLAMA_CPP_BUILD: &str = "b12110";

/// Dove risiedono modello e cache KV. Mai in entrambe le memorie
/// (vedi `docs/adr/0003-collocazione-memoria.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Vram,
    Ram,
}

/// Quantizzazione della cache KV.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KvCacheType {
    #[serde(rename = "f16")]
    F16,
    #[serde(rename = "q8_0")]
    Q8_0,
    #[serde(rename = "q4_0")]
    Q4_0,
}

impl KvCacheType {
    fn arg(self) -> &'static str {
        match self {
            Self::F16 => "f16",
            Self::Q8_0 => "q8_0",
            Self::Q4_0 => "q4_0",
        }
    }

    /// Byte occupati da ogni valore della cache. I tipi quantizzati lavorano a
    /// blocchi di 32 valori più un fattore di scala a 16 bit.
    fn bytes_per_value(self) -> f64 {
        match self {
            Self::F16 => 2.0,
            Self::Q8_0 => 34.0 / 32.0,
            Self::Q4_0 => 18.0 / 32.0,
        }
    }
}

/// Parametri di caricamento: cambiarli richiede di ricaricare il modello.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LoadParams {
    pub placement: Placement,
    pub context_length: u32,
    pub kv_cache_type: KvCacheType,
    /// Thread della CPU; `None` lascia decidere a llama.cpp.
    pub threads: Option<u32>,
    pub batch_size: Option<u32>,
    /// Modalità di lettura del file (`--load-mode`). `None` usa quella
    /// predefinita per la collocazione scelta. Solo per prove e misure.
    pub load_mode: Option<String>,
}

impl Default for LoadParams {
    fn default() -> Self {
        Self {
            placement: Placement::Vram,
            context_length: 8192,
            kv_cache_type: KvCacheType::Q8_0,
            threads: None,
            batch_size: None,
            load_mode: None,
        }
    }
}

impl LoadParams {
    /// Modalità di lettura effettiva del file del modello.
    ///
    /// Sempre lettura diretta dal disco (`dio`), mai mappatura in memoria.
    /// Con la mappatura i pesi finiscono due volte in RAM (misure nell'ADR 0003):
    /// - in VRAM, perché il file mappato resta in RAM dopo la copia sulla GPU;
    /// - in RAM, perché llama.cpp riorganizza i pesi in un secondo buffer.
    pub fn effective_load_mode(&self) -> &str {
        self.load_mode.as_deref().unwrap_or("dio")
    }
}

/// Parametri di generazione: viaggiano con ogni richiesta, effetto immediato.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub min_p: f32,
    pub repeat_penalty: f32,
    /// `None` = casuale a ogni risposta.
    pub seed: Option<i64>,
    /// `None` = nessun limite oltre al contesto.
    pub max_tokens: Option<u32>,
    pub stop: Vec<String>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.8,
            top_p: 0.95,
            top_k: 40,
            min_p: 0.05,
            repeat_penalty: 1.0,
            seed: None,
            max_tokens: None,
            stop: Vec::new(),
        }
    }
}

/// Tutti i parametri salvati per un modello.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preset {
    pub load: LoadParams,
    pub sampling: SamplingParams,
    pub system_prompt: String,
}

/// Argomenti con cui avviare `llama-server`.
pub fn build_args(model_path: &str, params: &LoadParams, port: u16) -> Vec<String> {
    let kv = params.kv_cache_type.arg();
    let mut args: Vec<String> = [
        "--model",
        model_path,
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--ctx-size",
        &params.context_length.to_string(),
        // Un solo flusso: tutto il contesto è per la conversazione in corso.
        "--parallel",
        "1",
        "--cache-type-k",
        kv,
        "--cache-type-v",
        kv,
        // Necessaria per quantizzare la cache KV.
        "--flash-attn",
        "on",
        // Niente adattamenti automatici: se non entra deve fallire, non dividersi.
        "--fit",
        "off",
        // Niente copie della cache in RAM: né dei prompt, né punti di ripristino.
        "--cache-ram",
        "0",
        "--ctx-checkpoints",
        "0",
        "--load-mode",
        params.effective_load_mode(),
        "--no-webui",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let placement: &[&str] = match params.placement {
        Placement::Vram => &["--gpu-layers", "all", "--kv-offload"],
        Placement::Ram => &["--device", "none", "--gpu-layers", "0", "--no-kv-offload"],
    };
    args.extend(placement.iter().map(|s| s.to_string()));

    if let Some(threads) = params.threads {
        args.extend(["--threads".to_string(), threads.to_string()]);
    }
    if let Some(batch) = params.batch_size {
        args.extend(["--batch-size".to_string(), batch.to_string()]);
    }
    args
}

/// Memoria condivisa che un processo CUDA usa anche quando tutto sta in VRAM
/// (buffer di scambio con la scheda): misurati circa 410 MiB.
const SHARED_GPU_BASELINE: u64 = 410 * 1024 * 1024;

/// Oltre questa soglia di memoria condivisa si considera che il driver stia
/// riversando la VRAM in RAM. Misure (ADR 0003): 410 MiB in condizioni
/// normali, 2418 MiB con la VRAM piena.
const SHARED_GPU_SPILL_THRESHOLD: u64 = 800 * 1024 * 1024;

/// Se la memoria condivisa indica uno sconfinamento, restituisce quanta RAM
/// viene usata al posto della VRAM.
pub fn vram_spill(shared_bytes: u64) -> Option<u64> {
    (shared_bytes > SHARED_GPU_SPILL_THRESHOLD).then(|| shared_bytes - SHARED_GPU_BASELINE)
}

/// Stima della memoria necessaria per caricare un modello.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEstimate {
    pub weights_bytes: u64,
    pub kv_cache_bytes: u64,
    /// Buffer di calcolo e margine di sicurezza.
    pub overhead_bytes: u64,
    pub total_bytes: u64,
    /// `false` se nel file mancavano dati e la cache KV non è stata calcolata.
    pub kv_cache_known: bool,
}

/// Margine fisso per i buffer di calcolo, più una quota proporzionale ai pesi.
const OVERHEAD_FIXED: u64 = 400 * 1024 * 1024;
const OVERHEAD_RATIO: f64 = 0.03;

pub fn estimate_memory(model: &ModelInfo, params: &LoadParams) -> MemoryEstimate {
    let weights_bytes = model.size_bytes;

    // Cache KV: per ogni layer e ogni token si conservano una chiave e un valore,
    // ciascuno grande (dimensione di una testa) × (numero di teste KV).
    let kv = match (model.block_count, model.embedding_length, model.head_count) {
        (Some(layers), Some(embd), Some(heads)) if heads > 0 => {
            let heads_kv = model.head_count_kv.unwrap_or(heads);
            let values_per_token = 2 * (embd / heads) * heads_kv;
            let values = layers * params.context_length as u64 * values_per_token;
            Some((values as f64 * params.kv_cache_type.bytes_per_value()) as u64)
        }
        _ => None,
    };
    let kv_cache_bytes = kv.unwrap_or(0);

    let overhead_bytes = OVERHEAD_FIXED + (weights_bytes as f64 * OVERHEAD_RATIO) as u64;
    MemoryEstimate {
        weights_bytes,
        kv_cache_bytes,
        overhead_bytes,
        total_bytes: weights_bytes + kv_cache_bytes + overhead_bytes,
        kv_cache_known: kv.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    fn has(args: &[String], flag: &str) -> bool {
        args.iter().any(|a| a == flag)
    }

    #[test]
    fn vram_mette_tutto_sulla_gpu() {
        let args = build_args("m.gguf", &LoadParams::default(), 4000);
        assert!(has_pair(&args, "--gpu-layers", "all"));
        assert!(has(&args, "--kv-offload"));
        assert!(!has(&args, "--no-kv-offload"));
        assert!(has_pair(&args, "--load-mode", "dio"));
    }

    #[test]
    fn ram_non_tocca_la_gpu() {
        let params = LoadParams {
            placement: Placement::Ram,
            ..Default::default()
        };
        let args = build_args("m.gguf", &params, 4000);
        assert!(has_pair(&args, "--gpu-layers", "0"));
        assert!(has_pair(&args, "--device", "none"));
        assert!(has(&args, "--no-kv-offload"));
        assert!(has_pair(&args, "--load-mode", "dio"));
    }

    #[test]
    fn cache_kv_q8_di_default_e_nessuna_copia_in_ram() {
        let args = build_args("m.gguf", &LoadParams::default(), 4000);
        assert!(has_pair(&args, "--cache-type-k", "q8_0"));
        assert!(has_pair(&args, "--cache-type-v", "q8_0"));
        assert!(has_pair(&args, "--flash-attn", "on"));
        assert!(has_pair(&args, "--cache-ram", "0"));
        assert!(has_pair(&args, "--fit", "off"));
    }

    #[test]
    fn lo_sconfinamento_si_riconosce_dalla_memoria_condivisa() {
        const MIB: u64 = 1024 * 1024;
        // Valori misurati: 410 MiB in condizioni normali, 2418 con la VRAM piena.
        assert_eq!(vram_spill(410 * MIB), None);
        assert_eq!(vram_spill(2418 * MIB), Some(2008 * MIB));
    }

    #[test]
    fn la_cache_q8_occupa_circa_meta_della_f16() {
        let model = ModelInfo {
            path: String::new(),
            file_name: String::new(),
            name: String::new(),
            size_bytes: 4_000_000_000,
            architecture: None,
            size_label: None,
            quantization: None,
            context_length: Some(32768),
            block_count: Some(28),
            embedding_length: Some(3584),
            head_count: Some(28),
            head_count_kv: Some(4),
            error: None,
        };
        let q8 = estimate_memory(&model, &LoadParams::default());
        let f16 = estimate_memory(
            &model,
            &LoadParams {
                kv_cache_type: KvCacheType::F16,
                ..Default::default()
            },
        );
        // 28 layer × 8192 token × 2 × (3584/28 × 4) valori × 34/32 byte
        assert_eq!(q8.kv_cache_bytes, 249_561_088);
        assert!(q8.kv_cache_known);
        let ratio = q8.kv_cache_bytes as f64 / f16.kv_cache_bytes as f64;
        assert!((ratio - 0.53).abs() < 0.01);
    }
}
