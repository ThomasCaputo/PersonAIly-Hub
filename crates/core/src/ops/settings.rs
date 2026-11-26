//! Impostazioni: cartelle concesse, strumenti, lingua, preferenze, sicurezza e spesa.

use crate::*;

/// Nome inglese di una lingua da un'etichetta come `it-IT`, per le istruzioni
/// ai modelli; le lingue meno comuni restano come etichetta, che i modelli
/// capiscono lo stesso.
pub(crate) fn language_name(tag: &str) -> String {
    let code = tag
        .split(['-', '_'])
        .next()
        .unwrap_or(tag)
        .to_ascii_lowercase();
    let name = match code.as_str() {
        "it" => "Italian",
        "en" => "English",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "pt" => "Portuguese",
        "nl" => "Dutch",
        "pl" => "Polish",
        "ru" => "Russian",
        "uk" => "Ukrainian",
        "zh" => "Chinese",
        "ja" => "Japanese",
        "ko" => "Korean",
        "ar" => "Arabic",
        "tr" => "Turkish",
        _ => return format!("the language with tag {tag}"),
    };
    name.to_string()
}

impl Core {
    /// Cartelle in cui i modelli possono leggere e scrivere file.
    pub fn granted_folders(&self) -> Result<Vec<String>> {
        Ok(match self.store().setting(SETTING_GRANTED_FOLDERS)? {
            Some(json) => serde_json::from_str(&json)?,
            None => Vec::new(),
        })
    }

    /// Revocare una cartella la toglie anche dai documenti consultabili: da
    /// subito i modelli non ne trovano più i brani.
    pub fn set_granted_folders(&self, folders: &[String]) -> Result<()> {
        self.store()
            .set_setting(SETTING_GRANTED_FOLDERS, &serde_json::to_string(folders)?)?;
        let mut rag = self.rag_settings();
        let before = rag.folders.len();
        rag.folders.retain(|f| folders.contains(f));
        if rag.folders.len() != before {
            self.store()
                .set_setting(SETTING_RAG, &serde_json::to_string(&rag)?)?;
        }
        self.store().rag_keep_folders(&rag.folders)?;
        self.invalidate_rag_vectors();
        Ok(())
    }

    /// Strumenti e permessi con cui nasce una nuova conversazione.
    pub fn tool_defaults(&self) -> Result<ToolSettings> {
        Ok(match self.store().setting(SETTING_TOOL_DEFAULTS)? {
            Some(json) => serde_json::from_str(&json).unwrap_or_default(),
            None => ToolSettings::default(),
        })
    }

    pub fn set_tool_defaults(&self, settings: &ToolSettings) -> Result<()> {
        // Le nuove conversazioni seguono la coda di riserva delle Impostazioni.
        let settings = ToolSettings {
            fallback_models: None,
            ..settings.clone()
        };
        self.store()
            .set_setting(SETTING_TOOL_DEFAULTS, &serde_json::to_string(&settings)?)
    }

    /// Strumenti e permessi di una conversazione; quelli predefiniti finché
    /// l'utente non li cambia.
    pub fn conversation_tools(&self, conversation_id: i64) -> Result<ToolSettings> {
        let saved = self.store().conversation_tools(conversation_id)?;
        match saved.and_then(|json| serde_json::from_str(&json).ok()) {
            Some(settings) => Ok(settings),
            None => self.tool_defaults(),
        }
    }

    pub fn set_conversation_tools(
        &self,
        conversation_id: i64,
        settings: &ToolSettings,
    ) -> Result<()> {
        let mut settings = settings.clone();
        if let Some(models) = &settings.fallback_models {
            settings.fallback_models = Some(normalize_fallbacks(models)?);
        }
        self.store()
            .set_conversation_tools(conversation_id, &serde_json::to_string(&settings)?)
    }

    /// Lingua dell'interfaccia scelta dall'utente (`it`, `en`); `None` finché
    /// non sceglie: l'interfaccia segue allora quella del sistema.
    pub fn language(&self) -> Result<Option<String>> {
        Ok(self
            .store()
            .setting(SETTING_LANGUAGE)?
            .filter(|lang| LANGUAGES.contains(&lang.as_str())))
    }

    pub fn set_language(&self, language: &str) -> Result<()> {
        if !LANGUAGES.contains(&language) {
            return Err(Error::msg(format!("Lingua non disponibile: {language}")));
        }
        self.store().set_setting(SETTING_LANGUAGE, language)
    }

