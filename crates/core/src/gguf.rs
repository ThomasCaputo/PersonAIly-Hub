//! Lettura dei metadati di un file GGUF (solo l'intestazione, non i pesi).
//!
//! Formato: <https://github.com/ggml-org/ggml/blob/master/docs/gguf.md>

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use crate::error::{Error, Result};

/// Le informazioni che ci servono di un modello.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct GgufMeta {
    pub name: Option<String>,
    pub architecture: Option<String>,
    pub size_label: Option<String>,
    pub quantization: Option<String>,
    /// Contesto massimo con cui il modello è stato addestrato.
    pub context_length: Option<u64>,
    pub block_count: Option<u64>,
    pub embedding_length: Option<u64>,
    pub head_count: Option<u64>,
    pub head_count_kv: Option<u64>,
    pub has_chat_template: bool,
    /// Il modello sa ragionare prima di rispondere ("thinking"): il suo
    /// template di chat lo prevede.
    pub supports_thinking: bool,
    pub layout: Layout,
}

/// Come llama.cpp distribuisce la memoria del modello: forma della cache KV e
/// pesi che restano comunque in RAM. Serve solo alla stima della memoria.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Layout {
    /// Dimensione di una testa per chiavi e valori, se scritta nel file
    /// (altrimenti si ricava da `embedding_length / head_count`).
    pub key_length: Option<u64>,
    pub value_length: Option<u64>,
    /// Le stesse, per i layer ad attenzione locale (finestra scorrevole).
    pub key_length_swa: Option<u64>,
    pub value_length_swa: Option<u64>,
    /// Ampiezza della finestra scorrevole, in token.
    pub sliding_window: Option<u64>,
    /// Per ogni layer, `true` se usa la finestra scorrevole.
    pub swa_layers: Option<Vec<bool>>,
    /// Gli ultimi N layer riusano la cache KV di quelli precedenti (Gemma 3n/4).
    pub shared_kv_layers: Option<u64>,
    /// Tabella degli embedding d'ingresso (`token_embd`): llama.cpp la tiene
    /// sempre in RAM.
    pub token_embd_bytes: u64,
    /// Embedding per layer (`per_layer_token_embd`, Gemma 3n/4): anche questi
    /// restano in RAM.
    pub per_layer_embd_bytes: u64,
    /// Senza un tensore `output` il modello riusa `token_embd` anche in
    /// uscita, e llama.cpp ne mette una copia sulla GPU.
    pub tied_output: bool,
}

impl Layout {
    /// Pesi che llama.cpp lascia in RAM anche con tutti i layer sulla GPU.
    pub fn host_weights_bytes(&self) -> u64 {
        self.token_embd_bytes + self.per_layer_embd_bytes
    }
}

enum Value {
    Int(u64),
    Text(String),
    Bools(Vec<bool>),
    Other,
}

/// Oltre questo numero di elementi un array di booleani non è uno schema di
/// layer: lo si salta.
const MAX_BOOL_ARRAY: u64 = 4096;

/// Oltre questa lunghezza una stringa è quasi certamente un file corrotto.
const MAX_STRING: u64 = 64 * 1024 * 1024;

