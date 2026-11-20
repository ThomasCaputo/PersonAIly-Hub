//! Punti di ripristino delle cartelle degli agenti (ADR 0015).
//!
//! Prima che un agente lavori, ogni sua cartella viene fotografata con git in
//! un repository **separato**, dentro la cartella dati dell'app: il repository
//! git dell'utente (se c'è) non viene toccato, né il suo indice né i suoi rami.
//! Contano le regole di `.gitignore` della cartella, più alcune cartelle
//! pesanti che non si fotografano mai (`node_modules`, `target`…).
//!
//! Ripristinare riporta la cartella com'era: i file cambiati tornano indietro,
//! quelli cancellati ricompaiono, quelli creati dopo spariscono. I file
//! ignorati non si toccano. Prima di ripristinare si scatta un'altra foto,
//! così anche il ripristino si può annullare.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Cartelle che non si fotografano mai: dipendenze e risultati di compilazione,
/// grandi e ricostruibili.
const ALWAYS_EXCLUDED: &[&str] = &[
    "node_modules/",
    "target/",
    ".venv/",
    "venv/",
    "__pycache__/",
    ".gradle/",
    "*.gguf",
];

/// File più grandi di così non entrano nell'istantanea.
const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// Una cartella fotografata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RestorePoint {
    pub folder: String,
    /// Il commit dell'istantanea nel repository separato.
    pub commit: String,
    /// File fotografati.
    pub files: u32,
}

/// `git` è installato e risponde.
pub fn git_available() -> bool {
    let mut cmd = Command::new("git");
    cmd.arg("--version");
    hide_window(&mut cmd);
    cmd.output().is_ok_and(|o| o.status.success())
}

/// Il repository separato di una cartella: uno per cartella, così le foto
/// successive condividono i file uguali.
fn repo_for(data_dir: &Path, folder: &Path) -> PathBuf {
    let key = folder.to_string_lossy().to_lowercase();
    let hash = Sha256::digest(key.as_bytes());
    let name: String = hash.iter().take(8).map(|b| format!("{b:02x}")).collect();
    data_dir.join("snapshots").join(format!("{name}.git"))
}

