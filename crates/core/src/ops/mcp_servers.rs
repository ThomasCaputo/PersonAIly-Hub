//! Server MCP configurati dall'utente.

use crate::*;

impl Core {
    /// I server MCP configurati, in ordine alfabetico, con i valori delle
    /// variabili d'ambiente presi dal portachiavi.
    pub fn mcp_servers(&self) -> Result<Vec<mcp::McpServerConfig>> {
        let mut servers = self.stored_mcp_servers()?;
        for server in &mut servers {
            mcp::open_env(&*self.env_vault, server)?;
        }
        servers.sort_by_key(|s| s.name.to_lowercase());
        Ok(servers)
    }

    /// I server come stanno in archivio: le variabili d'ambiente hanno solo il nome.
    ///
    /// Fino alla Tappa 5 i valori stavano nell'archivio: la prima volta li
    /// sposta nel portachiavi. Se il portachiavi non risponde li lascia dove
    /// sono, per non perderli, e riprova alla volta successiva.
    pub(crate) fn stored_mcp_servers(&self) -> Result<Vec<mcp::McpServerConfig>> {
        let mut servers: Vec<mcp::McpServerConfig> =
            match self.store().setting(SETTING_MCP_SERVERS)? {
                Some(json) => serde_json::from_str(&json)?,
                None => Vec::new(),
            };
        let mut moved = false;
        for server in &mut servers {
            if server.env.iter().any(|(_, value)| !value.is_empty())
                && let Ok(sealed) = mcp::seal_env(&*self.env_vault, server, None)
            {
                *server = sealed;
                moved = true;
            }
        }
        if moved {
            self.store_mcp_servers(&servers)?;
        }
        Ok(servers)
    }

    pub(crate) fn store_mcp_servers(&self, servers: &[mcp::McpServerConfig]) -> Result<()> {
        self.store()
            .set_setting(SETTING_MCP_SERVERS, &serde_json::to_string(servers)?)
    }

    /// Crea (con `id` vuoto) o aggiorna un server MCP. I valori delle
    /// variabili d'ambiente vanno nel portachiavi, non nell'archivio.
    pub fn save_mcp_server(
        &self,
        mut config: mcp::McpServerConfig,
    ) -> Result<mcp::McpServerConfig> {
        config.name = config.name.trim().to_string();
        config.command = config.command.trim().to_string();
        config.url = config.url.trim().to_string();
        for (name, _) in &mut config.headers {
            *name = name.trim().to_string();
        }
        config.args.retain(|a| !a.trim().is_empty());
        for (name, _) in &mut config.env {
            *name = name.trim().to_string();
        }
        config.validate()?;
        let mut all = self.stored_mcp_servers()?;
        match all
            .iter_mut()
            .find(|s| s.id == config.id && !config.id.is_empty())
        {
            Some(existing) => {
                let sealed = mcp::seal_env(&*self.env_vault, &config, Some(existing))?;
                *existing = sealed;
            }
            None => {
                config.id = providers::new_id(&config.name);
                all.push(mcp::seal_env(&*self.env_vault, &config, None)?);
            }
        }
        self.store_mcp_servers(&all)?;
        Ok(config)
    }

    /// Elimina un server MCP, con le sue variabili nel portachiavi. Gli agenti
    /// che lo usavano non lo trovano più e le loro sessioni lo segnalano.
    pub fn delete_mcp_server(&self, id: &str) -> Result<()> {
        let mut all = self.stored_mcp_servers()?;
        if let Some(server) = all.iter().find(|s| s.id == id) {
            mcp::forget_env(&*self.env_vault, server)?;
        }
        all.retain(|s| s.id != id);
        self.store_mcp_servers(&all)
    }

    /// Avvia un server, ne elenca gli strumenti e lo chiude.
    pub async fn probe_mcp_server(
        &self,
        config: &mcp::McpServerConfig,
    ) -> Result<Vec<mcp::McpToolInfo>> {
        mcp::probe(config).await
    }

    pub(crate) fn mcp_servers_for(&self, ids: &[String]) -> Result<Vec<mcp::McpServerConfig>> {
        let all = self.mcp_servers()?;
        ids.iter()
            .map(|id| {
                all.iter().find(|s| &s.id == id).cloned().ok_or_else(|| {
                    Error::msg(
                        "Un server MCP di questo agente è stato eliminato: togli il segno dall'agente o configura di nuovo il server nelle Impostazioni.",
                    )
                })
            })
            .collect()
    }
}
