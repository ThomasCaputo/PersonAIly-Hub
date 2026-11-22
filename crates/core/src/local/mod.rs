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

/// Dove risiedono modello e cache KV. Di base mai in entrambe le memorie
/// (vedi `docs/adr/0003-collocazione-memoria.md`); la divisione tra GPU e CPU
/// esiste solo come scelta esplicita e avanzata (LO-10, ADR 0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Vram,
    Ram,
    /// I primi [`LoadParams::gpu_layers`] layer (con la loro cache KV) in
    /// VRAM, gli altri in RAM: per modelli che non entrano nella VRAM.
    Hybrid,
}

/// Quantizzazione della cache KV.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
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
    /// Carica anche il componente visivo (`mmproj`) se accanto al modello ce
    /// n'è uno: il modello legge le immagini, al costo di un po' di memoria.
    pub vision: bool,
    /// Solo per [`Placement::Hybrid`]: quanti layer vanno sulla GPU.
    pub gpu_layers: Option<u32>,
    /// Con `false` il modello risponde subito, senza ragionare prima
    /// (`--reasoning off`). Conta solo per i modelli che sanno ragionare.
    pub thinking: bool,
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
            vision: true,
            gpu_layers: None,
            thinking: true,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
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

/// Argomenti con cui avviare `llama-server`. `mmproj` è il componente
/// visivo da caricare insieme al modello, se c'è.
pub fn build_args(
    model_path: &str,
    params: &LoadParams,
    port: u16,
    mmproj: Option<&str>,
) -> Vec<String> {
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
        // L'elenco degli slot mostra lo stato delle richieste: non serve all'app.
        "--no-slots",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let placement: Vec<String> = match params.placement {
        Placement::Vram => vec!["--gpu-layers".into(), "all".into(), "--kv-offload".into()],
        Placement::Ram => ["--device", "none", "--gpu-layers", "0", "--no-kv-offload"]
            .map(String::from)
            .to_vec(),
        // La cache KV di ogni layer sta dove sta il layer: sulla GPU quella dei
        // layer sulla GPU, in RAM le altre.
        Placement::Hybrid => vec![
            "--gpu-layers".into(),
            params.gpu_layers.unwrap_or(0).to_string(),
            "--kv-offload".into(),
        ],
    };
    args.extend(placement);

    // Acceso, llama.cpp decide dal template (`auto`): così i modelli che non
    // ragionano restano come sono.
    if !params.thinking {
        args.extend(["--reasoning".to_string(), "off".to_string()]);
    }
    if let Some(threads) = params.threads {
        args.extend(["--threads".to_string(), threads.to_string()]);
    }
    if let Some(batch) = params.batch_size {
        args.extend(["--batch-size".to_string(), batch.to_string()]);
    }
    if let Some(mmproj) = mmproj {
        args.extend(["--mmproj".to_string(), mmproj.to_string()]);
        // Anche il componente visivo resta nella memoria scelta (ADR 0003).
        if params.placement == Placement::Ram {
            args.push("--no-mmproj-offload".to_string());
        }
    }
    args
}

/// Memoria condivisa che un processo CUDA usa anche quando tutto sta in VRAM,
/// oltre ai pesi che llama.cpp tiene in RAM: buffer di scambio con la scheda.
/// Misurati 80-120 MiB (Qwen2.5 7B, Gemma 3 1B, Gemma 4 E4B).
const SHARED_GPU_BASELINE: u64 = 128 * 1024 * 1024;

/// Tolleranza oltre la memoria condivisa attesa prima di parlare di
/// sconfinamento. Misure (ADR 0003): 410 MiB in condizioni normali con
/// Qwen2.5 7B, 2418 MiB con la VRAM piena.
const SHARED_GPU_SPILL_MARGIN: u64 = 400 * 1024 * 1024;

/// Se la memoria condivisa indica uno sconfinamento, restituisce quanta RAM
/// viene usata al posto della VRAM.
///
/// `pinned_host_bytes` sono i pesi che llama.cpp tiene comunque in RAM (gli
/// embedding d'ingresso): letti con `dio` finiscono in memoria bloccata per
/// la GPU, che Windows conta come memoria condivisa. Con Gemma 4 E4B sono
/// 2,9 GB: senza toglierli, ogni caricamento sembrerebbe uno sconfinamento.
pub fn vram_spill(shared_bytes: u64, pinned_host_bytes: u64) -> Option<u64> {
    let expected = SHARED_GPU_BASELINE + pinned_host_bytes;
    (shared_bytes > expected + SHARED_GPU_SPILL_MARGIN).then(|| shared_bytes - expected)
}