pub fn read_meta(path: &Path) -> Result<GgufMeta> {
    let mut r = BufReader::with_capacity(1 << 16, File::open(path)?);

    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        return Err(Error::msg("non è un file GGUF"));
    }
    let version = read_u32(&mut r)?;
    if !(2..=3).contains(&version) {
        return Err(Error::msg(format!(
            "versione GGUF {version} non supportata"
        )));
    }
    let tensor_count = read_u64(&mut r)?;
    let kv_count = read_u64(&mut r)?;

    let mut values = HashMap::new();
    for _ in 0..kv_count {
        let key = read_string(&mut r)?;
        let kind = read_u32(&mut r)?;
        let value = read_value(&mut r, kind)?;
        values.insert(key, value);
    }

    let text = |key: &str| match values.get(key) {
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    };
    let int = |key: &str| match values.get(key) {
        Some(Value::Int(n)) => Some(*n),
        _ => None,
    };

    let architecture = text("general.architecture");
    let arch = architecture.clone().unwrap_or_default();
    let arch_int = |suffix: &str| int(&format!("{arch}.{suffix}"));

    let block_count = arch_int("block_count");
    let swa_layers = match values.get(&format!("{arch}.attention.sliding_window_pattern")) {
        Some(Value::Bools(pattern)) => Some(pattern.clone()),
        // Gemma 3 non scrive lo schema: llama.cpp usa un layer globale ogni sei.
        _ if arch == "gemma3" && arch_int("attention.sliding_window").is_some() => {
            block_count.map(|n| (0..n).map(|i| (i + 1) % 6 != 0).collect())
        }
        _ => None,
    };

    // Un elenco dei tensori illeggibile non rende inutilizzabile il modello:
    // la stima resta quella prudente.
    let alignment = int("general.alignment").filter(|a| *a > 0).unwrap_or(32);
    let tensors = read_tensors(&mut r, tensor_count, alignment).unwrap_or_default();
    let tensor_bytes = |name: &str| {
        tensors
            .iter()
            .find(|(n, _)| n == name)
            .map_or(0, |(_, bytes)| *bytes)
    };
    let has_output = tensors.iter().any(|(n, _)| n == "output.weight");

    let layout = Layout {
        key_length: arch_int("attention.key_length"),
        value_length: arch_int("attention.value_length"),
        key_length_swa: arch_int("attention.key_length_swa"),
        value_length_swa: arch_int("attention.value_length_swa"),
        sliding_window: arch_int("attention.sliding_window"),
        swa_layers,
        shared_kv_layers: arch_int("attention.shared_kv_layers"),
        token_embd_bytes: tensor_bytes("token_embd.weight"),
        per_layer_embd_bytes: tensor_bytes("per_layer_token_embd.weight"),
        tied_output: !tensors.is_empty() && !has_output,
    };

    Ok(GgufMeta {
        layout,
        name: text("general.name"),
        size_label: text("general.size_label"),
        quantization: int("general.file_type").and_then(file_type_name),
        context_length: arch_int("context_length"),
        block_count,
        embedding_length: arch_int("embedding_length"),
        head_count: arch_int("attention.head_count"),
        head_count_kv: arch_int("attention.head_count_kv"),
        has_chat_template: values.contains_key("tokenizer.chat_template"),
        supports_thinking: text("tokenizer.chat_template").is_some_and(|t| template_thinks(&t)),
        architecture,
    })
}

fn read_value<R: Read + Seek>(r: &mut R, kind: u32) -> Result<Value> {
    Ok(match kind {
        0 => Value::Int(read_bytes::<1, _>(r)?[0] as u64),
        2 => Value::Int(u16::from_le_bytes(read_bytes(r)?) as u64),
        4 => Value::Int(read_u32(r)? as u64),
        10 => Value::Int(read_u64(r)?),
        // Gli interi con segno ci interessano solo se non negativi.
        1 => int_or_other(i8::from_le_bytes(read_bytes(r)?) as i64),
        3 => int_or_other(i16::from_le_bytes(read_bytes(r)?) as i64),
        5 => int_or_other(i32::from_le_bytes(read_bytes(r)?) as i64),
        11 => int_or_other(i64::from_le_bytes(read_bytes(r)?)),
        8 => Value::Text(read_string(r)?),
        9 => {
            let elem_kind = read_u32(r)?;
            let count = read_u64(r)?;
            if elem_kind == 7 && count <= MAX_BOOL_ARRAY {
                let mut bools = vec![0u8; count as usize];
                r.read_exact(&mut bools)?;
                Value::Bools(bools.into_iter().map(|b| b != 0).collect())
            } else {
                skip_array(r, elem_kind, count)?;
                Value::Other
            }
        }
        other => {
            skip(r, scalar_size(other)?)?;
            Value::Other
        }
    })
}

