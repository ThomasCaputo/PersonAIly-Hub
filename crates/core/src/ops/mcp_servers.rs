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
            self.mcp_oauth_logout(id)?;
        }
        all.retain(|s| s.id != id);
        self.store_mcp_servers(&all)
    }

    /// Avvia un server, ne elenca gli strumenti e lo chiude.
    pub async fn probe_mcp_server(
        &self,
        config: &mcp::McpServerConfig,
    ) -> Result<Vec<mcp::McpToolInfo>> {
        let mut config = config.clone();
        // Con l'accesso OAuth serve il token, che c'è solo per un server salvato.
        if config.uses_oauth() {
            if config.id.is_empty() {
                return Err(Error::msg(
                    "Salva il server e fai l'accesso prima di provarlo.",
                ));
            }
            self.authorize_mcp(std::slice::from_mut(&mut config))
                .await?;
        }
        mcp::probe(&config).await
    }

    /// I server MCP di una conversazione in chat, accesi se non lo sono già
    /// (ADR 0022). Quelli eliminati nel frattempo si saltano. `None` se non ne
    /// resta nessuno; un errore se uno non parte.
    pub(crate) async fn chat_mcp_set(
        &self,
        conversation_id: i64,
        ids: &[String],
        cwd: Option<&Path>,
    ) -> Result<Option<Arc<mcp::McpSet>>> {
        let all = self.mcp_servers()?;
        let mut configs: Vec<mcp::McpServerConfig> = ids
            .iter()
            .filter_map(|id| all.iter().find(|s| &s.id == id).cloned())
            .collect();
        if configs.is_empty() {
            self.close_chat_mcp(conversation_id);
            return Ok(None);
        }
        self.authorize_mcp(&mut configs).await?;
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            serde_json::to_string(&configs)?.hash(&mut hasher);
            cwd.hash(&mut hasher);
            hasher.finish()
        };
        let old = {
            let mut live = self.chat_mcp.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = live.get_mut(&conversation_id)
                && entry.key == key
                && entry.set.is_alive()
            {
                entry.last_used = std::time::Instant::now();
                return Ok(Some(entry.set.clone()));
            }
            live.remove(&conversation_id)
        };
        // I processi vecchi si chiudono prima di avviare quelli nuovi.
        drop(old);

        let set = Arc::new(mcp::McpSet::start(&configs, cwd).await?);
        let evicted = {
            let mut live = self.chat_mcp.lock().unwrap_or_else(|e| e.into_inner());
            let replaced = live.insert(
                conversation_id,
                ChatMcp {
                    key,
                    set: set.clone(),
                    last_used: std::time::Instant::now(),
                },
            );
            let mut evicted: Vec<ChatMcp> = replaced.into_iter().collect();
            // Troppe conversazioni con server accesi: si chiude la meno recente.
            while live.len() > CHAT_MCP_MAX {
                let Some(oldest) = live
                    .iter()
                    .filter(|(id, _)| **id != conversation_id)
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(id, _)| *id)
                else {
                    break;
                };
                evicted.extend(live.remove(&oldest));
            }
            evicted
        };
        // Fuori dal lucchetto: chiudere un processo può richiedere un momento.
        drop(evicted);
        Ok(Some(set))
    }

    /// I server MCP di una conversazione, avviati come per un messaggio: stessa
    /// cartella di lavoro, così si riusano gli stessi processi.
    async fn conversation_mcp(&self, conversation_id: i64) -> Result<Option<Arc<mcp::McpSet>>> {
        let settings = self.conversation_tools(conversation_id)?;
        if settings.mcp_servers.is_empty() {
            return Ok(None);
        }
        let folders = self.granted_folders()?;
        self.chat_mcp_set(
            conversation_id,
            &settings.mcp_servers,
            folders.first().map(Path::new),
        )
        .await
    }

    /// I prompt dei server MCP della conversazione, da offrire come comandi `/`
    /// (ADR 0025).
    pub async fn conversation_prompts(&self, conversation_id: i64) -> Result<Vec<mcp::McpPrompt>> {
        Ok(match self.conversation_mcp(conversation_id).await? {
            Some(set) => set.prompts().await,
            None => Vec::new(),
        })
    }

    /// Il testo di un prompt, con gli argomenti scritti dall'utente.
    pub async fn conversation_prompt(
        &self,
        conversation_id: i64,
        server: usize,
        name: &str,
        arguments: &HashMap<String, String>,
    ) -> Result<String> {
        let set = self
            .conversation_mcp(conversation_id)
            .await?
            .ok_or_else(|| Error::msg("Questa conversazione non usa server MCP."))?;
        set.get_prompt(server, name, arguments).await
    }

    /// Segna come appena usati i server MCP di una conversazione.
    pub(crate) fn touch_chat_mcp(&self, conversation_id: i64) {
        if let Some(entry) = self
            .chat_mcp
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&conversation_id)
        {
            entry.last_used = std::time::Instant::now();
        }
    }

    /// Chiude i server MCP accesi per una conversazione, se ce ne sono.
    pub(crate) fn close_chat_mcp(&self, conversation_id: i64) {
        let entry = self
            .chat_mcp
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&conversation_id);
        drop(entry);
    }

    /// Chiude i server MCP delle conversazioni rimaste senza messaggi per
    /// [`CHAT_MCP_IDLE`]; quelli di una conversazione che sta rispondendo restano.
    /// Restituisce quante conversazioni ne avevano.
    pub(crate) fn close_idle_chat_mcp(&self) -> usize {
        let busy: Vec<i64> = self
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect();
        let closed: Vec<ChatMcp> = {
            let mut live = self.chat_mcp.lock().unwrap_or_else(|e| e.into_inner());
            let idle: Vec<i64> = live
                .iter()
                .filter(|(id, entry)| {
                    !busy.contains(id) && entry.last_used.elapsed() >= CHAT_MCP_IDLE
                })
                .map(|(id, _)| *id)
                .collect();
            idle.iter().filter_map(|id| live.remove(id)).collect()
        };
        closed.len()
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