/// Celle della cache dei layer a finestra scorrevole: llama.cpp ne tiene la
/// finestra più un lotto di elaborazione (512), arrotondate a 256.
fn swa_cells(window: u64, context: u64) -> u64 {
    (window + 512)
        .div_ceil(256)
        .saturating_mul(256)
        .min(context)
}

/// Byte della cache KV, layer per layer. `None` se nel file mancano i dati.
fn kv_cache_bytes(model: &ModelInfo, params: &LoadParams) -> Option<u64> {
    let layers = model.block_count?;
    let heads = model.head_count.filter(|h| *h > 0)?;
    let heads_kv = model.head_count_kv.unwrap_or(heads);
    let layout = &model.layout;
    let head_dim = model.embedding_length.map(|embd| embd / heads);
    let key = layout.key_length.or(head_dim)?;
    let value = layout.value_length.or(head_dim)?;
    let key_swa = layout.key_length_swa.unwrap_or(key);
    let value_swa = layout.value_length_swa.unwrap_or(value);

    let context = params.context_length as u64;
    let window_cells = layout.sliding_window.map(|w| swa_cells(w, context));
    // Gli ultimi layer condivisi riusano la cache dei precedenti: non ne hanno.
    let own_kv = layers.saturating_sub(layout.shared_kv_layers.unwrap_or(0));

    let values: u64 = (0..own_kv)
        .map(|i| {
            let swa = layout
                .swa_layers
                .as_ref()
                .and_then(|p| p.get(i as usize))
                .copied()
                .unwrap_or(false);
            match window_cells {
                Some(cells) if swa => cells * heads_kv * (key_swa + value_swa),
                _ => context * heads_kv * (key + value),
            }
        })
        .sum();
    Some((values as f64 * params.kv_cache_type.bytes_per_value()) as u64)
}

/// Stima della memoria necessaria per caricare un modello.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEstimate {
    /// Pesi nella memoria scelta: con la GPU, quelli che vanno sulla GPU.
    pub weights_bytes: u64,
    /// Pesi che llama.cpp tiene in RAM anche con tutti i layer sulla GPU
    /// (embedding d'ingresso); 0 con "Solo RAM", dove sono già in
    /// `weights_bytes`.
    pub host_weights_bytes: u64,
    pub kv_cache_bytes: u64,
    /// Buffer di calcolo e margine di sicurezza.
    pub overhead_bytes: u64,
    pub total_bytes: u64,
    /// Quanta parte del totale va in VRAM e quanta in RAM. Con "Solo VRAM" in
    /// RAM restano solo gli embedding d'ingresso.
    pub vram_bytes: u64,
    pub ram_bytes: u64,
    /// `false` se nel file mancavano dati e la cache KV non è stata calcolata.
    pub kv_cache_known: bool,
}

/// Margine fisso per i buffer di calcolo, più una quota proporzionale ai pesi.
const OVERHEAD_FIXED: u64 = 400 * 1024 * 1024;
const OVERHEAD_RATIO: f64 = 0.03;

/// Con la collocazione ibrida il processo tiene in RAM anche le librerie CUDA
/// e i buffer di scambio con la GPU: misurati 0,3-0,75 GB in più (ADR 0016).
const HYBRID_HOST_BASE: u64 = 750 * 1024 * 1024;

