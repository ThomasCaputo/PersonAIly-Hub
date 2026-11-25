//! Motore llama.cpp, caricamento dei modelli locali, completamento in linea e manutenzione.

use crate::*;

impl Core {
    /// La variante di llama.cpp scelta dall'utente (LO-9).
    pub fn engine_variant(&self) -> Option<Variant> {
        self.store()
            .setting(SETTING_ENGINE_VARIANT)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    /// Sceglie la variante da usare dal prossimo caricamento di un modello.
    pub fn set_engine_variant(&self, variant: Variant) -> Result<()> {
        self.store()
            .set_setting(SETTING_ENGINE_VARIANT, &serde_json::to_string(&variant)?)
    }

    /// La versione di llama.cpp con cui caricare il prossimo modello.
    pub(crate) fn engine(&self) -> Result<InstalledEngine> {
        local::runtime::find_server(&self.data_dir, self.engine_variant())
    }

    /// Dice se llama.cpp è installato, quali varianti ci sono e quale
    /// conviene a questo PC.
    pub async fn engine_status(&self) -> EngineStatus {
        // Senza passare dal motore: mentre un modello si carica è occupato, e
        // llama.cpp risulterebbe "non installato".
        let installed = self.engine().ok().map(|e| e.dir_name);
        // Con una GPU NVIDIA si usa CUDA; altrimenti Vulkan, che copre AMD e Intel.
        let has_nvidia = tokio::task::spawn_blocking(|| sysmon::snapshot().gpu.is_some())
            .await
            .unwrap_or(false);
        let recommended = if has_nvidia {
            Variant::Cuda
        } else {
            Variant::Vulkan
        };
        EngineStatus {
            installed,
            engines: local::runtime::installed_engines(&self.data_dir),
            selected: self.engine_variant(),
            build: local::LLAMA_CPP_BUILD,
            recommended,
            download_bytes: recommended.download_bytes(),
            variants: Variant::ALL
                .into_iter()
                .map(|variant| VariantInfo {
                    variant,
                    download_bytes: variant.download_bytes(),
                })
                .collect(),
        }
    }

    /// Scarica e installa una variante di llama.cpp, che diventa quella in
    /// uso. Una sola installazione alla volta.
    pub async fn install_engine(
        &self,
        variant: Variant,
        on_progress: impl FnMut(install::Progress),
    ) -> Result<()> {
        let _guard = self
            .installing
            .try_lock()
            .map_err(|_| Error::msg("Un'installazione è già in corso."))?;
        let bin_dir = local::runtime::bin_dir(&self.data_dir);
        std::fs::create_dir_all(&bin_dir)?;
        install::install(&self.cloud_http, &bin_dir, variant, on_progress).await?;
        self.set_engine_variant(variant)
    }

    /// Elimina una versione installata di llama.cpp, se nessun modello la usa.
    pub async fn remove_engine(&self, dir_name: &str) -> Result<()> {
        let engine = local::runtime::installed_engines(&self.data_dir)
            .into_iter()
            .find(|e| e.dir_name == dir_name)
            .ok_or_else(|| Error::msg("Questa versione di llama.cpp non è installata."))?;
        let in_use = match self.runtime.try_lock() {
            Ok(mut runtime) => runtime.loaded().is_some_and(|m| m.engine == dir_name),
            // Un caricamento in corso potrebbe usarla.
            Err(_) => true,
        };
        if in_use {
            return Err(Error::msg(
                "Questa versione di llama.cpp è in uso: scarica prima il modello.",
            ));
        }
        let _guard = self
            .installing
            .try_lock()
            .map_err(|_| Error::msg("Un'installazione è in corso: riprova quando finisce."))?;
        if let Some(dir) = engine.exe.parent() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    /// Confronta la memoria necessaria con quella libera nella collocazione scelta.
    ///
    /// Caricare un modello scarica prima quello presente: la memoria che
    /// occupa conta come libera, altrimenti ricaricare lo stesso modello con
    /// altri parametri sembrerebbe impossibile.
    pub fn check_load(&self, model_path: &str, params: &LoadParams) -> LoadCheck {
        let model = models::inspect(Path::new(model_path));
        let estimate = local::estimate_memory(&model, params);
        let mut memory = sysmon::snapshot();
        let loaded_pid = self
            .runtime
            .try_lock()
            .ok()
            .and_then(|mut runtime| runtime.loaded())
            .map(|m| m.pid);
        if let Some(pid) = loaded_pid {
            if let (Some(gpu), Some(process)) =
                (memory.gpu.as_mut(), sysmon::process_gpu_memory(pid))
            {
                gpu.free_bytes = (gpu.free_bytes + process.dedicated_bytes).min(gpu.total_bytes);
            }
            if let Some(ram) = sysmon::process_ram(pid) {
                memory.ram_available_bytes =
                    (memory.ram_available_bytes + ram).min(memory.ram_total_bytes);
            }
        }
        LoadCheck::new(estimate, params.placement, &memory)
    }

    /// Carica un modello. Se la stima dice che non entra, rifiuta, a meno che
    /// `force` non sia `true`.
    pub async fn load_model(
        &self,
        model_path: &str,
        params: &LoadParams,
        force: bool,
    ) -> Result<LoadedModel> {
        let mut runtime = self.runtime.lock().await;
        // Prima si libera la memoria del modello precedente, poi si misura.
        runtime.unload().await;

        let model = models::inspect(Path::new(model_path));
        if let Some(error) = &model.error {
            return Err(Error::msg(format!(
                "file del modello non leggibile: {error}"
            )));
        }
        if params.placement == Placement::Hybrid {
            let layers = model.block_count.unwrap_or(0);
            match params.gpu_layers {
                Some(n) if n > 0 && (layers == 0 || u64::from(n) < layers) => {}
                _ => {
                    return Err(Error::msg(
                        "Collocazione ibrida: scegli quanti layer mettere sulla GPU (almeno uno, non tutti).",
                    ));
                }
            }
        }
        let engine = self.engine()?;
        let mmproj = model.mmproj.as_deref().filter(|_| params.vision);

        if !force {
            let path = model_path.to_string();
            let params_for_check = params.clone();
            let check = self.check_load_blocking(path, params_for_check).await?;
            if !check.fits {
                let message = match params.placement {
                    Placement::Hybrid => format!(
                        "Il modello non entra: servono circa {} GB di VRAM e {} GB di RAM, liberi {} GB e {} GB.",
                        gib(check.estimate.vram_bytes),
                        gib(check.estimate.ram_bytes),
                        gib(check.available_bytes.unwrap_or(0)),
                        gib(check.ram_available_bytes.unwrap_or(0)),
                    ),
                    placement => format!(
                        "Il modello non entra nella {}: servono circa {} GB, liberi {} GB.",
                        if placement == Placement::Vram {
                            "VRAM"
                        } else {
                            "RAM"
                        },
                        gib(if placement == Placement::Vram {
                            check.estimate.vram_bytes
                        } else {
                            check.estimate.total_bytes
                        }),
                        gib(check.available_bytes.unwrap_or(0)),
                    ),
                };
                return Err(Error::msg(message));
            }
        }

        log::info(format!(
            "caricamento di {} con {} ({:?}, contesto {}, cache {:?}, layer GPU {:?}, immagini {}, ragionamento {})",
            model.name,
            engine.dir_name,
            params.placement,
            params.context_length,
            params.kv_cache_type,
            params.gpu_layers,
            mmproj.is_some(),
            params.thinking
        ));
        let loaded = match runtime
            .load(
                &engine,
                model_path,
                &model.name,
                params,
                mmproj,
                model.thinking,
            )
            .await
        {
            Ok(loaded) => loaded,
            Err(e) => {
                log::error(format!("caricamento non riuscito: {e}"));
                return Err(e);
            }
        };
        // Letti con `dio`, gli embedding che llama.cpp tiene in RAM finiscono
        // in memoria bloccata per la GPU, contata come memoria condivisa: il
        // controllo dello sconfinamento deve saperlo.
        let pinned = if params.effective_load_mode() == "mmap" {
            0
        } else {
            model.layout.host_weights_bytes()
        };
        runtime.set_model_facts(pinned, model.template);
        log::info(format!("modello caricato (processo {})", loaded.pid));
        self.touch_local();
        Ok(loaded)
    }

    pub(crate) async fn check_load_blocking(
        &self,
        path: String,
        params: LoadParams,
    ) -> Result<LoadCheck> {
        // La misura della VRAM avvia un processo esterno: fuori dal ciclo asincrono.
        let model = models::inspect(Path::new(&path));
        let estimate = local::estimate_memory(&model, &params);
        let memory = tokio::task::spawn_blocking(sysmon::snapshot)
            .await
            .map_err(|e| Error::msg(e.to_string()))?;
        Ok(LoadCheck::new(estimate, params.placement, &memory))
    }

    pub async fn unload_model(&self) {
        self.stop_generation();
        self.runtime.lock().await.unload().await;
    }

    pub async fn status(&self) -> RuntimeStatus {
        // Durante un caricamento il runtime è occupato: lo segnaliamo senza attendere.
        let (loading, loaded) = match self.runtime.try_lock() {
            Ok(mut runtime) => (false, runtime.loaded()),
            Err(_) => (true, None),
        };
        let pid = loaded.as_ref().map(|m| m.pid);
        let (memory, process_ram_bytes, process_gpu) = tokio::task::spawn_blocking(move || {
            (
                sysmon::snapshot(),
                pid.and_then(sysmon::process_ram),
                pid.and_then(sysmon::process_gpu_memory),
            )
        })
        .await
        .unwrap_or_default();

        // Solo in VRAM ha senso parlare di sconfinamento: in RAM è tutto lì per
        // scelta. Con la collocazione ibrida i pesi dei layer in RAM compaiono
        // come memoria condivisa della GPU (misurati 1,2-3 GB, ADR 0016): il
        // contatore non distingue uno sconfinamento, quindi non si usa.
        let vram_model = loaded
            .as_ref()
            .filter(|m| m.params.placement == Placement::Vram);
        let vram_spill_bytes = vram_model
            .zip(process_gpu.as_ref())
            .and_then(|(m, gpu)| local::vram_spill(gpu.shared_bytes, m.pinned_host_bytes));

        RuntimeStatus {
            loading,
            loaded,
            memory,
            process_ram_bytes,
            process_gpu,
            vram_spill_bytes,
        }
    }

    /// Completamento del codice in linea (VS-6): il modello locale scrive ciò
    /// che va tra `prefix` (il testo prima del cursore) e `suffix` (quello
    /// dopo), con l'endpoint `/infill` di `llama-server`. Serve un modello
    /// addestrato al "fill-in-the-middle", come Qwen2.5-Coder. Nulla si salva:
    /// se il client chiude la richiesta, la generazione si ferma.
    pub async fn infill(&self, prefix: &str, suffix: &str, max_tokens: u32) -> Result<String> {
        let loaded = match self.runtime.try_lock() {
            Ok(mut runtime) => runtime.loaded(),
            Err(_) => return Err(Error::msg("Il modello locale si sta caricando: aspetta.")),
        }
        .ok_or_else(|| Error::msg("Nessun modello locale caricato."))?;
        let body = serde_json::json!({
            "input_prefix": prefix,
            "input_suffix": suffix,
            "n_predict": max_tokens.clamp(1, 512),
            // Poca fantasia: un completamento deve essere prevedibile.
            "temperature": 0.2,
            "top_p": 0.9,
            "cache_prompt": true,
            "stream": false,
        });
        let url = format!("http://127.0.0.1:{}/infill", loaded.port);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&loaded.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::msg(format!("rete: {e}")))?;
        self.touch_local();
        let status = response.status();
        let reply: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::msg(format!("rete: {e}")))?;
        if !status.is_success() {
            let message = reply["error"]["message"].as_str().unwrap_or_default();
            // llama-server rifiuta /infill se il modello non ha i token FIM.
            if message.to_lowercase().contains("infill") || message.to_lowercase().contains("fim") {
                return Err(Error::msg(
                    "Il modello caricato non sa completare il codice in linea (gli manca il \
                     \"fill-in-the-middle\"): carica un modello per il codice, come Qwen2.5-Coder.",
                ));
            }
            return Err(Error::msg(format!("llama-server: {message}")));
        }
        Ok(reply["content"].as_str().unwrap_or_default().to_string())
    }