/// Un comando git sul repository separato con `folder` come cartella di lavoro.
fn git(repo: &Path, folder: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("--git-dir")
        .arg(repo)
        .arg("--work-tree")
        .arg(folder)
        // Un'identità solo per questi commit: niente configurazione dell'utente.
        .args([
            "-c",
            "user.name=PersonAIly Hub",
            "-c",
            "user.email=snapshots@personaily.invalid",
        ])
        // I file si salvano così come sono, senza conversioni di fine riga.
        .args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false"])
        .args(["-c", "core.longpaths=true", "-c", "commit.gpgsign=false"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(folder);
    hide_window(&mut cmd);
    cmd
}

fn run(mut cmd: Command) -> Result<String> {
    let output = cmd
        .output()
        .map_err(|e| Error::msg(format!("git non si avvia: {e}")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(Error::msg(format!("git: {}", err.trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Prepara il repository separato (la prima volta) e le esclusioni.
fn prepare(repo: &Path, folder: &Path) -> Result<()> {
    if !repo.join("HEAD").exists() {
        std::fs::create_dir_all(repo)?;
        let mut init = Command::new("git");
        init.args(["init", "--quiet", "--bare"]).arg(repo);
        hide_window(&mut init);
        run(init)?;
        let mut cmd = git(repo, folder);
        cmd.args(["config", "core.bare", "false"]);
        run(cmd)?;
    }
    let info = repo.join("info");
    std::fs::create_dir_all(&info)?;
    std::fs::write(info.join("exclude"), ALWAYS_EXCLUDED.join("\n") + "\n")?;
    Ok(())
}

/// Aggiunge all'indice tutto ciò che c'è nella cartella (salvo gli esclusi e
/// i file troppo grandi) e ne fa un commit, appeso al riferimento `reference`.
fn commit_all(repo: &Path, folder: &Path, message: &str, reference: &str) -> Result<RestorePoint> {
    // L'indice deve rispecchiare la cartella di adesso, non la foto precedente.
    let mut cmd = git(repo, folder);
    cmd.args(["add", "--all", "--ignore-errors", "."]);
    // File bloccati da altri programmi non fermano la foto.
    let _ = run(cmd);

    // I file troppo grandi escono dall'indice: la foto resta leggera.
    let mut list = git(repo, folder);
    list.args(["ls-files", "-z", "--cached"]);
    let listed = run(list)?;
    let big: Vec<&str> = listed
        .split('\0')
        .filter(|f| !f.is_empty())
        .filter(|f| std::fs::metadata(folder.join(f)).is_ok_and(|m| m.len() > MAX_FILE_BYTES))
        .collect();
    for chunk in big.chunks(100) {
        let mut cmd = git(repo, folder);
        cmd.args(["rm", "--cached", "--quiet", "--"]).args(chunk);
        run(cmd)?;
    }
    let files = listed.split('\0').filter(|f| !f.is_empty()).count() - big.len();

    let mut tree = git(repo, folder);
    tree.arg("write-tree");
    let tree = run(tree)?;
    let mut commit = git(repo, folder);
    commit.args(["commit-tree", &tree, "-m", message]);
    let commit = run(commit)?;
    let mut update = git(repo, folder);
    update.args(["update-ref", reference, &commit]);
    run(update)?;
    Ok(RestorePoint {
        folder: folder.to_string_lossy().into_owned(),
        commit,
        files: files as u32,
    })
}

/// Fotografa la cartella. `reference` tiene viva la foto (per esempio
/// `refs/sessions/12`): senza, git potrebbe cancellarla facendo pulizia.
pub fn create(
    data_dir: &Path,
    folder: &Path,
    message: &str,
    reference: &str,
) -> Result<RestorePoint> {
    if !folder.is_dir() {
        return Err(Error::msg(format!(
            "La cartella {} non esiste.",
            folder.display()
        )));
    }
    let repo = repo_for(data_dir, folder);
    prepare(&repo, folder)?;
    commit_all(&repo, folder, message, reference)
}

/// Riporta la cartella com'era nella foto. Prima ne scatta un'altra, appesa a
/// `safety_reference`, che viene restituita: serve ad annullare il ripristino.
pub fn restore(
    data_dir: &Path,
    point: &RestorePoint,
    safety_reference: &str,
) -> Result<RestorePoint> {
    let folder = Path::new(&point.folder);
    let repo = repo_for(data_dir, folder);
    if !repo.join("HEAD").exists() {
        return Err(Error::msg("Il punto di ripristino non c'è più."));
    }
    prepare(&repo, folder)?;
    let safety = commit_all(&repo, folder, "Prima del ripristino", safety_reference)?;
    // `read-tree -u --reset` porta indice e cartella alla foto: cambia i file
    // diversi, rimette i mancanti, toglie quelli nell'indice ma non nella foto
    // (cioè i file nuovi, appena aggiunti da `commit_all`).
    let mut cmd = git(&repo, folder);
    cmd.args(["read-tree", "-u", "--reset", &point.commit]);
    run(cmd)?;
    Ok(safety)
}

/// Dimentica le foto appese ai riferimenti indicati (anche con `*`, per
/// esempio tutte quelle di una sessione eliminata): git le cancella alla
/// prossima pulizia automatica.
pub fn forget(data_dir: &Path, folder: &Path, patterns: &[&str]) -> Result<()> {
    let repo = repo_for(data_dir, folder);
    if !repo.join("HEAD").exists() {
        return Ok(());
    }
    let mut list = git(&repo, folder);
    list.args(["for-each-ref", "--format=%(refname)"])
        .args(patterns);
    for reference in run(list)?.lines().filter(|l| !l.is_empty()) {
        let mut delete = git(&repo, folder);
        delete.args(["update-ref", "-d", reference]);
        run(delete)?;
    }
    let mut gc = git(&repo, folder);
    gc.args(["gc", "--auto", "--quiet"]);
    let _ = run(gc);
    Ok(())
}

/// Su Windows evita che si apra una finestra di console.
fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::local::runtime::CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("snapshots-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ripristina_cambiati_cancellati_e_nuovi() {
        if !git_available() {
            return;
        }
        let root = temp("giro");
        let data = root.join("dati");
        let work = root.join("progetto");
        std::fs::create_dir_all(work.join("src")).unwrap();
        std::fs::write(work.join("a.txt"), "uno\r\n").unwrap();
        std::fs::write(work.join("src").join("b.txt"), "due").unwrap();
        std::fs::write(work.join(".gitignore"), "ignorato.log\n").unwrap();
        std::fs::write(work.join("ignorato.log"), "prima").unwrap();
        std::fs::create_dir_all(work.join("node_modules")).unwrap();
        std::fs::write(work.join("node_modules").join("x.js"), "x").unwrap();

        let point = create(&data, &work, "Prima della sessione 1", "refs/sessions/1").unwrap();
        assert_eq!(point.files, 3); // a.txt, src/b.txt, .gitignore

        // L'agente lavora: cambia, cancella, crea.
        std::fs::write(work.join("a.txt"), "cambiato").unwrap();
        std::fs::remove_file(work.join("src").join("b.txt")).unwrap();
        std::fs::write(work.join("nuovo.txt"), "nuovo").unwrap();
        std::fs::write(work.join("ignorato.log"), "dopo").unwrap();

        let safety = restore(&data, &point, "refs/sessions/1-prima-del-ripristino").unwrap();
        assert_eq!(
            std::fs::read_to_string(work.join("a.txt")).unwrap(),
            "uno\r\n"
        );
        assert_eq!(
            std::fs::read_to_string(work.join("src").join("b.txt")).unwrap(),
            "due"
        );
        assert!(!work.join("nuovo.txt").exists());
        // Ignorati ed esclusi restano come sono.
        assert_eq!(
            std::fs::read_to_string(work.join("ignorato.log")).unwrap(),
            "dopo"
        );
        assert!(work.join("node_modules").join("x.js").exists());

        // Anche il ripristino si annulla.
        restore(&data, &safety, "refs/sessions/1-annullato").unwrap();

        // Dimenticare la sessione toglie tutti i suoi riferimenti.
        forget(&data, &work, &["refs/sessions/1", "refs/sessions/1-*"]).unwrap();
        let repo = repo_for(&data, &work);
        let mut refs = git(&repo, &work);
        refs.args(["for-each-ref", "refs/sessions"]);
        assert_eq!(run(refs).unwrap(), "");
        assert_eq!(
            std::fs::read_to_string(work.join("a.txt")).unwrap(),
            "cambiato"
        );
        assert!(work.join("nuovo.txt").exists());
        assert!(!work.join("src").join("b.txt").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn non_tocca_il_repository_dell_utente() {
        if !git_available() {
            return;
        }
        let root = temp("utente");
        let data = root.join("dati");
        let work = root.join("progetto");
        std::fs::create_dir_all(&work).unwrap();
        let mut init = Command::new("git");
        init.args(["init", "--quiet"]).current_dir(&work);
        init.output().unwrap();
        std::fs::write(work.join("a.txt"), "uno").unwrap();

        create(&data, &work, "foto", "refs/sessions/2").unwrap();
        // L'indice dell'utente è ancora vuoto: nessun file aggiunto per lui.
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&work)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "?? a.txt");
        let _ = std::fs::remove_dir_all(&root);
    }
}
