//! Ricerca e download di modelli GGUF da Hugging Face.
//!
//! Il download scrive in un file `.part` accanto alla destinazione: se viene
//! interrotto riprende da dove era arrivato, e il file prende il nome
//! definitivo solo dopo il controllo dell'impronta SHA-256. Così in libreria
//! non compaiono mai modelli incompleti.
//!
//! Ogni repository finisce in una sua sottocartella (`autore/nome`): i
//! componenti visivi (`mmproj`) hanno spesso lo stesso nome in repository
//! diversi, e devono stare accanto al loro modello.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use crate::chat;
use crate::error::{Error, Result};
use crate::models;

const HUB: &str = "https://huggingface.co";
const SEARCH_LIMIT: usize = 25;

/// Un repository di modelli.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubRepo {
    pub id: String,
    pub downloads: u64,
    pub likes: u64,
}

/// Che cosa contiene un file GGUF.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HubFileKind {
    #[default]
    Model,
    /// Componente visivo (`mmproj`): fa leggere le immagini al modello accanto.
    Vision,
}

/// Un file come lo pubblica il sito.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubPart {
    pub path: String,
    pub size_bytes: u64,
    /// Impronta dichiarata dal sito, usata per verificare il download.
    pub sha256: Option<String>,
}

/// Un modello (o componente visivo) scaricabile: un file, oppure le parti di
/// un modello diviso (`-00001-of-00003.gguf`), che si scaricano insieme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubFile {
    /// Il file, o la prima parte.
    pub path: String,
    /// Dimensione totale.
    pub size_bytes: u64,
    /// Impronta del file; per un modello diviso è in `parts`.
    pub sha256: Option<String>,
    /// Tutte le parti di un modello diviso; vuoto per un file solo.
    #[serde(default)]
    pub parts: Vec<HubPart>,
    #[serde(default)]
    pub kind: HubFileKind,
}

impl HubFile {
    /// I file da scaricare, nell'ordine.
    pub fn pieces(&self) -> Vec<HubPart> {
        if self.parts.is_empty() {
            vec![HubPart {
                path: self.path.clone(),
                size_bytes: self.size_bytes,
                sha256: self.sha256.clone(),
            }]
        } else {
            self.parts.clone()
        }
    }

    /// `true` se di almeno un file il sito non pubblica l'impronta.
    pub fn unverified(&self) -> bool {
        self.pieces().iter().any(|p| p.sha256.is_none())
    }
}

/// Com'è finito un download.
#[derive(Debug, PartialEq)]
pub enum DownloadEnd {
    Completed(PathBuf),
    /// Interrotto dall'utente: il file parziale resta, per poter riprendere.
    Cancelled,
}

/// Cerca i repository che contengono file GGUF, dai più scaricati.
pub async fn search(http: &reqwest::Client, query: &str) -> Result<Vec<HubRepo>> {
    let url = format!(
        "{HUB}/api/models?search={}&filter=gguf&sort=downloads&direction=-1&limit={SEARCH_LIMIT}",
        encode_query(query.trim())
    );
    let response = http.get(url).send().await?;
    if !response.status().is_success() {
        return Err(chat::http_error(response).await);
    }
    Ok(parse_search(&response.json::<Value>().await?))
}

fn parse_search(value: &Value) -> Vec<HubRepo> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|repo| {
            Some(HubRepo {
                id: repo.get("id")?.as_str()?.to_string(),
                downloads: repo.get("downloads").and_then(Value::as_u64).unwrap_or(0),
                likes: repo.get("likes").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect()
}

/// I file GGUF di un repository, dal più piccolo al più grande.
pub async fn files(http: &reqwest::Client, repo: &str) -> Result<Vec<HubFile>> {
    check_repo(repo)?;
    let response = http
        .get(format!("{HUB}/api/models/{repo}/tree/main?recursive=true"))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(chat::http_error(response).await);
    }
    Ok(parse_files(&response.json::<Value>().await?))
}

/// I file GGUF del repository: modelli in un file solo, modelli divisi (le
/// cui parti diventano una voce sola, se ci sono tutte) e componenti visivi.
/// Prima i modelli, dal più piccolo; in fondo i componenti visivi.
fn parse_files(value: &Value) -> Vec<HubFile> {
    let parts: Vec<HubPart> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let path = entry.get("path")?.as_str()?.to_string();
            path.to_lowercase().ends_with(".gguf").then(|| HubPart {
                size_bytes: entry.get("size").and_then(Value::as_u64).unwrap_or(0),
                sha256: entry
                    .pointer("/lfs/oid")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                path,
            })
        })
        .collect();

    let mut files = Vec::new();
    for part in &parts {
        let name = part.path.rsplit('/').next().unwrap_or(&part.path);
        if models::is_mmproj(name) {
            files.push(HubFile {
                path: part.path.clone(),
                size_bytes: part.size_bytes,
                sha256: part.sha256.clone(),
                parts: Vec::new(),
                kind: HubFileKind::Vision,
            });
            continue;
        }
        match models::split_part(name) {
            None => files.push(HubFile {
                path: part.path.clone(),
                size_bytes: part.size_bytes,
                sha256: part.sha256.clone(),
                parts: Vec::new(),
                kind: HubFileKind::Model,
            }),
            Some((1, _)) => {
                // Le parti si chiamano come la prima, cambia solo il numero.
                let wanted: Vec<String> = models::split_files(Path::new(&part.path))
                    .iter()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .collect();
                let set: Option<Vec<HubPart>> = wanted
                    .iter()
                    .map(|w| parts.iter().find(|p| p.path == *w).cloned())
                    .collect();
                if let Some(set) = set {
                    files.push(HubFile {
                        path: part.path.clone(),
                        size_bytes: set.iter().map(|p| p.size_bytes).sum(),
                        sha256: None,
                        parts: set,
                        kind: HubFileKind::Model,
                    });
                }
            }
            Some(_) => {}
        }
    }
    files.sort_by_key(|f| (f.kind == HubFileKind::Vision, f.size_bytes));
    files
}