/// Un template di chat che prevede il ragionamento: Qwen3 e Gemma 4
/// (`enable_thinking`), DeepSeek R1 (`<think>`), gpt-oss
/// (`reasoning_effort`)… Si guarda il template, non il vocabolario, dove
/// la parola "thinking" compare anche nei modelli che non ragionano.
fn template_thinks(template: &str) -> bool {
    [
        "enable_thinking",
        "<think>",
        "reasoning_content",
        "reasoning_effort",
        "thinking",
    ]
    .iter()
    .any(|k| template.contains(k))
}

/// Oltre questo numero di tensori il file è quasi certamente corrotto.
const MAX_TENSORS: u64 = 1 << 20;

/// Legge l'elenco dei tensori e ne restituisce nome e dimensione in byte.
/// La dimensione si ricava dalla distanza tra un tensore e il successivo
/// nella sezione dei dati; per l'ultimo, dalla fine del file.
fn read_tensors<R: Read + Seek>(
    r: &mut R,
    count: u64,
    alignment: u64,
) -> Result<Vec<(String, u64)>> {
    if count > MAX_TENSORS {
        return Err(Error::msg("file GGUF corrotto (troppi tensori)"));
    }
    let mut tensors = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = read_string(r)?;
        let dims = read_u32(r)?;
        if dims > 8 {
            return Err(Error::msg("file GGUF corrotto (troppe dimensioni)"));
        }
        skip(r, u64::from(dims) * 8)?;
        let _kind = read_u32(r)?;
        let offset = read_u64(r)?;
        tensors.push((name, offset));
    }
    let header_end = r.stream_position()?;
    let data_start = header_end.div_ceil(alignment) * alignment;
    let file_end = r.seek(std::io::SeekFrom::End(0))?;
    let data_len = file_end.saturating_sub(data_start);

    let mut offsets: Vec<u64> = tensors.iter().map(|(_, o)| *o).collect();
    offsets.sort_unstable();
    offsets.dedup();
    Ok(tensors
        .into_iter()
        .map(|(name, offset)| {
            let next = offsets
                .get(offsets.partition_point(|o| *o <= offset))
                .copied()
                .unwrap_or(data_len);
            (name, next.saturating_sub(offset))
        })
        .collect())
}

fn int_or_other(n: i64) -> Value {
    u64::try_from(n).map(Value::Int).unwrap_or(Value::Other)
}

/// Dimensione in byte dei tipi a lunghezza fissa.
fn scalar_size(kind: u32) -> Result<u64> {
    Ok(match kind {
        0 | 1 | 7 => 1,
        2 | 3 => 2,
        4..=6 => 4,
        10..=12 => 8,
        other => return Err(Error::msg(format!("tipo GGUF sconosciuto: {other}"))),
    })
}

/// Salta un array senza caricarlo: quelli del vocabolario hanno centinaia di
/// migliaia di elementi e non ci servono.
fn skip_array<R: Read + Seek>(r: &mut R, elem_kind: u32, count: u64) -> Result<()> {
    match elem_kind {
        8 => {
            for _ in 0..count {
                let len = read_u64(r)?;
                skip(r, len)?;
            }
        }
        9 => {
            for _ in 0..count {
                let inner_kind = read_u32(r)?;
                let inner_count = read_u64(r)?;
                skip_array(r, inner_kind, inner_count)?;
            }
        }
        other => {
            let bytes = scalar_size(other)?
                .checked_mul(count)
                .ok_or_else(|| Error::msg("array GGUF troppo grande"))?;
            skip(r, bytes)?;
        }
    }
    Ok(())
}

fn skip<R: Seek>(r: &mut R, bytes: u64) -> Result<()> {
    let offset = i64::try_from(bytes).map_err(|_| Error::msg("file GGUF corrotto"))?;
    r.seek_relative(offset)?;
    Ok(())
}

fn read_bytes<const N: usize, R: Read>(r: &mut R) -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_u32<R: Read>(r: &mut R) -> Result<u32> {
    Ok(u32::from_le_bytes(read_bytes(r)?))
}

fn read_u64<R: Read>(r: &mut R) -> Result<u64> {
    Ok(u64::from_le_bytes(read_bytes(r)?))
}

