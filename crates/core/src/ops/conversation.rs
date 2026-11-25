//! Conversazioni: invio dei messaggi, rami, esportazione.

use crate::*;

/// Che cosa sostituisce un messaggio mandato di nuovo.
#[derive(Clone, Copy)]
pub(crate) enum Replace {
    /// Niente: è un messaggio nuovo.
    None,
    /// Una versione nuova (modificata) di questo messaggio.
    Message(i64),
    /// Lo stesso messaggio: si rifà solo la risposta che lo segue.
    Answer(i64),
}

/// Gli allegati di un messaggio: da leggere dal disco, o già letti (quando
/// si rimanda un messaggio salvato).
pub(crate) enum Attachments {
    Paths(Vec<String>),
    Loaded(Vec<Attachment>),
}

/// Segna una conversazione come "sta rispondendo" finché esiste.
pub(crate) struct Running<'a> {
    pub(crate) core: &'a Core,
    pub(crate) conversation_id: i64,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.core
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.conversation_id);
    }
}

/// La cronologia salvata, nella forma da inviare a un modello.
///
/// Ogni chiamata a uno strumento deve avere il suo risultato, altrimenti i
/// servizi rifiutano la conversazione: a quelle rimaste senza (app chiusa a
/// metà) se ne aggiunge uno di ripiego.
pub(crate) fn chat_history(history: Vec<Message>, with_images: bool) -> Vec<ChatMessage> {
    fn close(open: &mut Vec<ToolCall>, out: &mut Vec<ChatMessage>) {
        for call in open.drain(..) {
            out.push(ChatMessage {
                tool_call_id: Some(call.id),
                is_error: true,
                ..ChatMessage::new("tool", "Interrotto prima del risultato.")
            });
        }
    }

    let mut out = Vec::with_capacity(history.len());
    let mut open: Vec<ToolCall> = Vec::new();
    for m in history {
        if m.role == "tool" {
            // Un risultato senza la sua chiamata non si può inviare.
            let position = m
                .tool_call_id
                .as_ref()
                .and_then(|id| open.iter().position(|call| call.id == *id));
            if let Some(position) = position {
                open.remove(position);
                out.push(ChatMessage {
                    tool_call_id: m.tool_call_id,
                    is_error: m.is_error,
                    ..ChatMessage::new("tool", m.content)
                });
            }
            continue;
        }
        close(&mut open, &mut out);

        if m.role == "assistant" {
            if m.content.is_empty() && m.tool_calls.is_empty() {
                continue;
            }
            open = m.tool_calls.clone();
            out.push(ChatMessage {
                tool_calls: m.tool_calls,
                ..ChatMessage::new("assistant", m.content)
            });
        } else {
            let mut content = attachments::with_text_attachments(&m.content, &m.attachment_data);
            let mut images = Vec::new();
            for a in m.attachment_data {
                if a.kind != AttachmentKind::Image {
                    continue;
                }
                if with_images {
                    images.push(chat::Image {
                        media_type: a.media_type.unwrap_or_else(|| "image/png".to_string()),
                        data: a.data,
                    });
                } else {
                    content.push_str(&format!(
                        "\n[Immagine allegata che questo modello non può vedere: {}]",
                        a.name
                    ));
                }
            }
            out.push(ChatMessage {
                images,
                ..ChatMessage::new(&m.role, content)
            });
        }
    }
    close(&mut open, &mut out);
    out
}

/// Titolo di una conversazione ricavato dal primo messaggio.
pub(crate) fn title_from(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim();
    if line.is_empty() {
        return DEFAULT_TITLE.to_string();
    }
    if line.chars().count() <= TITLE_MAX_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(TITLE_MAX_CHARS).collect();
    format!("{}…", cut.trim_end())
}

impl Core {
    pub fn create_conversation(&self) -> Result<Conversation> {
        self.store().create_conversation(DEFAULT_TITLE)
    }

    /// Una conversazione temporanea, per una colonna del confronto tra
    /// modelli: funziona come le altre ma non compare nella cronologia finché
    /// non la si salva con [`Core::keep_conversation`]. All'avvio si cancella.
    pub fn create_temporary_conversation(&self) -> Result<Conversation> {
        self.store().create_temporary_conversation(DEFAULT_TITLE)
    }

    /// Salva una conversazione temporanea: da qui è una conversazione normale.
    pub fn keep_conversation(&self, id: i64) -> Result<()> {
        self.store().keep_conversation(id)
    }

    /// Conversazioni dalla più recente; con `query` solo quelle che contengono
    /// il testo cercato nel titolo o in un messaggio.
    pub fn list_conversations(&self, query: &str) -> Result<Vec<Conversation>> {
        self.store().list_conversations(query)
    }

