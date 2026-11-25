//! Skill installate dall'utente (ADR 0023).

use crate::skills::{ImportOutcome, Skill, SkillDraft};
use crate::*;

impl Core {
    pub(crate) fn skills_dir(&self) -> PathBuf {
        self.data_dir.join(skills::SKILLS_DIR)
    }

    fn disabled_skills(&self) -> Result<Vec<String>> {
        Ok(match self.store().setting(SETTING_SKILLS_DISABLED)? {
            Some(json) => serde_json::from_str(&json).unwrap_or_default(),
            None => Vec::new(),
        })
    }

    /// Le skill installate, accese e spente, anche quelle illeggibili.
    pub fn list_skills(&self) -> Result<Vec<Skill>> {
        Ok(skills::list(&self.skills_dir(), &self.disabled_skills()?))
    }

    /// Le skill da offrire al modello: accese e leggibili.
    pub(crate) fn enabled_skills(&self) -> Vec<Skill> {
        self.list_skills()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.enabled && s.error.is_none())
            .collect()
    }

    pub fn set_skill_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let mut disabled = self.disabled_skills()?;
        disabled.retain(|n| n != name);
        if !enabled {
            disabled.push(name.to_string());
        }
        self.store()
            .set_setting(SETTING_SKILLS_DISABLED, &serde_json::to_string(&disabled)?)
    }

    /// Importa una skill da una cartella o da un `.zip`. Senza `replace`, se ne
    /// esiste già una con lo stesso nome non cambia nulla e lo dice.
    pub fn import_skill(&self, source: &Path, replace: bool) -> Result<ImportOutcome> {
        let dir = self.skills_dir();
        let is_zip = source
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        let outcome = if is_zip {
            skills::import_zip(&dir, source, replace)?
        } else {
            skills::import_folder(&dir, source, replace)?
        };
        // Una skill reinstallata torna accesa.
        if let Some(skill) = &outcome.installed {
            self.set_skill_enabled(&skill.name, true)?;
            log::info(format!("skill importata: {}", skill.name));
        }
        Ok(outcome)
    }

    /// Crea una skill (`original` assente) o ne modifica nome, descrizione e istruzioni.
    pub fn save_skill(&self, draft: &SkillDraft, original: Option<&str>) -> Result<Skill> {
        std::fs::create_dir_all(self.skills_dir())?;
        let saved = skills::save(&self.skills_dir(), draft, original)?;
        // Il nome nuovo eredita lo stato acceso/spento di quello vecchio.
        if let Some(old) = original.filter(|old| *old != saved.name) {
            let was_enabled = !self.disabled_skills()?.iter().any(|n| n == old);
            self.set_skill_enabled(old, true)?;
            self.set_skill_enabled(&saved.name, was_enabled)?;
        }
        Ok(saved)
    }

    pub fn delete_skill(&self, name: &str) -> Result<()> {
        skills::delete(&self.skills_dir(), name)?;
        self.set_skill_enabled(name, true)
    }

    /// Strumenti e prompt delle skill per un ciclo con lo strumento Skill acceso.
    pub(crate) fn skill_offer(&self, settings: &ToolSettings) -> (Vec<ToolDef>, String) {
        if !settings.skills {
            return (Vec::new(), String::new());
        }
        let enabled = self.enabled_skills();
        if enabled.is_empty() {
            return (Vec::new(), String::new());
        }
        (skills::tool_defs(&enabled), skills::preamble(&enabled))
    }
}
