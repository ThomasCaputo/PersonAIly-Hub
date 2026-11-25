//! Accesso OAuth ai server MCP remoti (ADR 0024).

use crate::mcp::McpServerConfig;
use crate::mcp_oauth::{
    self, ACCESS_KEY, LoginStart, OAuthRecord, OAuthStatus, PendingLogin, REFRESH_KEY,
};
use crate::*;

/// Nel portachiavi, il segreto del client ottenuto con la registrazione dinamica.
const REGISTERED_SECRET_KEY: &str = "oauth:registered_secret";

/// Un accesso aperto nel browser: i dati per completarlo e il modo di annullarlo.
pub(crate) struct LoginSlot {
    login: Option<PendingLogin>,
    cancel: Arc<Notify>,
}

impl Core {
    fn oauth_records(&self) -> Result<HashMap<String, OAuthRecord>> {
        Ok(match self.store().setting(SETTING_MCP_OAUTH)? {
            Some(json) => serde_json::from_str(&json).unwrap_or_default(),
            None => HashMap::new(),
        })
    }

    #[cfg(test)]
    pub(crate) fn oauth_records_for_tests(&self) -> HashMap<String, OAuthRecord> {
        self.oauth_records().unwrap()
    }

    fn set_oauth_record(&self, id: &str, record: Option<OAuthRecord>) -> Result<()> {
        let mut all = self.oauth_records()?;
        match record {
            Some(record) => all.insert(id.to_string(), record),
            None => all.remove(id),
        };
        self.store()
            .set_setting(SETTING_MCP_OAUTH, &serde_json::to_string(&all)?)
    }