/// La cartella in cui finiscono i file di un repository: `autore/nome`
/// dentro la cartella della libreria.
pub fn repo_dir(library: &Path, repo: &str) -> Result<PathBuf> {
    check_repo(repo)?;
    let mut dir = library.to_path_buf();
    for part in repo.split('/') {
        dir.push(part);
    }
    Ok(dir)
}

/// Scarica un modello (tutte le sue parti, se è diviso) nella sottocartella
/// del repository dentro `library`, riprendendo un eventuale download
/// interrotto. `on_progress` riceve i byte scaricati finora, in totale.
pub async fn download(
    http: &reqwest::Client,
    repo: &str,
    file: &HubFile,
    library: &Path,
    cancel: Arc<Notify>,
    mut on_progress: impl FnMut(u64),
) -> Result<DownloadEnd> {
    let dest_dir = repo_dir(library, repo)?;
    let pieces = file.pieces();
    let mut names = Vec::with_capacity(pieces.len());
    for piece in &pieces {
        names.push(safe_file_name(&piece.path)?);
    }
    if names.iter().all(|name| dest_dir.join(name).exists()) {
        return Err(Error::msg(format!(
            "{} è già presente nella cartella dei modelli.",
            names[0]
        )));
    }
    std::fs::create_dir_all(&dest_dir)?;

    let mut before = 0;
    for (piece, name) in pieces.iter().zip(&names) {
        let dest = dest_dir.join(name);
        // Una parte già completa (download precedente interrotto più avanti)
        // ha superato il controllo dell'impronta prima di prendere il suo nome.
        if !dest.exists() {
            let end = download_piece(http, repo, piece, &dest_dir, name, cancel.clone(), |done| {
                on_progress(before + done)
            })
            .await?;
            if end == DownloadEnd::Cancelled {
                return Ok(DownloadEnd::Cancelled);
            }
        }
        before += piece.size_bytes;
        on_progress(before);
    }
    Ok(DownloadEnd::Completed(dest_dir.join(names[0])))
}

/// Scarica un singolo file in `dest_dir/name`, passando da `name.part`.
async fn download_piece(
    http: &reqwest::Client,
    repo: &str,
    file: &HubPart,
    dest_dir: &Path,
    name: &str,
    cancel: Arc<Notify>,
    mut on_progress: impl FnMut(u64),
) -> Result<DownloadEnd> {
    let dest = dest_dir.join(name);
    let part = dest_dir.join(format!("{name}.part"));

    // Quanto c'è già di un download precedente.
    let mut done = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if done > file.size_bytes {
        done = 0;
    }

    let mut request = http.get(format!("{HUB}/{repo}/resolve/main/{}", file.path));
    if done > 0 {
        request = request.header("Range", format!("bytes={done}-"));
    }
    let mut response = request.send().await?;
    if !response.status().is_success() {
        return Err(chat::http_error(response).await);
    }
    // 206 = il sito riprende dal punto richiesto; 200 = rimanda tutto da capo.
    if done > 0 && response.status().as_u16() != 206 {
        done = 0;
    }

    // L'impronta va calcolata sull'intero file: prima la parte già scaricata,
    // che può essere di molti GB, quindi fuori dal ciclo asincrono.
    let mut hasher = if done > 0 {
        let part = part.clone();
        tokio::task::spawn_blocking(move || hash_file(&part))
            .await
            .map_err(|e| Error::msg(e.to_string()))??
    } else {
        Sha256::new()
    };
    let mut out = OpenOptions::new()
        .create(true)
        .write(true)
        .append(done > 0)
        .truncate(done == 0)
        .open(&part)?;
    on_progress(done);

    loop {
        let chunk = tokio::select! {
            chunk = response.chunk() => chunk?,
            _ = cancel.notified() => {
                out.flush()?;
                return Ok(DownloadEnd::Cancelled);
            }
        };
        let Some(chunk) = chunk else { break };
        hasher.update(&chunk);
        out.write_all(&chunk)?;
        done += chunk.len() as u64;
        on_progress(done);
    }
    out.flush()?;
    drop(out);

    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let size_ok = file.size_bytes == 0 || done == file.size_bytes;
    let hash_ok = file
        .sha256
        .as_ref()
        .is_none_or(|expected| *expected == digest);
    if !size_ok || !hash_ok {
        let _ = std::fs::remove_file(&part);
        return Err(Error::msg(format!(
            "{name}: il file scaricato non corrisponde a quello pubblicato (download rovinato). \
             È stato eliminato: riprova."
        )));
    }

    std::fs::rename(&part, &dest)?;
    Ok(DownloadEnd::Completed(dest))
}