pub fn estimate_memory(model: &ModelInfo, params: &LoadParams) -> MemoryEstimate {
    // Il componente visivo si carica accanto ai pesi, nella stessa memoria.
    let vision_bytes = if params.vision {
        model.mmproj_bytes.unwrap_or(0)
    } else {
        0
    };
    let all_weights = model.size_bytes + vision_bytes;

    // Cache KV: per ogni layer e ogni token si conservano una chiave e un
    // valore, ciascuno grande (dimensione di una testa) × (numero di teste
    // KV); i layer a finestra scorrevole tengono solo la finestra.
    let kv = kv_cache_bytes(model, params);
    let kv_cache_bytes = kv.unwrap_or(0);

    // llama.cpp tiene sempre in RAM gli embedding d'ingresso; se il modello
    // li riusa anche in uscita, sulla GPU ne va una copia.
    let layout = &model.layout;
    let host = layout.host_weights_bytes().min(all_weights);
    let output_copy = if layout.tied_output {
        layout.token_embd_bytes
    } else {
        0
    };

    let (weights_bytes, host_weights_bytes, host_base) = match params.placement {
        Placement::Vram => (all_weights - host + output_copy, host, 0),
        Placement::Ram => (all_weights, 0, 0),
        Placement::Hybrid => (all_weights - host, host, HYBRID_HOST_BASE),
    };
    let overhead_bytes = OVERHEAD_FIXED + (weights_bytes as f64 * OVERHEAD_RATIO) as u64;
    let placed = weights_bytes + kv_cache_bytes + overhead_bytes;
    let (vram_bytes, ram_bytes) = match params.placement {
        Placement::Vram => (placed, host_weights_bytes),
        Placement::Ram => (0, placed),
        Placement::Hybrid => {
            // Pesi e cache si dividono in proporzione ai layer; i buffer di
            // calcolo (la parte fissa del margine) stanno sulla GPU.
            let layers = model.block_count.unwrap_or(1).max(1);
            let on_gpu = u64::from(params.gpu_layers.unwrap_or(0)).min(layers);
            let share = on_gpu as f64 / layers as f64;
            let split = (placed - OVERHEAD_FIXED) as f64;
            let vram = (split * share) as u64 + if on_gpu > 0 { OVERHEAD_FIXED } else { 0 };
            (vram, placed + host_weights_bytes + host_base - vram)
        }
    };
    let total_bytes = vram_bytes + ram_bytes;
    MemoryEstimate {
        weights_bytes,
        host_weights_bytes,
        kv_cache_bytes,
        overhead_bytes,
        total_bytes,
        vram_bytes,
        ram_bytes,
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
        let args = build_args("m.gguf", &LoadParams::default(), 4000, None);
        assert!(has_pair(&args, "--gpu-layers", "all"));
        assert!(has(&args, "--kv-offload"));
        assert!(!has(&args, "--no-kv-offload"));
        assert!(has_pair(&args, "--load-mode", "dio"));
    }

    #[test]
    fn ibrida_divide_i_layer() {
        let params = LoadParams {
            placement: Placement::Hybrid,
            gpu_layers: Some(14),
            ..Default::default()
        };
        let args = build_args("m.gguf", &params, 4000, None);
        assert!(has_pair(&args, "--gpu-layers", "14"));
        assert!(has(&args, "--kv-offload"));
        assert!(!has(&args, "--device"));
    }

    #[test]
    fn ram_non_tocca_la_gpu() {
        let params = LoadParams {
            placement: Placement::Ram,
            ..Default::default()
        };
        let args = build_args("m.gguf", &params, 4000, None);
        assert!(has_pair(&args, "--gpu-layers", "0"));
        assert!(has_pair(&args, "--device", "none"));
        assert!(has(&args, "--no-kv-offload"));
        assert!(has_pair(&args, "--load-mode", "dio"));
    }

    #[test]
    fn cache_kv_q8_di_default_e_nessuna_copia_in_ram() {
        let args = build_args("m.gguf", &LoadParams::default(), 4000, None);
        assert!(has_pair(&args, "--cache-type-k", "q8_0"));
        assert!(has_pair(&args, "--cache-type-v", "q8_0"));
        assert!(has_pair(&args, "--flash-attn", "on"));
        assert!(has_pair(&args, "--cache-ram", "0"));
        assert!(has_pair(&args, "--fit", "off"));
    }

    #[test]
    fn il_ragionamento_si_spegne_solo_se_richiesto() {
        let on = build_args("m.gguf", &LoadParams::default(), 4000, None);
        assert!(!has(&on, "--reasoning"));
        let params = LoadParams {
            thinking: false,
            ..Default::default()
        };
        let off = build_args("m.gguf", &params, 4000, None);
        assert!(has_pair(&off, "--reasoning", "off"));
    }

    #[test]
    fn il_componente_visivo_segue_la_collocazione() {
        let vram = build_args("m.gguf", &LoadParams::default(), 4000, Some("v.gguf"));
        assert!(has_pair(&vram, "--mmproj", "v.gguf"));
        assert!(!has(&vram, "--no-mmproj-offload"));
        let params = LoadParams {
            placement: Placement::Ram,
            ..Default::default()
        };
        let ram = build_args("m.gguf", &params, 4000, Some("v.gguf"));
        assert!(has(&ram, "--no-mmproj-offload"));
    }

    #[test]
    fn lo_sconfinamento_si_riconosce_dalla_memoria_condivisa() {
        const MIB: u64 = 1024 * 1024;
        // Qwen2.5 7B (292 MiB di embedding in RAM): 410 MiB in condizioni
        // normali, 2418 con la VRAM piena.
        assert_eq!(vram_spill(410 * MIB, 292 * MIB), None);
        assert_eq!(vram_spill(2418 * MIB, 292 * MIB), Some(1998 * MIB));
        // Gemma 4 E4B: 2885 MiB di embedding in RAM, 3038 MiB di memoria
        // condivisa con la VRAM mezza vuota. Non è uno sconfinamento.
        assert_eq!(vram_spill(3038 * MIB, 2885 * MIB), None);
    }

    fn model(size_bytes: u64) -> ModelInfo {
        ModelInfo {
            path: String::new(),
            file_name: String::new(),
            name: String::new(),
            size_bytes,
            architecture: None,
            size_label: None,
            quantization: None,
            context_length: None,
            block_count: None,
            embedding_length: None,
            head_count: None,
            head_count_kv: None,
            mmproj: None,
            mmproj_bytes: None,
            parts: 1,
            error: None,
            supports_thinking: false,
            layout: Default::default(),
        }
    }

    #[test]
    fn gemma4_e4b_tiene_gli_embedding_in_ram_e_poca_cache() {
        const MIB: u64 = 1024 * 1024;
        // Dati di NVIDIA-Nemotron-3.5-Bolt-E4B (architettura gemma4).
        let mut pattern = Vec::new();
        for _ in 0..7 {
            pattern.extend([true, true, true, true, true, false]);
        }
        let model = ModelInfo {
            block_count: Some(42),
            embedding_length: Some(2560),
            head_count: Some(8),
            head_count_kv: Some(2),
            layout: crate::gguf::Layout {
                key_length: Some(512),
                value_length: Some(512),
                key_length_swa: Some(256),
                value_length_swa: Some(256),
                sliding_window: Some(512),
                swa_layers: Some(pattern),
                shared_kv_layers: Some(18),
                token_embd_bytes: 680 * MIB,
                per_layer_embd_bytes: 2205 * MIB,
                tied_output: true,
            },
            ..model(7_258_879_168)
        };
        let params = LoadParams {
            context_length: 32256,
            vision: false,
            ..Default::default()
        };
        let e = estimate_memory(&model, &params);
        // Misurati con llama.cpp b12110: 267,75 + 21,25 MiB di cache KV.
        assert_eq!(e.kv_cache_bytes, 289 * MIB);
        // Sulla GPU: 4702,5 MiB di pesi (copia di token_embd compresa); la
        // stima parte dal file intero, intestazione compresa.
        assert!((4702..4702 + 32).contains(&(e.weights_bytes / MIB)));
        assert_eq!(e.host_weights_bytes, 2885 * MIB);
        assert_eq!(e.ram_bytes, 2885 * MIB);
        assert_eq!(e.vram_bytes + e.ram_bytes, e.total_bytes);
        // Prima la stima diceva 9,6 GB di VRAM: ora poco più di 5,5.
        assert!(e.vram_bytes < 6 * 1024 * MIB);
    }

    #[test]
    fn senza_schema_la_finestra_scorrevole_non_riduce_la_cache() {
        let mut model = ModelInfo {
            block_count: Some(10),
            embedding_length: Some(1024),
            head_count: Some(8),
            head_count_kv: Some(8),
            ..model(1_000_000_000)
        };
        let params = LoadParams::default();
        let full = estimate_memory(&model, &params).kv_cache_bytes;
        model.layout.sliding_window = Some(512);
        assert_eq!(estimate_memory(&model, &params).kv_cache_bytes, full);
        model.layout.swa_layers = Some(vec![true; 10]);
        assert!(estimate_memory(&model, &params).kv_cache_bytes < full / 4);
    }

    #[test]
    fn la_cache_q8_occupa_circa_meta_della_f16() {
        let model = ModelInfo {
            context_length: Some(32768),
            block_count: Some(28),
            embedding_length: Some(3584),
            head_count: Some(28),
            head_count_kv: Some(4),
            ..model(4_000_000_000)
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
        assert_eq!((q8.vram_bytes, q8.ram_bytes), (q8.total_bytes, 0));

        // Metà dei layer sulla GPU: circa metà di pesi e cache per parte,
        // più i buffer di calcolo sulla GPU.
        let half = estimate_memory(
            &model,
            &LoadParams {
                placement: Placement::Hybrid,
                gpu_layers: Some(14),
                ..Default::default()
            },
        );
        assert_eq!(half.vram_bytes + half.ram_bytes, half.total_bytes);
        // Pesi e cache divisi a metà: la differenza sono le quote fisse.
        assert!(half.vram_bytes.abs_diff(half.ram_bytes) <= HYBRID_HOST_BASE - OVERHEAD_FIXED + 1);
    }
}