    /// Un server salvato, con i segreti presi dal portachiavi.
    fn saved_mcp_server(&self, id: &str) -> Result<McpServerConfig> {
        self.mcp_servers()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::msg("Salva il server prima di fare l'accesso."))
    }

    /// Prepara l'accesso e restituisce la pagina da aprire nel browser. Poi
    /// [`Core::mcp_oauth_finish`] aspetta che l'utente lo completi.
    pub async fn mcp_oauth_start(&self, id: &str) -> Result<LoginStart> {
        let config = self.saved_mcp_server(id)?;
        if !config.uses_oauth() {
            return Err(Error::msg(
                "Questo server non usa l'accesso dal browser: sceglilo nel modulo e salva.",
            ));
        }
        let (login, start) = mcp_oauth::begin(&self.cloud_http, &config).await?;
        let previous = self
            .oauth_logins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id.to_string(),
                LoginSlot {
                    login: Some(login),
                    cancel: Arc::new(Notify::new()),
                },
            );
        // Un accesso lasciato a metà si chiude.
        if let Some(previous) = previous {
            previous.cancel.notify_one();
        }
        Ok(start)
    }

    /// Aspetta il ritorno dal browser (al massimo cinque minuti), scambia il
    /// codice con i token e li salva.
    pub async fn mcp_oauth_finish(&self, id: &str) -> Result<OAuthStatus> {
        let (login, cancel) = {
            let mut logins = self.oauth_logins.lock().unwrap_or_else(|e| e.into_inner());
            let slot = logins
                .get_mut(id)
                .ok_or_else(|| Error::msg("Nessun accesso in corso per questo server."))?;
            let login = slot
                .login
                .take()
                .ok_or_else(|| Error::msg("L'accesso è già in attesa."))?;
            (login, slot.cancel.clone())
        };
        let result = tokio::select! {
            code = tokio::time::timeout(
                mcp_oauth::LOGIN_TIMEOUT,
                mcp_oauth::wait_for_code(&login.listener, &login.state),
            ) => match code {
                Ok(code) => code,
                Err(_) => Err(Error::msg("L'accesso non è stato completato entro 5 minuti: riprova.")),
            },
            _ = cancel.notified() => Err(Error::msg("Accesso annullato.")),
        };
        self.oauth_logins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        let code = result?;
        let tokens = mcp_oauth::exchange(&self.cloud_http, &login, &code).await?;
        let vault = &*self.env_vault;
        let registered_secret = login.client_secret.as_deref().filter(|_| login.registered);
        mcp_oauth::put_secret(vault, id, REGISTERED_SECRET_KEY, registered_secret)?;
        let record = OAuthRecord {
            token_endpoint: login.token_endpoint.clone(),
            client_id: login.client_id.clone(),
            resource: login.resource.clone(),
            expires_at: tokens.expires_in.map(|s| mcp_oauth::now() + s),
            registered_secret: registered_secret.is_some(),
        };
        self.store_tokens(id, record, &tokens)?;
        log::info(format!("accesso OAuth fatto per il server MCP {id}"));
        self.mcp_oauth_status(id)
    }

    fn store_tokens(
        &self,
        id: &str,
        record: OAuthRecord,
        tokens: &mcp_oauth::TokenSet,
    ) -> Result<()> {
        let vault = &*self.env_vault;
        mcp_oauth::put_secret(vault, id, ACCESS_KEY, Some(&tokens.access))?;
        // Un rinnovo senza token di rinnovo nuovo tiene quello di prima.
        if let Some(refresh) = &tokens.refresh {
            mcp_oauth::put_secret(vault, id, REFRESH_KEY, Some(refresh))?;
        }
        self.set_oauth_record(id, Some(record))
    }

    /// Smette di aspettare un accesso aperto nel browser.
    pub fn mcp_oauth_cancel(&self, id: &str) {
        if let Some(slot) = self
            .oauth_logins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
        {
            slot.cancel.notify_one();
        }
    }

    pub fn mcp_oauth_status(&self, id: &str) -> Result<OAuthStatus> {
        let Some(record) = self.oauth_records()?.remove(id) else {
            return Ok(OAuthStatus::default());
        };
        let vault = &*self.env_vault;
        Ok(OAuthStatus {
            signed_in: mcp_oauth::get_secret(vault, id, ACCESS_KEY)?.is_some(),
            expires_at: record.expires_at,
            refreshable: mcp_oauth::get_secret(vault, id, REFRESH_KEY)?.is_some(),
        })
    }

    /// Dimentica l'accesso: token, client registrato e dati nell'archivio.
    pub fn mcp_oauth_logout(&self, id: &str) -> Result<()> {
        let vault = &*self.env_vault;
        for key in [ACCESS_KEY, REFRESH_KEY, REGISTERED_SECRET_KEY] {
            mcp_oauth::put_secret(vault, id, key, None)?;
        }
        self.set_oauth_record(id, None)
    }

    /// Aggiunge il token ai server con accesso OAuth, rinnovandolo se è
    /// scaduto. Un server senza accesso dà un errore che spiega cosa fare.
    pub(crate) async fn authorize_mcp(&self, configs: &mut [McpServerConfig]) -> Result<()> {
        for config in configs.iter_mut().filter(|c| c.uses_oauth()) {
            let id = config.id.clone();
            let sign_in = || {
                Error::msg(format!(
                    "Fai l'accesso al server MCP «{}»: Impostazioni → Server MCP → Modifica → Accedi.",
                    config.name
                ))
            };
            let vault = &*self.env_vault;
            let (Some(record), Some(mut access)) = (
                self.oauth_records()?.remove(&id),
                mcp_oauth::get_secret(vault, &id, ACCESS_KEY)?,
            ) else {
                return Err(sign_in());
            };
            if mcp_oauth::expired(&record) {
                let refresh_token =
                    mcp_oauth::get_secret(vault, &id, REFRESH_KEY)?.ok_or_else(sign_in)?;
                let secret = if record.registered_secret {
                    mcp_oauth::get_secret(vault, &id, REGISTERED_SECRET_KEY)?
                } else {
                    Some(config.client_secret.clone()).filter(|s| !s.is_empty())
                };
                let tokens = mcp_oauth::refresh(
                    &self.cloud_http,
                    &record,
                    &refresh_token,
                    secret.as_deref(),
                )
                .await
                .map_err(|e| {
                    log::error(format!("rinnovo del token OAuth di {id} non riuscito: {e}"));
                    sign_in()
                })?;
                let renewed = OAuthRecord {
                    expires_at: tokens.expires_in.map(|s| mcp_oauth::now() + s),
                    ..record
                };
                self.store_tokens(&id, renewed, &tokens)?;
                access = tokens.access;
            }
            mcp_oauth::apply_token(config, &access);
        }
        Ok(())
    }
}
