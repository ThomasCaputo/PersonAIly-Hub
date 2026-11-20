//! Provider cloud.

use crate::*;

impl Core {
    pub(crate) fn stored_providers(&self) -> Result<Vec<Provider>> {
        Ok(match self.store().setting(SETTING_PROVIDERS)? {
            Some(json) => serde_json::from_str(&json)?,
            None => Vec::new(),
        })
    }

    pub(crate) fn store_providers(&self, providers: &[Provider]) -> Result<()> {
        self.store()
            .set_setting(SETTING_PROVIDERS, &serde_json::to_string(providers)?)
    }

    pub(crate) fn provider(&self, id: &str) -> Result<Provider> {
        self.stored_providers()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::msg("Provider non trovato: forse è stato eliminato."))
    }

    pub(crate) fn provider_info(provider: Provider) -> ProviderInfo {
        ProviderInfo {
            // Se il portachiavi non risponde, l'interfaccia chiederà di reinserire la chiave.
            has_key: matches!(secrets::api_key(&provider.id), Ok(Some(_))),
            supported_params: provider.kind.supported_params(),
            provider,
        }
    }

    /// I provider configurati, in ordine alfabetico.
    pub fn providers(&self) -> Result<Vec<ProviderInfo>> {
        let mut providers = self.stored_providers()?;
        providers.sort_by_key(|p| p.name.to_lowercase());
        Ok(providers.into_iter().map(Self::provider_info).collect())
    }

    /// Crea un provider (se `id` è vuoto) o ne aggiorna uno esistente.
    /// La chiave API, se indicata, va nel portachiavi di sistema.
    pub fn save_provider(
        &self,
        mut provider: Provider,
        api_key: Option<&str>,
    ) -> Result<ProviderInfo> {
        provider.name = provider.name.trim().to_string();
        provider.base_url = provider.base().trim().to_string();
        providers::validate(&provider)?;

        let mut all = self.stored_providers()?;
        match all.iter_mut().find(|p| p.id == provider.id) {
            Some(existing) => {
                // L'elenco dei modelli non arriva dall'interfaccia: si conserva.
                provider.models = std::mem::take(&mut existing.models);
                *existing = provider.clone();
            }
            None => {
                provider.id = providers::new_id(&provider.name);
                provider.models.clear();
                all.push(provider.clone());
            }
        }
        if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
            secrets::set_api_key(&provider.id, key)?;
        }
        self.store_providers(&all)?;
        Ok(Self::provider_info(provider))
    }

    /// Elimina un provider e la sua chiave.
    pub fn delete_provider(&self, id: &str) -> Result<()> {
        let mut all = self.stored_providers()?;
        all.retain(|p| p.id != id);
        self.store_providers(&all)?;
        secrets::delete_api_key(id)
    }

    /// Scarica dal provider l'elenco aggiornato dei suoi modelli.
    pub async fn refresh_provider_models(&self, id: &str) -> Result<ProviderInfo> {
        let provider = self.provider(id)?;
        let key = secrets::api_key(id)?
            .ok_or_else(|| Error::msg("Inserisci prima la chiave API di questo provider."))?;
        let models = providers::fetch_models(&self.cloud_http, &provider, &key).await?;

        let mut all = self.stored_providers()?;
        let stored = all
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::msg("Provider non trovato: forse è stato eliminato."))?;
        stored.models = models;
        let updated = stored.clone();
        self.store_providers(&all)?;
        Ok(Self::provider_info(updated))
    }
}