    pub fn rename_conversation(&self, id: i64, title: &str) -> Result<()> {
        self.store().rename_conversation(id, title)
    }

    pub fn delete_conversation(&self, id: i64) -> Result<()> {
        self.close_chat_mcp(id);
        self.store().delete_conversation(id)
    }

    pub fn messages(&self, conversation_id: i64) -> Result<Vec<Message>> {
        self.store().messages(conversation_id)
    }

    /// I messaggi da mostrare, senza il contenuto degli allegati.
    pub fn messages_for_display(&self, conversation_id: i64) -> Result<Vec<Message>> {
        self.store().messages_for_display(conversation_id)
    }

    /// Il preset salvato per un modello (per quello locale, il modello caricato).
    pub fn preset_for(&self, model: &ModelRef) -> Result<Preset> {
        self.preset(&self.target(model)?.preset_key())
    }

    /// Una risposta "senza archivio", per l'endpoint compatibile con OpenAI:
    /// la cronologia arriva intera dal client, nulla si salva e gli strumenti
    /// indicati dal client li esegue il client stesso. I messaggi `system`
    /// diventano il prompt di sistema; i parametri assenti si prendono dal
    /// preset del modello.
    pub async fn complete(
        &self,
        model: &ModelRef,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        overrides: &SamplingOverrides,
        cancel: Arc<Notify>,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<chat::ChatOutcome> {
        let target = self.target(model)?;
        let mut sampling = self.preset(&target.preset_key())?.sampling;
        overrides.apply(&mut sampling);

        let system: Vec<&str> = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .collect();
        let system = join_prompt(&system);
        let rest: Vec<ChatMessage> = messages
            .iter()
            .filter(|m| m.role != "system")
            .cloned()
            .collect();
        if rest.is_empty() {
            return Err(Error::msg("La richiesta non contiene messaggi."));
        }

        match &target {
            Target::Local(loaded) => {
                let mut all = Vec::with_capacity(rest.len() + 1);
                if !system.is_empty() {
                    all.push(ChatMessage::new("system", system));
                }
                all.extend(rest);
                let all = chat::fit_template(all, loaded.template);
                let tools = if loaded.template.no_tools { &[] } else { tools };
                let url = format!("http://127.0.0.1:{}/v1/chat/completions", loaded.port);
                let body = chat::request_body(&all, tools, &sampling, None, chat::Flavor::Llama);
                let result = chat::stream_chat(
                    &self.http,
                    &url,
                    Some(&loaded.api_key),
                    &body,
                    cancel,
                    on_event,
                )
                .await;
                self.touch_local();
                result
            }
            Target::Cloud(provider, key, id) => {
                providers::stream(
                    &self.cloud_http,
                    provider,
                    key,
                    id,
                    &rest,
                    tools,
                    &system,
                    &sampling,
                    cancel,
                    on_event,
                )
                .await
            }
        }
    }

    /// A chi mandare le richieste per un modello; errore se non è raggiungibile.
    pub(crate) fn target(&self, model: &ModelRef) -> Result<Target> {
        Ok(match model {
            ModelRef::Local => {
                let loaded = self
                    .runtime
                    .try_lock()
                    .ok()
                    .and_then(|mut runtime| runtime.loaded())
                    .ok_or_else(|| Error::msg("Nessun modello locale caricato."))?;
                self.touch_local();
                Target::Local(loaded)
            }
            ModelRef::Cloud {
                provider_id,
                model: id,
            } => {
                let provider = self.provider(provider_id)?;
                let key = secrets::api_key(provider_id)?.ok_or_else(|| {
                    Error::msg(format!(
                        "Manca la chiave API di {}: inseriscila nella pagina Modelli.",
                        provider.name
                    ))
                })?;
                Target::Cloud(provider, key, id.clone())
            }
        })
    }

    /// Salva il messaggio dell'utente, lo invia al modello insieme alla
    /// cronologia e salva la risposta. Se il modello chiede degli strumenti li
    /// esegue (con conferma, dove serve) e gli rimanda i risultati, finché non
    /// arriva la risposta finale. `on_event` segue tutto mentre accade.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_message(
        &self,
        conversation_id: i64,
        text: &str,
        attachment_paths: &[String],
        sampling: &SamplingParams,
        system_prompt: &str,
        model: &ModelRef,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<SendOutcome> {
        let paths = attachment_paths.to_vec();
        self.send_with(
            conversation_id,
            text,
            Attachments::Paths(paths),
            Replace::None,
            sampling,
            system_prompt,
            model,
            on_event,
        )
        .await
    }

    /// Modifica un messaggio dell'utente e rigenera la risposta (CH-8). Il
    /// messaggio nuovo diventa un'altra versione di quello vecchio, che resta
    /// con le sue risposte in un ramo a parte. Con `text` assente (o uguale)
    /// il messaggio resta quello: si rigenera solo la risposta che lo segue, e
    /// la vecchia diventa a sua volta un ramo.
    #[allow(clippy::too_many_arguments)]
    pub async fn resend_message(
        &self,
        conversation_id: i64,
        message_id: i64,
        text: Option<&str>,
        sampling: &SamplingParams,
        system_prompt: &str,
        model: &ModelRef,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<SendOutcome> {
        let original = self.store().message_in(conversation_id, message_id)?;
        if original.role != "user" {
            return Err(Error::msg("Si può modificare solo un tuo messaggio."));
        }
        let text = text.map(str::trim).unwrap_or(&original.content).to_string();
        if text.is_empty() && original.attachment_data.is_empty() {
            return Err(Error::msg("Il messaggio è vuoto."));
        }
        let replace = if text == original.content {
            Replace::Answer(message_id)
        } else {
            Replace::Message(message_id)
        };
        self.send_with(
            conversation_id,
            &text,
            Attachments::Loaded(original.attachment_data),
            replace,
            sampling,
            system_prompt,
            model,
            on_event,
        )
        .await
    }

    /// Il lavoro comune di [`Core::send_message`] e [`Core::resend_message`].
    /// `replace` dice se il messaggio sostituisce, in un altro ramo, uno già mandato.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn send_with(
        &self,
        conversation_id: i64,
        text: &str,
        attachments: Attachments,
        replace: Replace,
        sampling: &SamplingParams,
        system_prompt: &str,
        model: &ModelRef,
        mut on_event: impl FnMut(ChatEvent),
    ) -> Result<SendOutcome> {
        // Prima si verifica di poter raggiungere il modello, poi si salva il messaggio.
        let target = self.target(model)?;
        // Anche il limite di spesa, prima di salvare: niente messaggi senza risposta.
        if matches!(target, Target::Cloud(..)) {
            self.check_spend(conversation_id)?;
        }

        let control = Arc::new(RunControl::new());
        {
            let mut running = self.generation.lock().unwrap_or_else(|e| e.into_inner());
            if running.contains_key(&conversation_id) {
                return Err(Error::msg(
                    "Questa conversazione sta già rispondendo: aspetta o interrompi la risposta.",
                ));
            }
            running.insert(conversation_id, control.clone());
        }
        // Libera la conversazione a qualunque uscita, errori compresi.
        let _running = Running {
            core: self,
            conversation_id,
        };

        let attachments = match attachments {
            Attachments::Loaded(loaded) => loaded,
            // Fino a 20 MB per file, e i PDF vanno interpretati: fuori dal ciclo asincrono.
            Attachments::Paths(paths) => tokio::task::spawn_blocking(move || {
                paths
                    .iter()
                    .map(|path| attachments::load(Path::new(path)))
                    .collect::<Result<Vec<Attachment>>>()
            })
            .await
            .map_err(|e| Error::msg(e.to_string()))??,
        };
        let reads_images = target.reads_images();
        if !reads_images && attachments.iter().any(|a| a.kind == AttachmentKind::Image) {
            return Err(Error::msg(
                "Il modello locale caricato non legge le immagini: caricalo con il suo \
                 componente visivo (mmproj), scegli un modello cloud che le supporti, \
                 oppure togli l'immagine.",
            ));
        }

        let mut settings = self.conversation_tools(conversation_id)?;
        // Il terminale è degli agenti: in chat non si offre.
        settings.terminal = false;
        let folders: Vec<PathBuf> = self
            .granted_folders()?
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let mut defs = tools::available(&settings, &folders);
        if settings.documents && self.rag_ready() {
            defs.push(rag::tool_def());
        }
        let (skill_defs, skill_prompt) = self.skill_offer(&settings);
        defs.extend(skill_defs);
        let (chat, title) = {
            let store = self.store();
            match replace {
                Replace::None => {}
                Replace::Message(id) => {
                    store.branch_from(conversation_id, id)?;
                }
                // Il messaggio resta: da parte va solo ciò che lo segue.
                Replace::Answer(id) => {
                    store.branch_from(conversation_id, id + 1)?;
                }
            }
            // Modificando il primo messaggio il titolo resta: forse l'ha scelto l'utente.
            let is_first =
                matches!(replace, Replace::None) && store.has_no_messages(conversation_id)?;
            if !matches!(replace, Replace::Answer(_)) {
                store.add_message_with(conversation_id, "user", text, None, None, &attachments)?;
            }
            if is_first {
                store.rename_conversation(conversation_id, &title_from(text))?;
            }
            (
                chat_history(store.messages(conversation_id)?, reads_images),
                store.conversation(conversation_id)?.title,
            )
        };

        // I server MCP della conversazione (ADR 0022): partono dopo il messaggio,
        // così intanto si vede l'attesa, e «Interrompi» ferma anche l'avvio.
        let mcp_set = if settings.mcp_servers.is_empty() {
            self.close_chat_mcp(conversation_id);
            None
        } else {
            let start = self.chat_mcp_set(
                conversation_id,
                &settings.mcp_servers,
                folders.first().map(PathBuf::as_path),
            );
            tokio::select! {
                set = start => set?,
                _ = control.cancel.notified() => {
                    return Ok(SendOutcome {
                        conversation: self.store().conversation(conversation_id)?,
                        assistant_message: None,
                        stats: GenerationStats::default(),
                        cancelled: true,
                        notice: None,
                    });
                }
            }
        };
        if let Some(set) = &mcp_set {
            defs.extend(set.defs().iter().cloned());
        }
        let system = join_prompt(&[
            system_prompt,
            &self.language_instruction(),
            &tools::system_preamble(&defs, &folders),
            &skill_prompt,
        ]);

        let scope = RunScope {
            conversation_id,
            session_id: None,
            source: title,
            key_prefix: format!("c{conversation_id}"),
            control: &control,
            max_rounds: MAX_TOOL_ROUNDS,
            limit_notice: NOTICE_TOO_MANY_TOOLS.to_string(),
            mcp: mcp_set.as_deref(),
            delegation: None,
            step_tokens: None,
        };
        let result = self
            .run_rounds(
                &scope,
                &target,
                chat,
                &defs,
                &mut settings,
                &folders,
                &system,
                sampling,
                &mut on_event,
            )
            .await;

        self.forget_approvals(&scope.key_prefix);
        // Il riposo dei server si conta dalla fine della risposta, non dall'inizio.
        if mcp_set.is_some() {
            self.touch_chat_mcp(conversation_id);
        }
        result
    }

    /// Interrompe tutte le risposte in corso in chat. Il testo già arrivato viene salvato.
    pub fn stop_generation(&self) {
        for control in self
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            control.stop();
        }
    }

