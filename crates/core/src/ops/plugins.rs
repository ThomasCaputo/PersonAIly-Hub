//! Plugin installati (ADR 0026).

use crate::plugins::{self, PluginCommand, PluginInstall, PluginRecord};
use crate::*;

/// Un comando `/` di un plugin, per il menu della chat.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandInfo {
    pub plugin: String,
    #[serde(flatten)]
    pub command: PluginCommand,
}

impl Core {
    pub(crate) fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join(plugins::PLUGINS_DIR)
    }

    /// I plugin installati, in ordine di nome.
    pub fn list_plugins(&self) -> Result<Vec<PluginRecord>> {
        let mut list: Vec<PluginRecord> = match self.store().setting(SETTING_PLUGINS)? {
            Some(json) => serde_json::from_str(&json).unwrap_or_default(),
            None => Vec::new(),
        };
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }

    fn store_plugins(&self, list: &[PluginRecord]) -> Result<()> {
        self.store()
            .set_setting(SETTING_PLUGINS, &serde_json::to_string(list)?)
    }

    /// Importa un plugin da una cartella o da un `.zip`. Senza `replace`, se ne
    /// è installato uno con lo stesso nome non cambia nulla e lo dice; con
    /// `replace` il vecchio si disinstalla prima.
    pub fn import_plugin(&self, source: &Path, replace: bool) -> Result<PluginInstall> {
        let dir = self.plugins_dir();
        let staged = bundle::staging_dir(&dir)?;
        let result = self.install_from(source, &staged, replace);
        let _ = std::fs::remove_dir_all(&staged);
        result
    }

    fn install_from(&self, source: &Path, staged: &Path, replace: bool) -> Result<PluginInstall> {
        let is_zip = source
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        if is_zip {
            bundle::extract_zip(
                source,
                staged,
                &plugins::MANIFESTS,
                &plugins::LIMITS,
                plugins::NO_PLUGIN,
                plugins::MANY_PLUGINS,
            )?;
        } else {
            if !plugins::MANIFESTS.iter().any(|m| source.join(m).is_file()) {
                return Err(Error::msg(plugins::NO_PLUGIN));
            }
            bundle::copy_folder(source, staged, &plugins::LIMITS)?;
        }

        // Il nome si legge prima di sapere dove andrà; poi si rilegge con la
        // cartella definitiva, che entra nei comandi dei server.
        let name = plugins::read_contents(staged, staged, &|_| None)?.name;
        let previous = self.list_plugins()?.into_iter().find(|p| p.name == name);
        if previous.is_some() && !replace {
            return Ok(PluginInstall {
                installed: None,
                existing: Some(name),
            });
        }
        let target = self.plugins_dir().join(&name);
        let env = |var: &str| std::env::var(var).ok();
        let contents = plugins::read_contents(staged, &target, &env)?;

        // Una skill con lo stesso nome che non è del plugin ferma tutto, prima
        // di cambiare qualcosa: anche prima di togliere la versione installata,
        // che altrimenti andrebbe persa per niente. Le skill di quella versione
        // non contano: se ne vanno con lei.
        let own: &[String] = previous.as_ref().map_or(&[], |p| &p.skills);
        let existing: Vec<String> = self
            .list_skills()?
            .into_iter()
            .map(|s| s.name)
            .filter(|n| !own.contains(n))
            .collect();
        let clashes: Vec<String> = contents
            .skills
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .filter(|n| existing.contains(n))
            .collect();
        if !clashes.is_empty() {
            return Err(Error::msg(format!(
                "Il plugin contiene skill con lo stesso nome di skill già installate: {}. Eliminale o rinominale prima di installarlo.",
                clashes.join(", ")
            )));
        }

        // Solo ora, con i controlli superati, si toglie la versione installata.
        if previous.is_some() {
            self.uninstall_plugin(&name)?;
        }
        // Una cartella rimasta da un'installazione interrotta si sostituisce.
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::rename(staged, &target)?;

        let mut record = PluginRecord {
            name: contents.name.clone(),
            version: contents.version.clone(),
            description: contents.description.clone(),
            author: contents.author.clone(),
            path: target.to_string_lossy().into_owned(),
            commands: contents.commands.clone(),
            notes: contents.notes.clone(),
            ..Default::default()
        };
        match self.install_parts(&contents, &target, &mut record) {
            Ok(()) => {
                let mut list = self.list_plugins()?;
                list.push(record.clone());
                self.store_plugins(&list)?;
                log::info(format!(
                    "plugin installato: {} {}",
                    record.name, record.version
                ));
                Ok(PluginInstall {
                    installed: Some(record),
                    existing: None,
                })
            }
            Err(error) => {
                // Si toglie ciò che era già stato creato.
                self.remove_parts(&record);
                let _ = std::fs::remove_dir_all(&target);
                Err(error)
            }
        }
    }

    /// Crea skill, server e agenti del plugin, annotandoli in `record` man mano.
    fn install_parts(
        &self,
        contents: &plugins::Contents,
        target: &Path,
        record: &mut PluginRecord,
    ) -> Result<()> {
        let skills_dir = self.skills_dir();
        for skill in &contents.skills {
            let staged = bundle::staging_dir(&skills_dir)?;
            let result = bundle::copy_folder(&target.join(skill), &staged, &skills::LIMITS)
                .and_then(|()| skills::install_staged(&skills_dir, &staged, false));
            let _ = std::fs::remove_dir_all(&staged);
            match result? {
                skills::ImportOutcome {
                    installed: Some(installed),
                    ..
                } => {
                    self.set_skill_enabled(&installed.name, true)?;
                    record.skills.push(installed.name);
                }
                skills::ImportOutcome { existing, .. } => {
                    return Err(Error::msg(format!(
                        "Esiste già una skill di nome «{}».",
                        existing.unwrap_or_default()
                    )));
                }
            }
        }
        for server in &contents.servers {
            let saved = self.save_mcp_server(server.clone())?;
            record.mcp_servers.push(saved.id);
        }
        for agent in &contents.agents {
            let saved = self.save_agent(agent.clone())?;
            record.agents.push(saved.id);
        }
        Ok(())
    }

    /// Toglie skill, server e agenti di un plugin. Ciò che non si trova più
    /// (già tolto a mano) si salta.
    fn remove_parts(&self, record: &PluginRecord) {
        for skill in &record.skills {
            let _ = self.delete_skill(skill);
        }
        for id in &record.mcp_servers {
            let _ = self.delete_mcp_server(id);
        }
        for id in &record.agents {
            let _ = self.delete_agent(*id);
        }
    }

    /// Disinstalla un plugin con tutto ciò che aveva creato.
    pub fn uninstall_plugin(&self, name: &str) -> Result<()> {
        let mut list = self.list_plugins()?;
        let Some(at) = list.iter().position(|p| p.name == name) else {
            return Err(Error::msg("Questo plugin non è installato."));
        };
        let record = list.remove(at);
        self.remove_parts(&record);
        skills::validate_name(&record.name)?;
        let dir = self.plugins_dir().join(&record.name);
        if dir.is_dir() {
            std::fs::remove_dir_all(dir)?;
        }
        self.store_plugins(&list)?;
        log::info(format!("plugin disinstallato: {name}"));
        Ok(())
    }

    /// I comandi `/` di tutti i plugin installati.
    pub fn plugin_commands(&self) -> Result<Vec<PluginCommandInfo>> {
        Ok(self
            .list_plugins()?
            .into_iter()
            .flat_map(|p| {
                p.commands
                    .into_iter()
                    .map(move |command| PluginCommandInfo {
                        plugin: p.name.clone(),
                        command,
                    })
            })
            .collect())
    }

    /// Il testo di un comando di un plugin, con gli argomenti dell'utente.
    pub fn plugin_command_text(&self, plugin: &str, name: &str, arguments: &str) -> Result<String> {
        let record = self
            .list_plugins()?
            .into_iter()
            .find(|p| p.name == plugin)
            .ok_or_else(|| Error::msg("Questo plugin non è installato."))?;
        let command = record
            .commands
            .iter()
            .find(|c| c.name == name)
            .ok_or_else(|| Error::msg("Il plugin non ha più questo comando."))?;
        plugins::command_text(&self.plugins_dir().join(&record.name), command, arguments)
    }
}
