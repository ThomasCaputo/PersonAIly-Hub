//! Libreria dei modelli, preset e download da Hugging Face.

use crate::*;

impl Core {
    pub fn model_folders(&self) -> Result<Vec<String>> {
        Ok(match self.store().setting(SETTING_MODEL_FOLDERS)? {
            Some(json) => serde_json::from_str(&json)?,
            None => Vec::new(),
        })
    }

    pub fn set_model_folders(&self, folders: &[String]) -> Result<()> {
        self.store()
            .set_setting(SETTING_MODEL_FOLDERS, &serde_json::to_string(folders)?)
    }

    pub fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let folders: Vec<PathBuf> = self.model_folders()?.iter().map(PathBuf::from).collect();
        Ok(models::scan(&folders))
    }

    /// File del preset. `model_key` è il percorso del file per un modello
    /// locale, oppure `cloud:<provider>:<modello>` per uno cloud.
    pub(crate) fn preset_path(&self, model_key: &str) -> PathBuf {
        let file_name = if model_key.starts_with(CLOUD_KEY_PREFIX) {
            // Caratteri come `:` e `/` non sono ammessi nei nomi di file.
            model_key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect()
        } else {
            Path::new(model_key)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "modello".to_string())
        };
        self.data_dir
            .join("presets")
            .join(format!("{file_name}.json"))
    }

    /// Parametri salvati per un modello; quelli predefiniti se non ce ne sono.
    pub fn preset(&self, model_key: &str) -> Result<Preset> {
        match std::fs::read_to_string(self.preset_path(model_key)) {
            Ok(json) => Ok(serde_json::from_str(&json)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut preset = Preset::default();
                if model_key.starts_with(CLOUD_KEY_PREFIX) {
                    // Per i servizi cloud si parte dai valori neutri dell'API:
                    // alcuni modelli non ne accettano altri.
                    preset.sampling.temperature = 1.0;
                    preset.sampling.top_p = 1.0;
                }
                Ok(preset)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_preset(&self, model_path: &str, preset: &Preset) -> Result<()> {
        let path = self.preset_path(model_path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(preset)?)?;
        Ok(())
    }

    pub(crate) fn named_presets_dir(&self) -> PathBuf {
        self.data_dir.join("presets").join("named")
    }

    pub fn named_presets(&self) -> Vec<NamedPreset> {
        named_presets::list(&self.named_presets_dir())
    }

    pub fn save_named_preset(&self, preset: &NamedPreset) -> Result<NamedPreset> {
        named_presets::save(&self.named_presets_dir(), preset)
    }

    pub fn delete_named_preset(&self, name: &str) -> Result<()> {
        named_presets::delete(&self.named_presets_dir(), name)
    }

    pub fn export_named_preset(&self, name: &str, destination: &Path) -> Result<()> {
        named_presets::export(&self.named_presets_dir(), name, destination)
    }

    pub fn import_named_preset(&self, source: &Path) -> Result<NamedPreset> {
        named_presets::import(&self.named_presets_dir(), source)
    }

    pub async fn search_hub(&self, query: &str) -> Result<Vec<HubRepo>> {
        hub::search(&self.cloud_http, query).await
    }

    pub async fn hub_files(&self, repo: &str) -> Result<Vec<HubFile>> {
        hub::files(&self.cloud_http, repo).await
    }

    /// I download avviati da quando il nucleo è acceso, in corso e conclusi,
    /// nell'ordine in cui sono partiti.
    pub fn downloads(&self) -> Vec<DownloadState> {
        self.downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(crate) fn update_download(&self, id: &str, change: impl FnOnce(&mut DownloadState)) {
        if let Some(state) = self
            .downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
            .find(|d| d.id == id)
        {
            change(state);
        }
    }

    /// Scarica un modello in una delle cartelle della libreria, nella
    /// sottocartella del suo repository. Più download possono procedere
    /// insieme, ma non due volte lo stesso file. Termina a download concluso:
    /// chi la chiama la esegue in sottofondo e segue l'avanzamento con
    /// [`Core::downloads`].
    pub async fn download_model(&self, repo: &str, file: &HubFile, folder: &str) -> Result<()> {
        // Si scrive solo dove l'utente ha già detto di tenere i modelli.
        if !self.model_folders()?.iter().any(|f| f == folder) {
            return Err(Error::msg(
                "La cartella di destinazione non è tra quelle della libreria.",
            ));
        }

        let id = format!("{repo}/{}", file.path);
        let cancel = Arc::new(Notify::new());
        {
            let mut all = self.downloads.lock().unwrap_or_else(|e| e.into_inner());
            if all
                .iter()
                .any(|d| d.id == id && d.phase == DownloadPhase::Downloading)
            {
                return Err(Error::msg("Questo file si sta già scaricando."));
            }
            // Un download ripetuto prende il posto del tentativo precedente.
            all.retain(|d| d.id != id);
            all.push(DownloadState {
                id: id.clone(),
                repo: repo.to_string(),
                file: file.path.clone(),
                done_bytes: 0,
                total_bytes: file.size_bytes,
                phase: DownloadPhase::Downloading,
                error: None,
                unverified: file.unverified(),
            });
            self.download_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.clone(), cancel.clone());
        }

        let result = hub::download(
            &self.cloud_http,
            repo,
            file,
            Path::new(folder),
            cancel,
            |done| self.update_download(&id, |state| state.done_bytes = done),
        )
        .await;

        self.download_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        self.update_download(&id, |state| match &result {
            Ok(DownloadEnd::Completed(_)) => state.phase = DownloadPhase::Done,
            Ok(DownloadEnd::Cancelled) => state.phase = DownloadPhase::Cancelled,
            Err(e) => {
                state.phase = DownloadPhase::Failed;
                state.error = Some(e.to_string());
            }
        });
        result.map(|_| ())
    }

    /// Interrompe un download in corso; il file parziale resta per riprendere.
    pub fn cancel_download(&self, id: &str) {
        if let Some(cancel) = self
            .download_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
        {
            cancel.notify_one();
        }
    }

    /// Toglie dall'elenco un download concluso.
    pub fn dismiss_download(&self, id: &str) {
        self.downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|d| d.id != id || d.phase == DownloadPhase::Downloading);
    }
}