/// L'impronta SHA-256 (ancora da completare) di un file già sul disco.
fn hash_file(path: &Path) -> Result<Sha256> {
    let mut hasher = Sha256::new();
    let mut file = File::open(path)?;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hasher);
        }
        hasher.update(&buffer[..read]);
    }
}

/// Un identificatore di repository ha la forma `autore/nome`, con soli
/// caratteri innocui: lo si mette in un indirizzo, quindi va controllato.
fn check_repo(repo: &str) -> Result<()> {
    let ok_part = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    let mut parts = repo.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(author), Some(name), None) if ok_part(author) && ok_part(name) => Ok(()),
        _ => Err(Error::msg("Nome del repository non valido.")),
    }
}

/// Nome con cui salvare il file: l'ultima parte del percorso, purché innocua.
fn safe_file_name(path: &str) -> Result<&str> {
    let safe_segment = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
    };
    if !path.split('/').all(safe_segment) {
        return Err(Error::msg("Nome del file non valido."));
    }
    Ok(path.rsplit('/').next().unwrap_or(path))
}

/// Codifica il testo cercato per metterlo in un indirizzo.
fn encode_query(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legge_i_risultati_della_ricerca() {
        let repos = parse_search(&json!([
            {"id": "autore/Modello-GGUF", "downloads": 1200, "likes": 30},
            {"id": "altro/Senza-Numeri"},
            {"senza": "id"}
        ]));
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].downloads, 1200);
        assert_eq!(repos[1].likes, 0);
    }

    #[test]
    fn elenca_modelli_divisi_e_componenti_visivi() {
        let files = parse_files(&json!([
            {"type": "file", "path": "README.md", "size": 100},
            {"type": "file", "path": "modello-Q8_0.gguf", "size": 8000,
             "lfs": {"oid": "bb", "size": 8000}},
            {"type": "file", "path": "modello-Q4_K_M.gguf", "size": 4000,
             "lfs": {"oid": "aa", "size": 4000}},
            {"type": "file", "path": "mmproj-modello.gguf", "size": 500},
            {"type": "file", "path": "Q8/grande-00001-of-00002.gguf", "size": 9000,
             "lfs": {"oid": "c1", "size": 9000}},
            {"type": "file", "path": "Q8/grande-00002-of-00002.gguf", "size": 7000,
             "lfs": {"oid": "c2", "size": 7000}},
            {"type": "file", "path": "monco-00001-of-00002.gguf", "size": 9000},
            {"type": "directory", "path": "sottocartella"}
        ]));
        assert_eq!(
            files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            [
                "modello-Q4_K_M.gguf",
                "modello-Q8_0.gguf",
                "Q8/grande-00001-of-00002.gguf",
                "mmproj-modello.gguf"
            ]
        );
        assert_eq!(files[0].sha256.as_deref(), Some("aa"));
        assert_eq!(files[2].size_bytes, 16000);
        assert_eq!(files[2].parts.len(), 2);
        assert_eq!(files[2].pieces()[1].sha256.as_deref(), Some("c2"));
        assert_eq!(files[3].kind, HubFileKind::Vision);
        assert!(files[3].unverified());
        assert!(!files[2].unverified());
    }

    #[test]
    fn ogni_repository_ha_la_sua_cartella() {
        let dir = repo_dir(Path::new("modelli"), "autore/Nome-GGUF").unwrap();
        assert_eq!(dir, Path::new("modelli").join("autore").join("Nome-GGUF"));
        assert!(repo_dir(Path::new("modelli"), "../fuori").is_err());
    }

    #[test]
    fn rifiuta_nomi_che_potrebbero_uscire_dalla_cartella() {
        assert!(check_repo("bartowski/Qwen2.5-7B-Instruct-GGUF").is_ok());
        assert!(check_repo("solo-un-pezzo").is_err());
        assert!(check_repo("a/b/c").is_err());
        assert!(check_repo("../segreto").is_err());
        assert!(check_repo("autore/nome?x=1").is_err());

        assert_eq!(safe_file_name("modello.gguf").unwrap(), "modello.gguf");
        assert_eq!(
            safe_file_name("cartella/modello.gguf").unwrap(),
            "modello.gguf"
        );
        assert!(safe_file_name("../modello.gguf").is_err());
        assert!(safe_file_name("a/../../modello.gguf").is_err());
        assert!(safe_file_name("C:\\modello.gguf").is_err());
    }

    #[test]
    fn codifica_il_testo_cercato() {
        assert_eq!(encode_query("qwen 2.5"), "qwen%202.5");
        assert_eq!(encode_query("a&b=c"), "a%26b%3Dc");
        assert_eq!(encode_query("perché"), "perch%C3%A9");
    }
}
