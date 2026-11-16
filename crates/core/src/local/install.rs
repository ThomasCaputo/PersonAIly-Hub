//! Download e installazione di llama.cpp nella cartella dati dell'app.
//!
//! I file vengono presi dalla release ufficiale indicata da
//! [`LLAMA_CPP_BUILD`](super::LLAMA_CPP_BUILD) e controllati con la loro
//! impronta SHA-256, scritta qui nel codice: un file alterato o incompleto
//! viene rifiutato prima di essere estratto.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::LLAMA_CPP_BUILD;
use crate::error::{Error, Result};

const RELEASE_URL: &str = "https://github.com/ggml-org/llama.cpp/releases/download";

/// Un archivio da scaricare.
pub struct EngineFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// Versione di llama.cpp adatta all'hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    /// GPU NVIDIA.
    Cuda,
    /// Altre GPU (AMD, Intel).
    Vulkan,
    /// Solo processore.
    Cpu,
}

impl Variant {
    /// Tutte le varianti, nell'ordine in cui si propongono.
    pub const ALL: [Variant; 3] = [Variant::Cuda, Variant::Vulkan, Variant::Cpu];

    pub fn id(self) -> &'static str {
        match self {
            Self::Cuda => "cuda",
            Self::Vulkan => "vulkan",
            Self::Cpu => "cpu",
        }
    }

    /// La variante di una cartella di installazione (`b12110-cuda`).
    pub fn from_dir_name(name: &str) -> Option<Variant> {
        let (_, id) = name.rsplit_once('-')?;
        Self::ALL.into_iter().find(|v| v.id() == id)
    }

    /// Archivi che compongono questa variante (solo Windows x64, per ora).
    pub fn files(self) -> &'static [EngineFile] {
        match self {
            Self::Cuda => &[
                EngineFile {
                    name: "llama-b12110-bin-win-cuda-13.4-x64.zip",
                    size: 169_175_655,
                    sha256: "dc910a969190b5d42e189afd73117f6bbdd8dd8ed6586342c5e0b82cab62c673",
                },
                // Librerie CUDA di NVIDIA, necessarie per usare la GPU.
                EngineFile {
                    name: "cudart-llama-bin-win-cuda-13.4-x64.zip",
                    size: 423_535_356,
                    sha256: "738f8c251ac22b70c3ae6f83a10cf222725df0395246a2cf58f32bdb85fbe668",
                },
            ],
            Self::Vulkan => &[EngineFile {
                name: "llama-b12110-bin-win-vulkan-x64.zip",
                size: 37_661_146,
                sha256: "9732a5e48887ca54d0bfe4af4324dc85fd56cd8cda19b52df3542fa6ebe6874f",
            }],
            Self::Cpu => &[EngineFile {
                name: "llama-b12110-bin-win-cpu-x64.zip",
                size: 24_708_445,
                sha256: "98901e327c252875accaf2d1bf364e10e868843b0b63468baad1aa945cdc5da2",
            }],
        }
    }

    pub fn download_bytes(self) -> u64 {
        self.files().iter().map(|f| f.size).sum()
    }

    /// Nome della cartella in cui la variante viene installata.
    pub fn dir_name(self) -> String {
        format!("{LLAMA_CPP_BUILD}-{}", self.id())
    }
}

/// Fase dell'installazione, per la barra di avanzamento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Download,
    Extract,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: Phase,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

/// Scarica, controlla ed estrae una variante di llama.cpp in `bin_dir`.
/// Restituisce la cartella di installazione.
pub async fn install(
    http: &reqwest::Client,
    bin_dir: &Path,
    variant: Variant,
    mut on_progress: impl FnMut(Progress),
) -> Result<PathBuf> {
    let download_dir = bin_dir.join("_download");
    let staging_dir = bin_dir.join(format!("{}.tmp", variant.dir_name()));
    let final_dir = bin_dir.join(variant.dir_name());

    // Resti di un'installazione interrotta.
    let _ = std::fs::remove_dir_all(&download_dir);
    let _ = std::fs::remove_dir_all(&staging_dir);
    std::fs::create_dir_all(&download_dir)?;

    let total_bytes = variant.download_bytes();
    let mut done_bytes = 0;
    let mut archives = Vec::new();

    for file in variant.files() {
        let path = download_dir.join(file.name);
        let url = format!("{RELEASE_URL}/{LLAMA_CPP_BUILD}/{}", file.name);
        download(http, &url, &path, file, |chunk| {
            done_bytes += chunk;
            on_progress(Progress {
                phase: Phase::Download,
                done_bytes,
                total_bytes,
            });
        })
        .await?;
        archives.push(path);
    }

    on_progress(Progress {
        phase: Phase::Extract,
        done_bytes: total_bytes,
        total_bytes,
    });

    let staging = staging_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        std::fs::create_dir_all(&staging)?;
        for archive in &archives {
            let mut zip = zip::ZipArchive::new(File::open(archive)?)
                .map_err(|e| Error::msg(format!("archivio non valido: {e}")))?;
            // `extract` rifiuta i percorsi che uscirebbero dalla cartella di destinazione.
            zip.extract(&staging)
                .map_err(|e| Error::msg(format!("estrazione non riuscita: {e}")))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))??;

    // Si sostituisce la cartella definitiva solo a estrazione completata.
    let _ = std::fs::remove_dir_all(&final_dir);
    std::fs::rename(&staging_dir, &final_dir)?;
    let _ = std::fs::remove_dir_all(&download_dir);
    Ok(final_dir)
}

/// Scarica un file controllandone dimensione e impronta.
async fn download(
    http: &reqwest::Client,
    url: &str,
    path: &Path,
    expected: &EngineFile,
    mut on_chunk: impl FnMut(u64),
) -> Result<()> {
    let mut response = http.get(url).send().await?.error_for_status()?;
    let mut out = File::create(path)?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;

    while let Some(chunk) = response.chunk().await? {
        size += chunk.len() as u64;
        if size > expected.size {
            return Err(Error::msg(format!(
                "{}: il file scaricato è più grande del previsto",
                expected.name
            )));
        }
        hasher.update(&chunk);
        out.write_all(&chunk)?;
        on_chunk(chunk.len() as u64);
    }
    out.flush()?;

    let sha256 = hex(&hasher.finalize());
    if size != expected.size || sha256 != expected.sha256 {
        let _ = std::fs::remove_file(path);
        return Err(Error::msg(format!(
            "{}: il file scaricato non corrisponde a quello atteso (download incompleto o alterato)",
            expected.name
        )));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_varianti_dichiarano_file_coerenti() {
        for variant in [Variant::Cuda, Variant::Vulkan, Variant::Cpu] {
            assert!(!variant.files().is_empty());
            for file in variant.files() {
                assert!(file.name.ends_with(".zip"));
                assert_eq!(file.sha256.len(), 64);
                assert!(file.size > 0);
            }
            assert_eq!(
                variant.dir_name(),
                format!("{LLAMA_CPP_BUILD}-{}", variant.id())
            );
        }
        // Il programma vero e proprio porta nel nome la build fissata.
        assert!(Variant::Cuda.files()[0].name.contains(LLAMA_CPP_BUILD));
    }

    #[test]
    fn impronta_in_esadecimale() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
