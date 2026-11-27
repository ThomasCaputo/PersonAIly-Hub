//! Motore llama.cpp, caricamento dei modelli locali, completamento in linea e manutenzione.

use crate::local::pool::Engine;
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
        // Un caricamento in corso potrebbe usarla.
        let in_use = self
            .engine_list()
            .iter()
            .any(|e| e.is_busy() || e.loaded().is_some_and(|m| m.engine == dir_name));
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

    // ── Modelli caricati (ADR 0029) ─────────────────────────────────────

    /// I motori, in ordine di caricamento.
    pub(crate) fn engine_list(&self) -> Vec<Arc<Engine>> {
        let mut list = self
            .engines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        list.sort_by_key(|e| e.seq);
        list
    }

    fn find_engine(&self, key: &str) -> Option<Arc<Engine>> {
        self.engines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|e| e.key == key)
            .cloned()
    }

    fn remove_engine_slot(&self, key: &str) {
        self.engines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|e| e.key != key);
        let mut primary = self.primary.lock().unwrap_or_else(|e| e.into_inner());
        if primary.as_deref() == Some(key) {
            *primary = None;
        }
    }

    /// I modelli locali caricati, in ordine di caricamento. Quelli che si
    /// stanno caricando non ci sono ancora.
    pub fn loaded_models(&self) -> Vec<LoadedModel> {
        self.engine_list()
            .iter()
            .filter_map(|e| e.loaded())
            .collect()
    }

    /// Il motore del modello principale: quello scelto, o altrimenti quello
    /// caricato da più tempo.
    fn primary_engine(&self) -> Option<Arc<Engine>> {
        let chosen = self
            .primary
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let list = self.engine_list();
        chosen
            .and_then(|key| {
                list.iter()
                    .find(|e| e.key == key && e.loaded().is_some())
                    .cloned()
            })
            .or_else(|| list.into_iter().find(|e| e.loaded().is_some()))
    }

    /// Il modello principale, se ce n'è uno caricato.
    pub fn primary_model(&self) -> Option<LoadedModel> {
        self.primary_engine().and_then(|e| e.loaded())
    }

    /// Il modello locale da usare: quello indicato o, senza percorso, il
    /// principale. Segna che sta per lavorare.
    pub(crate) fn local_model(&self, path: Option<&str>) -> Result<LoadedModel> {
        let engine = match path {
            None => self.primary_engine(),
            Some(path) => self.find_engine(&local::pool::key(path)),
        };
        if let Some(engine) = &engine
            && let Some(model) = engine.loaded()
        {
            engine.touch();
            return Ok(model);
        }
        let loading = match &engine {
            Some(engine) => engine.is_busy(),
            None => path.is_none() && self.engine_list().iter().any(|e| e.is_busy()),
        };
        Err(Error::msg(match (path, loading) {
            (_, true) => "Il modello locale si sta caricando: aspetta.".to_string(),
            (None, false) => "Nessun modello locale caricato.".to_string(),
            (Some(path), false) => format!(
                "Il modello locale «{}» non è caricato: caricalo nella pagina Modelli.",
                model_file_name(path)
            ),
        }))
    }

    /// Rende principale un modello già caricato.
    pub fn set_primary_model(&self, path: &str) -> Result<()> {
        let key = local::pool::key(path);
        if self.find_engine(&key).and_then(|e| e.loaded()).is_none() {
            return Err(Error::msg("Questo modello non è caricato."));
        }
        *self.primary.lock().unwrap_or_else(|e| e.into_inner()) = Some(key);
        Ok(())
    }

    /// La memoria da misurare per un caricamento: quella libera, più quella
    /// dei modelli che il caricamento toglierà, meno il margine se il modello
    /// andrà accanto ad altri.
    fn load_check(&self, model_path: &str, params: &LoadParams, replace: bool) -> LoadCheck {
        let model = models::inspect(Path::new(model_path));
        let estimate = local::estimate_memory(&model, params);
        let mut memory = sysmon::snapshot();
        let key = local::pool::key(model_path);
        let loaded = self.loaded_models();
        let (leaving, staying): (Vec<_>, Vec<_>) = loaded
            .iter()
            .partition(|m| replace || local::pool::key(&m.model_path) == key);
        for pid in leaving.iter().map(|m| m.pid) {
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
        if !staying.is_empty() {
            reduce_by_reserve(&mut memory);
        }
        LoadCheck::new(estimate, params.placement, &memory)
    }

    /// Confronta la memoria necessaria con quella libera nella collocazione
    /// scelta, come farebbe "Carica".
    ///
    /// Caricare al posto degli altri li scarica prima: la memoria che occupano
    /// conta come libera, e così quella del modello stesso se lo si ricarica
    /// con altri parametri. Accanto ad altri modelli si tiene un margine.
    pub fn check_load(&self, model_path: &str, params: &LoadParams) -> LoadCheck {
        let replace = !self.preferences().multi_model;
        self.load_check(model_path, params, replace)
    }

    /// Come [`Core::check_load`], dal ciclo asincrono.
    pub(crate) async fn check_load_async(
        &self,
        model_path: &str,
        params: &LoadParams,
        replace: bool,
    ) -> Result<LoadCheck> {
        let model = models::inspect(Path::new(model_path));
        let estimate = local::estimate_memory(&model, params);
        let key = local::pool::key(model_path);
        let others: Vec<(u32, bool)> = self
            .loaded_models()
            .iter()
            .map(|m| (m.pid, replace || local::pool::key(&m.model_path) == key))
            .collect();
        let placement = params.placement;
        // La misura della VRAM avvia un processo esterno: fuori dal ciclo asincrono.
        tokio::task::spawn_blocking(move || {
            let mut memory = sysmon::snapshot();
            for &(pid, leaving) in &others {
                if !leaving {
                    continue;
                }
                if let (Some(gpu), Some(process)) =
                    (memory.gpu.as_mut(), sysmon::process_gpu_memory(pid))
                {
                    gpu.free_bytes =
                        (gpu.free_bytes + process.dedicated_bytes).min(gpu.total_bytes);
                }
                if let Some(ram) = sysmon::process_ram(pid) {
                    memory.ram_available_bytes =
                        (memory.ram_available_bytes + ram).min(memory.ram_total_bytes);
                }
            }
            if others.iter().any(|&(_, leaving)| !leaving) {
                reduce_by_reserve(&mut memory);
            }
            LoadCheck::new(estimate, placement, &memory)
        })
        .await
        .map_err(|e| Error::msg(e.to_string()))
    }

    /// Carica un modello: accanto agli altri se "Più modelli locali insieme"
    /// è acceso, altrimenti al loro posto. Diventa il principale. Se la stima
    /// dice che non entra, rifiuta, a meno che `force` non sia `true`.
    pub async fn load_model(
        &self,
        model_path: &str,
        params: &LoadParams,
        force: bool,
    ) -> Result<LoadedModel> {
        self.load_model_with(
            model_path,
            params,
            LoadOptions {
                force,
                ..LoadOptions::default()
            },
        )
        .await
    }

    /// Carica un modello con le scelte di [`LoadOptions`].
    pub async fn load_model_with(
        &self,
        model_path: &str,
        params: &LoadParams,
        options: LoadOptions,
    ) -> Result<LoadedModel> {
        let preferences = self.preferences();
        let replace = options.replace_others || !preferences.multi_model;
        let key = local::pool::key(model_path);

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

        let others: Vec<Arc<Engine>> = self
            .engine_list()
            .into_iter()
            .filter(|e| e.key != key)
            .collect();
        if replace {
            // Prima si libera la memoria degli altri modelli, poi si misura.
            for other in &others {
                other.runtime.lock().await.unload().await;
                self.remove_engine_slot(&other.key);
            }
        } else if preferences.max_local_models > 0
            && others.len() >= preferences.max_local_models as usize
        {
            return Err(Error::msg(format!(
                "Ci sono già {} modelli locali caricati, il massimo che hai scelto: scaricane \
                 uno, o carica questo al posto degli altri.",
                others.len()
            )));
        }

        // Il motore di questo modello: quello che c'è già (si ricarica) o uno nuovo.
        let slot = {
            let mut list = self.engines.lock().unwrap_or_else(|e| e.into_inner());
            match list.iter().find(|e| e.key == key) {
                Some(slot) => slot.clone(),
                None => {
                    let used: Vec<usize> = list.iter().map(|e| e.log_index).collect();
                    let log_index = (0..).find(|i| !used.contains(i)).unwrap_or(0);
                    let seq = self
                        .engine_seq
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let slot = Arc::new(Engine::new(
                        key.clone(),
                        seq,
                        log_index,
                        self.data_dir.clone(),
                        self.http.clone(),
                    ));
                    list.push(slot.clone());
                    slot
                }
            }
        };
        let mut runtime = slot.runtime.lock().await;
        runtime.unload().await;
        let result = self
            .start_engine(
                &mut runtime,
                &engine,
                model_path,
                &model,
                params,
                mmproj,
                replace,
                options.force,
                !others.is_empty() && !replace,
            )
            .await;
        match result {
            Ok(loaded) => {
                drop(runtime);
                slot.touch();
                if options.make_primary {
                    *self.primary.lock().unwrap_or_else(|e| e.into_inner()) = Some(key);
                }
                Ok(loaded)
            }
            Err(e) => {
                drop(runtime);
                if slot.loaded().is_none() {
                    self.remove_engine_slot(&key);
                }
                Err(e)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_engine(
        &self,
        runtime: &mut LocalRuntime,
        engine: &InstalledEngine,
        model_path: &str,
        model: &ModelInfo,
        params: &LoadParams,
        mmproj: Option<&str>,
        replace: bool,
        force: bool,
        alongside: bool,
    ) -> Result<LoadedModel> {
        if !force {
            let check = self.check_load_async(model_path, params, replace).await?;
            if !check.fits {
                let mut message = match params.placement {
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
                if alongside {
                    message.push_str(
                        " Con altri modelli caricati l'app tiene libero un margine per il \
                         sistema: scarica un altro modello, o carica questo al posto degli altri.",
                    );
                }
                return Err(Error::msg(message));
            }
        }

        log::info(format!(
            "caricamento di {} con {} ({:?}, contesto {}, cache {:?}, layer GPU {:?}, immagini {}, ragionamento {}{})",
            model.name,
            engine.dir_name,
            params.placement,
            params.context_length,
            params.kv_cache_type,
            params.gpu_layers,
            mmproj.is_some(),
            params.thinking,
            if alongside { ", accanto ad altri" } else { "" }
        ));
        let loaded = match runtime
            .load(
                engine,
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
        Ok(runtime.loaded().unwrap_or(loaded))
    }

    /// Per agenti e pianificazioni: carica `path` accanto agli altri, senza
    /// renderlo principale, se c'è posto (o se non c'è nient'altro caricato).
    /// `Some(true)` se l'ha caricato, `Some(false)` se c'era già, `None` se
    /// non c'è posto o un solo modello alla volta non lo permette.
    pub(crate) async fn load_alongside(&self, path: &str) -> Result<Option<bool>> {
        let key = local::pool::key(path);
        // Un altro (per esempio un aiutante in parallelo) lo sta già caricando.
        if let Some(engine) = self.find_engine(&key) {
            drop(engine.runtime.lock().await);
            if engine.loaded().is_some() {
                return Ok(Some(false));
            }
        }
        let others = self
            .loaded_models()
            .iter()
            .filter(|m| local::pool::key(&m.model_path) != key)
            .count();
        let preferences = self.preferences();
        let allowed = others == 0
            || (preferences.multi_model
                && (preferences.max_local_models == 0
                    || others < preferences.max_local_models as usize));
        if !allowed {
            return Ok(None);
        }
        let params = self.preset(path)?.load;
        if others > 0 && !self.check_load_async(path, &params, false).await?.fits {
            return Ok(None);
        }
        self.load_model_with(
            path,
            &params,
            LoadOptions {
                make_primary: false,
                ..LoadOptions::default()
            },
        )
        .await?;
        Ok(Some(true))
    }

    /// Scarica tutti i modelli locali.
    pub async fn unload_model(&self) {
        for engine in self.engine_list() {
            engine.runtime.lock().await.unload().await;
            self.remove_engine_slot(&engine.key);
        }
    }

    /// Scarica un solo modello locale.
    pub async fn unload_local(&self, model_path: &str) {
        let key = local::pool::key(model_path);
        if let Some(engine) = self.find_engine(&key) {
            engine.runtime.lock().await.unload().await;
            self.remove_engine_slot(&key);
        }
    }

    pub async fn status(&self) -> RuntimeStatus {
        let list = self.engine_list();
        let loading = list.iter().any(|e| e.is_busy());
        let primary_key = self.primary_engine().map(|e| e.key.clone());
        let models: Vec<(LoadedModel, bool)> = list
            .iter()
            .filter_map(|e| {
                e.loaded()
                    .map(|m| (m, primary_key.as_deref() == Some(e.key.as_str())))
            })
            .collect();
        let pids: Vec<u32> = models.iter().map(|(m, _)| m.pid).collect();
        let (memory, per_process) = tokio::task::spawn_blocking(move || {
            let per: Vec<(Option<u64>, Option<ProcessGpuMemory>)> = pids
                .iter()
                .map(|&pid| (sysmon::process_ram(pid), sysmon::process_gpu_memory(pid)))
                .collect();
            (sysmon::snapshot(), per)
        })
        .await
        .unwrap_or_default();

        let mut entries = Vec::new();
        for ((model, primary), (ram, gpu)) in models.into_iter().zip(per_process) {
            // Solo in VRAM ha senso parlare di sconfinamento: in RAM è tutto lì
            // per scelta. Con la collocazione ibrida i pesi dei layer in RAM
            // compaiono come memoria condivisa della GPU (misurati 1,2-3 GB,
            // ADR 0016): il contatore non distingue uno sconfinamento.
            let spill = gpu
                .filter(|_| model.params.placement == Placement::Vram)
                .and_then(|g| local::vram_spill(g.shared_bytes, model.pinned_host_bytes));
            entries.push(LocalModelStatus {
                model,
                primary,
                process_ram_bytes: ram,
                process_gpu: gpu,
                vram_spill_bytes: spill,
            });
        }
        let main = entries.iter().find(|e| e.primary);
        let sum = |f: &dyn Fn(&LocalModelStatus) -> Option<u64>| {
            let values: Vec<u64> = entries.iter().filter_map(f).collect();
            (!values.is_empty()).then(|| values.iter().sum())
        };
        let process_gpu = {
            let all: Vec<ProcessGpuMemory> = entries.iter().filter_map(|e| e.process_gpu).collect();
            (!all.is_empty()).then(|| ProcessGpuMemory {
                dedicated_bytes: all.iter().map(|g| g.dedicated_bytes).sum(),
                shared_bytes: all.iter().map(|g| g.shared_bytes).sum(),
            })
        };
        let multi_model = self.preferences().multi_model;
        let (room_vram_bytes, room_ram_bytes) = if multi_model && !entries.is_empty() {
            let mut reduced = memory.clone();
            reduce_by_reserve(&mut reduced);
            (
                reduced.gpu.as_ref().map(|g| g.free_bytes),
                Some(reduced.ram_available_bytes),
            )
        } else {
            (None, None)
        };
        RuntimeStatus {
            loading,
            loaded: main.map(|e| e.model.clone()),
            process_ram_bytes: sum(&|e| e.process_ram_bytes),
            process_gpu,
            vram_spill_bytes: sum(&|e| e.vram_spill_bytes),
            memory,
            multi_model,
            room_vram_bytes,
            room_ram_bytes,
            models: entries,
        }
    }

    /// Completamento del codice in linea (VS-6): il modello locale principale
    /// scrive ciò che va tra `prefix` (il testo prima del cursore) e `suffix`
    /// (quello dopo), con l'endpoint `/infill` di `llama-server`. Serve un
    /// modello addestrato al "fill-in-the-middle", come Qwen2.5-Coder. Nulla
    /// si salva: se il client chiude la richiesta, la generazione si ferma.
    pub async fn infill(&self, prefix: &str, suffix: &str, max_tokens: u32) -> Result<String> {
        self.note_local_activity();
        let loaded = self.local_model(None)?;
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
        self.touch_local_model(&loaded.model_path);
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

    /// Segna che un modello locale ha appena lavorato.
    pub(crate) fn touch_local_model(&self, model_path: &str) {
        if let Some(engine) = self.find_engine(&local::pool::key(model_path)) {
            engine.touch();
        }
    }

    /// Fa ripartire da adesso il tempo di inattività di tutti i modelli.
    pub(crate) fn touch_local(&self) {
        for engine in self.engine_list() {
            engine.touch();
        }
    }

    /// Scarica i modelli locali rimasti inattivi più a lungo di quanto scelto
    /// nelle preferenze (LO-8), ognuno con il suo tempo. Restituisce quanti
    /// ne ha scaricati.
    pub async fn unload_if_idle(&self) -> usize {
        let minutes = self.preferences().idle_unload_minutes;
        if minutes == 0 || self.is_busy() {
            return 0;
        }
        let limit = std::time::Duration::from_secs(u64::from(minutes) * 60);
        let mut unloaded = 0;
        for engine in self.engine_list() {
            if engine.idle() < limit {
                continue;
            }
            // Un caricamento in corso tiene occupato il motore: non si tocca.
            let Ok(mut runtime) = engine.runtime.try_lock() else {
                continue;
            };
            if runtime.loaded().is_some() {
                runtime.unload().await;
                unloaded += 1;
            }
            drop(runtime);
            self.remove_engine_slot(&engine.key);
        }
        unloaded
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
            self.check_schedules();
        }
    }
}

/// Il nome del file di un modello, senza cartella né `.gguf`.
pub fn model_file_name(path: &str) -> String {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    name.strip_suffix(".gguf").unwrap_or(name).to_string()
}

/// Toglie dalla memoria libera il margine che resta al sistema quando si
/// carica un modello accanto ad altri (ADR 0029).
fn reduce_by_reserve(memory: &mut MemorySnapshot) {
    if let Some(gpu) = memory.gpu.as_mut() {
        gpu.free_bytes = gpu
            .free_bytes
            .saturating_sub(local::pool::vram_reserve(gpu.total_bytes));
    }
    memory.ram_available_bytes = memory
        .ram_available_bytes
        .saturating_sub(local::pool::ram_reserve(memory.ram_total_bytes));
}