    /// Segna che il modello locale ha appena lavorato.
    pub(crate) fn touch_local(&self) {
        *self
            .last_local_use
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = std::time::Instant::now();
    }

    /// Scarica il modello locale se è rimasto inattivo più a lungo di quanto
    /// scelto nelle preferenze (LO-8). Restituisce `true` se l'ha scaricato.
    pub async fn unload_if_idle(&self) -> bool {
        let minutes = self.preferences().idle_unload_minutes;
        if minutes == 0 || self.is_busy() {
            return false;
        }
        let idle = self
            .last_local_use
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed();
        if idle < std::time::Duration::from_secs(u64::from(minutes) * 60) {
            return false;
        }
        // Un caricamento in corso tiene occupato il motore: non si tocca.
        let Ok(mut runtime) = self.runtime.try_lock() else {
            return false;
        };
        if runtime.loaded().is_none() {
            return false;
        }
        runtime.unload().await;
        true
    }

    /// Lavori di sottofondo del nucleo, finché il programma resta acceso:
    /// chi lo avvia (app o `app-server`) la esegue come compito a parte.
    pub async fn maintenance(self: Arc<Self>) {
        let mut tick = tokio::time::interval(MAINTENANCE_INTERVAL);
        loop {
            tick.tick().await;
            self.unload_if_idle().await;
            self.stop_embedder_if_idle().await;
            self.close_idle_chat_mcp();
        }
    }
}
