//! Preset con nome: insiemi di parametri di generazione e prompt di sistema
//! che l'utente salva, applica a qualsiasi modello, esporta e importa.
//!
//! Ognuno è un file JSON leggibile in `presets/named/`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::local::SamplingParams;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedPreset {
    pub name: String,
    #[serde(default)]
    pub sampling: SamplingParams,
    #[serde(default)]
    pub system_prompt: String,
}

const MAX_NAME_CHARS: usize = 60;

/// Nome del file per un preset: solo caratteri sicuri su ogni sistema.
fn file_name(name: &str) -> String {
    let safe: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.json", safe.trim_matches('.'))
}

fn check_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::msg("Dai un nome al preset."));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::msg(format!(
            "Il nome del preset è troppo lungo (massimo {MAX_NAME_CHARS} caratteri)."
        )));
    }
    Ok(name)
}

/// Tutti i preset della cartella, in ordine alfabetico. I file illeggibili
/// vengono ignorati invece di bloccare l'elenco.
pub fn list(dir: &Path) -> Vec<NamedPreset> {
    let mut presets: Vec<NamedPreset> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|json| serde_json::from_str(&json).ok())
        .collect();
    presets.sort_by_key(|p| p.name.to_lowercase());
    presets
}

/// Salva un preset; uno con lo stesso nome viene sostituito.
pub fn save(dir: &Path, preset: &NamedPreset) -> Result<NamedPreset> {
    let mut preset = preset.clone();
    preset.name = check_name(&preset.name)?.to_string();
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join(file_name(&preset.name)),
        serde_json::to_string_pretty(&preset)?,
    )?;
    Ok(preset)
}

pub fn delete(dir: &Path, name: &str) -> Result<()> {
    match std::fs::remove_file(dir.join(file_name(name))) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Copia un preset in un file scelto dall'utente.
pub fn export(dir: &Path, name: &str, destination: &Path) -> Result<()> {
    std::fs::copy(dir.join(file_name(name)), destination)?;
    Ok(())
}

/// Importa un preset da un file. Se il nome è già usato ne sceglie uno libero,
/// così un'importazione non cancella mai un preset esistente.
pub fn import(dir: &Path, source: &Path) -> Result<NamedPreset> {
    let json = std::fs::read_to_string(source)?;
    let mut preset: NamedPreset = serde_json::from_str(&json)
        .map_err(|_| Error::msg("Il file scelto non è un preset valido."))?;
    let base = check_name(&preset.name)?.to_string();

    let taken = |name: &str| -> bool { path_for(dir, name).exists() };
    if taken(&base) {
        preset.name = (2..)
            .map(|n| format!("{base} ({n})"))
            .find(|candidate| !taken(candidate))
            .unwrap_or(base);
    }
    save(dir, &preset)
}

fn path_for(dir: &Path, name: &str) -> PathBuf {
    dir.join(file_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("named-presets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn preset(name: &str, temperature: f32) -> NamedPreset {
        NamedPreset {
            name: name.to_string(),
            sampling: SamplingParams {
                temperature,
                ..Default::default()
            },
            system_prompt: "Rispondi in italiano.".to_string(),
        }
    }

    #[test]
    fn salva_elenca_ed_elimina() {
        let dir = temp_dir("base");
        assert!(list(&dir).is_empty());

        save(&dir, &preset("Preciso", 0.2)).unwrap();
        save(&dir, &preset("creativo", 1.2)).unwrap();
        // Stesso nome: sostituisce.
        save(&dir, &preset("Preciso", 0.1)).unwrap();

        let all = list(&dir);
        assert_eq!(
            all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["creativo", "Preciso"]
        );
        assert_eq!(all[1].sampling.temperature, 0.1);

        delete(&dir, "creativo").unwrap();
        delete(&dir, "mai esistito").unwrap();
        assert_eq!(list(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn esporta_e_reimporta_senza_sovrascrivere() {
        let dir = temp_dir("scambio");
        save(&dir, &preset("Preciso", 0.2)).unwrap();
        let file = dir
            .join("..")
            .join(format!("esportato-{}.json", std::process::id()));
        export(&dir, "Preciso", &file).unwrap();

        // Il nome esiste già: l'importazione ne sceglie un altro.
        let imported = import(&dir, &file).unwrap();
        assert_eq!(imported.name, "Preciso (2)");
        assert_eq!(imported.sampling.temperature, 0.2);
        assert_eq!(import(&dir, &file).unwrap().name, "Preciso (3)");
        assert_eq!(list(&dir).len(), 3);

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rifiuta_nomi_e_file_non_validi() {
        let dir = temp_dir("errori");
        assert!(save(&dir, &preset("   ", 1.0)).is_err());
        assert!(save(&dir, &preset(&"x".repeat(61), 1.0)).is_err());

        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("non-un-preset.txt");
        std::fs::write(&junk, "ciao").unwrap();
        assert!(import(&dir, &junk).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn i_nomi_con_caratteri_speciali_non_escono_dalla_cartella() {
        assert_eq!(file_name("../../segreto"), "_.._segreto.json");
        assert!(!file_name("a/b\\c:d").contains(['/', '\\', ':']));
    }
}