    /// Interrompe la risposta in corso in una conversazione, se c'è.
    pub fn stop_conversation(&self, conversation_id: i64) {
        if let Some(control) = self
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&conversation_id)
        {
            control.stop();
        }
    }

    /// `true` se una risposta in chat o una sessione di un agente è al lavoro.
    pub fn is_busy(&self) -> bool {
        !self
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
            || !self.live_sessions().is_empty()
    }

    /// La conversazione in Markdown (CH-9). `lang` è la lingua delle
    /// intestazioni (`it` o `en`).
    pub fn conversation_markdown(&self, conversation_id: i64, lang: &str) -> Result<String> {
        let (conversation, messages) = {
            let store = self.store();
            (
                store.conversation(conversation_id)?,
                store.messages_for_display(conversation_id)?,
            )
        };
        Ok(export::markdown(&conversation, &messages, lang == "en"))
    }

    /// La conversazione in JSON, con tutti i rami.
    pub fn conversation_json(&self, conversation_id: i64) -> Result<String> {
        let store = self.store();
        let conversation = store.conversation(conversation_id)?;
        let tree = store.message_tree(conversation_id)?;
        export::json(&conversation, &tree)
    }

    /// Mostra il ramo della conversazione che passa per `message_id`.
    pub fn switch_branch(&self, conversation_id: i64, message_id: i64) -> Result<()> {
        let generating = self
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&conversation_id);
        if generating {
            return Err(Error::msg(
                "Questa conversazione sta già rispondendo: aspetta o interrompi la risposta.",
            ));
        }
        self.store().switch_branch(conversation_id, message_id)
    }

    /// Salva la conversazione in Markdown. `path` arriva dalla finestra
    /// "Salva con nome" del sistema.
    pub fn export_conversation(&self, conversation_id: i64, path: &Path, lang: &str) -> Result<()> {
        // Il formato lo sceglie l'estensione del file: `.json` con tutti i rami,
        // altrimenti Markdown con il ramo che si vede.
        let is_json = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        let text = if is_json {
            self.conversation_json(conversation_id)?
        } else {
            self.conversation_markdown(conversation_id, lang)?
        };
        std::fs::write(path, text)?;
        Ok(())
    }
}