fn read_string<R: Read>(r: &mut R) -> Result<String> {
    let len = read_u64(r)?;
    if len > MAX_STRING {
        return Err(Error::msg("file GGUF corrotto (stringa troppo lunga)"));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Nome della quantizzazione dal campo `general.file_type`.
fn file_type_name(file_type: u64) -> Option<String> {
    let name = match file_type {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        21 => "Q2_K_S",
        22 => "IQ3_XS",
        23 => "IQ3_XXS",
        24 => "IQ1_S",
        25 => "IQ4_NL",
        26 => "IQ3_S",
        27 => "IQ3_M",
        28 => "IQ2_S",
        29 => "IQ2_M",
        30 => "IQ4_XS",
        31 => "IQ1_M",
        32 => "BF16",
        _ => return None,
    };
    Some(name.to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    fn put_str(out: &mut Vec<u8>, s: &str) {
        out.extend((s.len() as u64).to_le_bytes());
        out.extend(s.as_bytes());
    }

    fn put_text(out: &mut Vec<u8>, key: &str, value: &str) {
        put_str(out, key);
        out.extend(8u32.to_le_bytes());
        put_str(out, value);
    }

    fn put_u32(out: &mut Vec<u8>, key: &str, value: u32) {
        put_str(out, key);
        out.extend(4u32.to_le_bytes());
        out.extend(value.to_le_bytes());
    }

    /// Un GGUF minimo ma valido, usato anche dai test degli altri moduli.
    pub(crate) fn sample_gguf() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(b"GGUF");
        out.extend(3u32.to_le_bytes());
        out.extend(0u64.to_le_bytes()); // tensori
        out.extend(9u64.to_le_bytes()); // coppie chiave-valore

        put_text(&mut out, "general.architecture", "llama");
        put_text(&mut out, "general.name", "Modello di prova");
        put_u32(&mut out, "general.file_type", 15);
        put_u32(&mut out, "llama.context_length", 4096);
        put_u32(&mut out, "llama.block_count", 12);
        put_u32(&mut out, "llama.embedding_length", 768);
        put_u32(&mut out, "llama.attention.head_count", 12);
        put_u32(&mut out, "llama.attention.head_count_kv", 4);

        // Un array di stringhe, come il vocabolario: va saltato.
        put_str(&mut out, "tokenizer.ggml.tokens");
        out.extend(9u32.to_le_bytes());
        out.extend(8u32.to_le_bytes());
        out.extend(2u64.to_le_bytes());
        put_str(&mut out, "ciao");
        put_str(&mut out, "mondo");
        out
    }

    pub(crate) fn write_sample(path: &Path) {
        File::create(path)
            .unwrap()
            .write_all(&sample_gguf())
            .unwrap();
    }

    #[test]
    fn legge_i_metadati() {
        let file = std::env::temp_dir().join(format!("gguf-test-{}.gguf", std::process::id()));
        write_sample(&file);
        let meta = read_meta(&file).unwrap();
        let _ = std::fs::remove_file(&file);

        assert_eq!(meta.name.as_deref(), Some("Modello di prova"));
        assert_eq!(meta.architecture.as_deref(), Some("llama"));
        assert_eq!(meta.quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(meta.context_length, Some(4096));
        assert_eq!(meta.block_count, Some(12));
        assert_eq!(meta.embedding_length, Some(768));
        assert_eq!(meta.head_count, Some(12));
        assert_eq!(meta.head_count_kv, Some(4));
        assert!(!meta.has_chat_template);
        assert!(!meta.supports_thinking);
    }

    #[test]
    fn riconosce_i_template_che_ragionano() {
        assert!(template_thinks(
            "{% if enable_thinking %}<|think|>{% endif %}"
        ));
        assert!(template_thinks("{{ '<think>\\n' }}"));
        assert!(!template_thinks(
            "{% for m in messages %}<|im_start|>{{ m.role }}{% endfor %}"
        ));
    }

    #[test]
    fn rifiuta_un_file_che_non_e_gguf() {
        let file = std::env::temp_dir().join(format!("gguf-bad-{}.gguf", std::process::id()));
        std::fs::write(&file, b"non sono un modello").unwrap();
        let result = read_meta(&file);
        let _ = std::fs::remove_file(&file);
        assert!(result.is_err());
    }
}
