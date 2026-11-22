//! Documenti consultabili (RAG locale, ADR 0017).

use crate::*;

/// Dopo tanti file di fila rifiutati dal modello di embedding, l'errore non
/// è dei file ma del modello: l'indicizzazione si ferma.
const MAX_EMBED_FAILURES_IN_A_ROW: u32 = 5;

impl Core {
    pub fn rag_settings(&self) -> rag::RagSettings {
        self.store()
            .setting(SETTING_RAG)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Cambia modello di embedding o cartelle. Le cartelle devono essere tra
    /// quelle concesse; cambiando modello l'indice si svuota (i vettori di
    /// modelli diversi non si confrontano) e va rifatto.
    pub fn set_rag_settings(&self, settings: &rag::RagSettings) -> Result<()> {
        let granted = self.granted_folders()?;
        if let Some(folder) = settings.folders.iter().find(|f| !granted.contains(f)) {
            return Err(Error::msg(format!(
                "La cartella {folder} non è tra quelle concesse ai modelli: concedila prima nelle Impostazioni."
            )));
        }
        if let Some(model) = &settings.embedding_model
            && !self.list_models()?.iter().any(|m| &m.path == model)
        {
            return Err(Error::msg("Il modello di embedding non è nella libreria."));
        }
        let previous = self.rag_settings();
        let store = self.store();
        if previous.embedding_model != settings.embedding_model {
            store.rag_clear()?;
        }
        store.set_setting(SETTING_RAG, &serde_json::to_string(settings)?)?;
        // Una cartella tolta dai documenti esce subito dall'indice.
        store.rag_keep_folders(&settings.folders)?;
        Ok(())
    }

    /// Le cartelle in cui si può cercare adesso: scelte per i documenti e
    /// ancora concesse ai modelli.
    pub(crate) fn rag_allowed_folders(&self) -> Result<Vec<String>> {
        let granted = self.granted_folders()?;
        Ok(self
            .rag_settings()
            .folders
            .into_iter()
            .filter(|f| granted.contains(f))
            .collect())
    }

    pub fn rag_status(&self) -> Result<rag::RagStatus> {
        let (files, chunks) = self.store().rag_counts()?;
        let mut status = self
            .rag_progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        status.settings = self.rag_settings();
        status.files = files;
        status.chunks = chunks;
        Ok(status)
    }

    /// Si può cercare: c'è un modello di embedding e qualcosa nell'indice.
    pub(crate) fn rag_ready(&self) -> bool {
        self.rag_settings().embedding_model.is_some()
            && self
                .store()
                .rag_counts()
                .is_ok_and(|(_, chunks)| chunks > 0)
    }

    /// Prefissi che alcuni modelli di embedding vogliono davanti a domande e
    /// documenti (e5: `query: ` e `passage: `; nomic: `search_query: `…).
    ///
    /// Si guarda solo il nome del file, e "e5" deve essere una parola a sé
    /// (`multilingual-e5-small`): una cartella o un nome che la contengono
    /// per caso non devono cambiare i prefissi.
    pub(crate) fn embedding_prefixes(model_path: &str) -> (&'static str, &'static str) {
        let name = Path::new(model_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let mut words = name.split(|c: char| !c.is_ascii_alphanumeric());
        if name.contains("nomic") {
            ("search_query: ", "search_document: ")
        } else if words.any(|w| w == "e5") {
            ("query: ", "passage: ")
        } else {
            ("", "")
        }
    }

    /// Il `llama-server` degli embedding, avviato se serve.
    pub(crate) async fn embedder(&self) -> Result<(LoadedModel, String)> {
        let model_path = self.rag_settings().embedding_model.ok_or_else(|| {
            Error::msg("Scegli un modello di embedding in Impostazioni → Documenti consultabili.")
        })?;
        self.touch_embedder();
        let mut runtime = self.embedder.lock().await;
        if let Some(loaded) = runtime.loaded()
            && loaded.model_path == model_path
        {
            return Ok((loaded, model_path));
        }
        let engine = self.engine()?;
        let port = local::runtime::free_port()?;
        let info = models::inspect(Path::new(&model_path));
        let context = rag::embedding_context(info.context_length);
        // Il contesto resta nei parametri: l'indicizzazione ne ricava la
        // lunghezza dei pezzi.
        let params = LoadParams {
            placement: Placement::Ram,
            context_length: context,
            ..Default::default()
        };
        let mean = self
            .mean_pooling_model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            == Some(model_path.as_str());
        let args = rag::embedding_args(&model_path, port, mean, context);
        let loaded = runtime
            .start(&engine, &model_path, &info.name, &params, port, args, false)
            .await?;
        Ok((loaded, model_path))
    }

    /// I vettori di alcuni testi. Se il modello non dichiara il "pooling"
    /// (succede con i modelli di chat) lo si riavvia con la media e si riprova.
    pub(crate) async fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let (embedder, model_path) = self.embedder().await?;
        match rag::embed(&self.http, embedder.port, &embedder.api_key, texts).await {
            Err(e) if e.to_string().contains("Pooling type 'none'") => {
                *self
                    .mean_pooling_model
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(model_path);
                self.embedder.lock().await.unload().await;
                let (embedder, _) = self.embedder().await?;
                rag::embed(&self.http, embedder.port, &embedder.api_key, texts).await
            }
            other => other,
        }
    }

    pub(crate) fn touch_embedder(&self) {
        *self
            .last_embed_use
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = std::time::Instant::now();
    }

    /// Chiude il modello di embedding se non serve da un po'.
    pub(crate) async fn stop_embedder_if_idle(&self) {
        let idle = self
            .last_embed_use
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed();
        let indexing = self
            .rag_progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .indexing;
        if idle >= EMBEDDER_IDLE
            && !indexing
            && let Ok(mut runtime) = self.embedder.try_lock()
        {
            runtime.unload().await;
        }
    }

    /// Aggiorna l'indice: rielabora solo i file nuovi o cambiati e toglie
    /// quelli spariti. È lungo: chi lo chiama di solito lo avvia in sottofondo
    /// e ne segue l'avanzamento con [`Core::rag_status`].
    pub async fn index_documents(&self) -> Result<()> {
        {
            let mut progress = self.rag_progress.lock().unwrap_or_else(|e| e.into_inner());
            if progress.indexing {
                return Err(Error::msg("L'indicizzazione è già in corso."));
            }
            *progress = rag::RagStatus {
                indexing: true,
                ..Default::default()
            };
        }
        let result = self.index_all().await;
        let mut progress = self.rag_progress.lock().unwrap_or_else(|e| e.into_inner());
        progress.indexing = false;
        if let Err(e) = &result {
            progress.error = Some(e.to_string());
        }
        result
    }

    /// I vettori dei pezzi di un file, a lotti.
    async fn embed_chunks(&self, chunks: &[String], passage: &str) -> Result<Vec<Vec<f32>>> {
        let mut vectors = Vec::with_capacity(chunks.len());
        for batch in chunks.chunks(rag::EMBED_BATCH) {
            let inputs: Vec<String> = batch.iter().map(|c| format!("{passage}{c}")).collect();
            vectors.extend(self.embed_texts(&inputs).await?);
            self.touch_embedder();
        }
        Ok(vectors)
    }

    pub(crate) async fn index_all(&self) -> Result<()> {
        let settings = self.rag_settings();
        let (embedder, model_path) = self.embedder().await?;
        let (_, passage) = Self::embedding_prefixes(&model_path);
        let chunk_chars = rag::chunk_chars(embedder.params.context_length);
        let mut failures_in_a_row = 0;
        let granted = self.granted_folders()?;
        let folders: Vec<String> = settings
            .folders
            .iter()
            .filter(|f| granted.contains(f))
            .cloned()
            .collect();
        let scans: Vec<(String, Vec<rag::FileStamp>)> = {
            let folders = folders.clone();
            tokio::task::spawn_blocking(move || {
                folders
                    .into_iter()
                    .map(|f| {
                        let files = rag::scan(Path::new(&f));
                        (f, files)
                    })
                    .collect()
            })
            .await
            .map_err(|e| Error::msg(e.to_string()))?
        };
        let total: usize = scans.iter().map(|(_, files)| files.len()).sum();
        self.rag_progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .total_files = total as u32;
        let mut existing = HashSet::new();
        for (folder, files) in scans {
            for file in files {
                let path = file.path.to_string_lossy().into_owned();
                existing.insert(path.clone());
                let stamp = self.store().rag_stamp(&path)?;
                if stamp != Some((file.modified, file.size)) {
                    let source = file.path.clone();
                    let text =
                        tokio::task::spawn_blocking(move || tools::files::read_text(&source))
                            .await
                            .map_err(|e| Error::msg(e.to_string()))?;
                    let embedded = match text {
                        Ok(text) => {
                            let chunks = rag::chunk_text_sized(&text, chunk_chars);
                            self.embed_chunks(&chunks, passage)
                                .await
                                .map(|vectors| chunks.into_iter().zip(vectors).collect::<Vec<_>>())
                                .map_err(Some)
                        }
                        Err(_) => Err(None),
                    };
                    match embedded {
                        Ok(pairs) => {
                            failures_in_a_row = 0;
                            self.store().rag_replace_file(
                                &folder,
                                &path,
                                file.modified,
                                file.size,
                                &pairs,
                            )?;
                        }
                        // Un file che il modello di embedding rifiuta (per
                        // esempio un pezzo troppo lungo) si salta, come quelli
                        // illeggibili. Se però falliscono molti file di fila il
                        // problema è il modello: ci si ferma.
                        Err(Some(e)) => {
                            failures_in_a_row += 1;
                            if failures_in_a_row >= MAX_EMBED_FAILURES_IN_A_ROW {
                                return Err(e);
                            }
                            self.rag_progress
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .skipped
                                .push(path.clone());
                        }
                        Err(None) => {
                            self.rag_progress
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .skipped
                                .push(path.clone());
                        }
                    }
                }
                self.rag_progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .done_files += 1;
            }
        }
        // Una cartella revocata durante l'indicizzazione non deve restare:
        // si tengono solo quelle ancora ammesse adesso.
        let allowed = self.rag_allowed_folders()?;
        let still: Vec<String> = folders
            .into_iter()
            .filter(|f| allowed.contains(f))
            .collect();
        self.store().rag_prune(&still, &existing)?;
        Ok(())
    }

    /// I brani dei documenti più vicini alla domanda.
    pub async fn search_documents(&self, query: &str) -> Result<Vec<rag::Hit>> {
        let (_, model_path) = self.embedder().await?;
        let (prefix, _) = Self::embedding_prefixes(&model_path);
        let input = [format!("{prefix}{query}")];
        let question = self.embed_texts(&input).await?.pop().unwrap_or_default();
        let allowed = self.rag_allowed_folders()?;
        let chunks = self.store().rag_chunks(&allowed)?;
        let mut hits: Vec<rag::Hit> = chunks
            .into_iter()
            .map(|(path, text, vector)| rag::Hit {
                score: rag::cosine(&question, &vector),
                path,
                text,
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(rag::RESULTS);
        Ok(hits)
    }
}