    /// In che lingua devono rispondere i modelli: in quella dell'utente; se
    /// non si capisce, in quella scelta per l'interfaccia o, in mancanza, in
    /// quella del sistema.
    pub(crate) fn language_instruction(&self) -> String {
        let tag = self
            .language()
            .ok()
            .flatten()
            .or_else(sys_locale::get_locale)
            .unwrap_or_else(|| "en".to_string());
        format!(
            "Reply in the language the user writes in. If that is unclear (for example a \
             message made only of code), reply in {}.",
            language_name(&tag)
        )
    }

    /// Preferenze generali (tema, area di notifica, scaricamento automatico).
    pub fn preferences(&self) -> Preferences {
        self.store()
            .setting(SETTING_PREFERENCES)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn set_preferences(&self, preferences: &Preferences) -> Result<()> {
        self.store()
            .set_setting(SETTING_PREFERENCES, &serde_json::to_string(preferences)?)?;
        // Il tempo di inattività si conta da adesso: accorciarlo non deve
        // scaricare il modello all'istante.
        self.touch_local();
        Ok(())
    }

    /// Protezioni che l'utente può spegnere (vedi [`SecuritySettings`]).
    pub fn security_settings(&self) -> SecuritySettings {
        self.store()
            .setting(SETTING_SECURITY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn set_security_settings(&self, settings: &SecuritySettings) -> Result<()> {
        self.store()
            .set_setting(SETTING_SECURITY, &serde_json::to_string(settings)?)
    }

    pub fn spend_limits(&self) -> SpendLimits {
        self.store()
            .setting(SETTING_SPEND_LIMITS)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn set_spend_limits(&self, limits: &SpendLimits) -> Result<()> {
        let valid = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && v >= 0.0);
        if !valid(limits.monthly_usd) || !valid(limits.conversation_usd) {
            return Err(Error::msg(
                "Il limite di spesa deve essere un numero positivo.",
            ));
        }
        self.store()
            .set_setting(SETTING_SPEND_LIMITS, &serde_json::to_string(limits)?)
    }

    /// La coda dei modelli di riserva: modelli cloud da provare, in ordine, quando
    /// quello scelto dà un errore (occupato, troppe richieste, credito finito…).
    pub fn fallback_models(&self) -> Vec<ModelRef> {
        self.store()
            .setting(SETTING_FALLBACK_MODELS)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Salva la coda di riserva delle Impostazioni (vedi [`normalize_fallbacks`]).
    pub fn set_fallback_models(&self, models: &[ModelRef]) -> Result<()> {
        let queue = normalize_fallbacks(models)?;
        self.store()
            .set_setting(SETTING_FALLBACK_MODELS, &serde_json::to_string(&queue)?)
    }

    pub fn spending(&self) -> Result<Spending> {
        // Prima si legge e si rilascia l'archivio: anche `spend_limits` lo usa.
        let month_usd = self.store().spent_this_month()?;
        Ok(Spending {
            month_usd,
            limits: self.spend_limits(),
        })
    }

    /// Ferma una richiesta a un modello cloud se un limite di spesa è già
    /// stato raggiunto. Si controlla prima di ogni richiesta, anche a metà di
    /// un giro di strumenti o del lavoro di un agente.
    pub(crate) fn check_spend(&self, conversation_id: i64) -> Result<()> {
        let limits = self.spend_limits();
        let store = self.store();
        if let Some(limit) = limits.monthly_usd {
            let spent = store.spent_this_month()?;
            if spent >= limit {
                return Err(Error::msg(format!(
                    "Limite di spesa mensile raggiunto: spesi circa {spent:.2} $ su {limit:.2} $. \
                     Puoi alzarlo in Impostazioni → Spesa cloud, o usare un modello locale."
                )));
            }
        }
        if let Some(limit) = limits.conversation_usd {
            let spent = store.spent_in_conversation(conversation_id)?;
            if spent >= limit {
                return Err(Error::msg(format!(
                    "Limite di spesa per conversazione raggiunto: spesi circa {spent:.2} $ su \
                     {limit:.2} $. Puoi alzarlo in Impostazioni → Spesa cloud, o continuare in \
                     una conversazione nuova."
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn exfiltration_guard(&self) -> bool {
        self.security_settings().exfiltration_guard
    }
}
