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
}

enum Value {
    Int(u64),
    Text(String),
    Other,
}

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
    let _tensor_count = read_u64(&mut r)?;
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

    Ok(GgufMeta {
        name: text("general.name"),
        size_label: text("general.size_label"),
        quantization: int("general.file_type").and_then(file_type_name),
        context_length: arch_int("context_length"),
        block_count: arch_int("block_count"),
        embedding_length: arch_int("embedding_length"),
        head_count: arch_int("attention.head_count"),
        head_count_kv: arch_int("attention.head_count_kv"),
        has_chat_template: values.contains_key("tokenizer.chat_template"),
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
            skip_array(r, elem_kind, count)?;
            Value::Other
        }
        other => {
            skip(r, scalar_size(other)?)?;
            Value::Other
        }
    })
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
