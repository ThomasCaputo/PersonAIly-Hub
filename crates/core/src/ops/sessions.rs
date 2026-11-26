//! Agenti e loro sessioni: avvio, pausa, deleghe e punti di ripristino.

use crate::ops::conversation::chat_history;
use crate::*;

/// Secondi dal 1970: rende unici i nomi delle foto di sicurezza.
pub(crate) fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Core {
    pub fn agents(&self) -> Result<Vec<Agent>> {
        self.store().agents()
    }

    /// Crea (con `id` 0) o aggiorna un agente.
    pub fn save_agent(&self, agent: Agent) -> Result<Agent> {
        let mut agent = agent.normalized();
        if agent.name.is_empty() {
            return Err(Error::msg("Dai un nome all'agente."));
        }
        if let Some(models) = &agent.fallback_models {
            agent.fallback_models = Some(normalize_fallbacks(models)?);
        }
        self.store().save_agent(&agent)
    }

    pub fn duplicate_agent(&self, id: i64) -> Result<Agent> {
        let store = self.store();
        let original = store.agent(id)?;
        store.save_agent(&Agent {
            id: 0,
            name: format!("{} (copia)", original.name),
            ..original
        })
    }

    pub fn delete_agent(&self, id: i64) -> Result<()> {
        self.store().delete_agent(id)
    }

    pub(crate) fn live_sessions(&self) -> Vec<Arc<LiveSession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    pub(crate) fn live_session(&self, id: i64) -> Option<Arc<LiveSession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    pub(crate) fn with_live_status(&self, mut session: AgentSession) -> AgentSession {
        if let Some(status) = self
            .live_session(session.id)
            .and_then(|l| l.snapshot().status)
        {
            session.status = status;
        }
        session
    }

    /// Prepara una sessione: l'agente lavorerà sul compito appena chi la
    /// chiama avvia [`Core::run_session`] (di solito in sottofondo).
    pub fn create_session(&self, agent_id: i64, task: &str) -> Result<AgentSession> {
        let task = task.trim();
        if task.is_empty() {
            return Err(Error::msg("Scrivi il compito da affidare all'agente."));
        }
        let store = self.store();
        let agent = store.agent(agent_id)?;
        store.create_session(&agent, task)
    }

    /// Aggiunge un messaggio dell'utente a una sessione ferma, perché l'agente
    /// riprenda da lì con [`Core::run_session`].
    pub fn reply_session(&self, id: i64, text: &str) -> Result<AgentSession> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::msg("Scrivi qualcosa all'agente."));
        }
        if self.live_session(id).is_some() {
            return Err(Error::msg("L'agente sta già lavorando."));
        }
        let store = self.store();
        let session = store.session(id)?;
        store.add_message(session.conversation_id, "user", text, None)?;
        store.set_session_status(id, SessionStatus::Running, None)?;
        store.session(id)
    }

    /// Le sessioni, dalla più recente, con lo stato attuale di quelle al lavoro.
    pub fn sessions(&self) -> Result<Vec<AgentSession>> {
        let sessions = self.store().sessions()?;
        Ok(sessions
            .into_iter()
            .map(|s| self.with_live_status(s))
            .collect())
    }

    pub fn session(&self, id: i64) -> Result<AgentSession> {
        let session = self.store().session(id)?;
        Ok(self.with_live_status(session))
    }

    /// Ciò che una sessione al lavoro sta facendo; `None` se è ferma.
    pub fn session_live(&self, id: i64) -> Option<LiveState> {
        self.live_session(id).map(|live| live.snapshot())
    }

    /// Mette in pausa una sessione: si ferma prima del passo successivo.
    pub fn pause_session(&self, id: i64) -> Result<()> {
        let live = self
            .live_session(id)
            .ok_or_else(|| Error::msg("La sessione non sta lavorando."))?;
        live.control.pause();
        Ok(())
    }

    pub fn resume_session(&self, id: i64) -> Result<()> {
        let live = self
            .live_session(id)
            .ok_or_else(|| Error::msg("La sessione non sta lavorando."))?;
        live.control.resume();
        Ok(())
    }

    /// Ferma una sessione: la risposta in corso si interrompe e il comando in
    /// esecuzione viene terminato. Ciò che è già arrivato resta salvato.
    pub fn stop_session(&self, id: i64) {
        if let Some(live) = self.live_session(id) {
            live.control.stop();
        }
    }

    pub fn stop_all_sessions(&self) {
        for live in self.live_sessions() {
            live.control.stop();
        }
    }

    /// Gli agenti delle sessioni che hanno affidato lavoro a questa, dalla più
    /// vicina: la catena delle deleghe (AG-8).
    pub(crate) fn session_ancestors(&self, id: i64) -> Result<Vec<i64>> {
        let store = self.store();
        let mut agents = Vec::new();
        let mut current = store.session(id)?.parent_session_id;
        // Il limite protegge da catene rovinate nell'archivio.
        while let Some(parent) = current.filter(|_| agents.len() < 16) {
            let session = store.session(parent)?;
            agents.extend(session.agent_id);
            current = session.parent_session_id;
        }
        Ok(agents)
    }

    /// Esegue `delegate_task`: avvia una sessione dell'altro agente, la segue
    /// fino alla fine e ne restituisce il resoconto. Fermare questa sessione
    /// ferma anche quella. `None` se l'utente ha interrotto.
    ///
    /// Il futuro è dichiarato `Send` in una Box: la ricorsione (un aiutante può
    /// avere aiutanti) altrimenti impedirebbe al compilatore di verificarlo.
    pub(crate) fn delegate<'a>(
        &'a self,
        run: &'a RunScope<'a>,
        delegation: &'a Delegation,
        call: &'a ToolCall,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ToolOutput>> + Send + 'a>> {
        Box::pin(async move {
            let get = |key: &str| call.arguments.get(key).and_then(serde_json::Value::as_str);
            let (Some(name), Some(task)) = (get("agent"), get("task")) else {
                return Some(ToolOutput::error(
                    "Servono i parametri \"agent\" e \"task\".",
                ));
            };
            let Some(agent) = delegation
                .agents
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(name.trim()))
            else {
                let names: Vec<&str> = delegation.agents.iter().map(|a| a.name.as_str()).collect();
                return Some(ToolOutput::error(format!(
                    "Agente sconosciuto: \"{name}\". Puoi scegliere tra: {}.",
                    names.join(", ")
                )));
            };
            let child = match self.create_session(agent.id, task) {
                Ok(child) => child,
                Err(e) => return Some(ToolOutput::error(e.to_string())),
            };
            if let Some(parent) = run.session_id {
                let _ = self.store().set_session_parent(child.id, parent);
            }
            let result = tokio::select! {
                result = self.run_session(child.id) => result,
                _ = run.control.cancel.notified() => {
                    self.stop_session(child.id);
                    return None;
                }
            };
            let session = self.store().session(child.id).ok();
            let reply = self
                .store()
                .messages(child.conversation_id)
                .ok()
                .and_then(|m| {
                    m.into_iter()
                        .rev()
                        .find(|m| m.role == "assistant" && !m.content.trim().is_empty())
                })
                .map(|m| m.content)
                .unwrap_or_default();
            let status = session
                .as_ref()
                .map(|s| s.status.as_str())
                .unwrap_or("failed");
            let note = session.and_then(|s| s.note).unwrap_or_default();
            let header = format!("Sessione {} di {}, stato: {status}.", child.id, agent.name);
            Some(match result {
                Ok(()) if status == "done" => ToolOutput::ok(format!(
                    "{header}

Resoconto:
{reply}"
                )),
                Ok(()) => ToolOutput::error(format!(
                    "{header} {note}

Ultima risposta:
{reply}"
                )),
                Err(e) => ToolOutput::error(format!("{header} {e}")),
            })
        })
    }

    /// `git` c'è: senza, gli agenti lavorano senza punti di ripristino.
    pub fn restore_points_available(&self) -> bool {
        snapshots::git_available()
    }

    /// Annulla ciò che l'agente ha cambiato nelle sue cartelle: le riporta
    /// com'erano prima della sessione. Prima scatta un'altra foto, che
    /// diventa il nuovo punto di ripristino della sessione: ripristinare di
    /// nuovo rimette le cose come prima dell'annullamento.
    pub fn restore_session(&self, id: i64) -> Result<AgentSession> {
        if self.live_session(id).is_some() {
            return Err(Error::msg(
                "Ferma l'agente prima di annullare le sue modifiche.",
            ));
        }
        let session = self.store().session(id)?;
        if session.restore_points.is_empty() {
            return Err(Error::msg("Questa sessione non ha un punto di ripristino."));
        }
        let mut safety = Vec::new();
        let mut errors = Vec::new();
        for (n, point) in session.restore_points.iter().enumerate() {
            let reference = format!("refs/sessions/{id}-ripristino-{}-{n}", unix_seconds());
            match snapshots::restore(&self.data_dir, point, &reference) {
                Ok(saved) => safety.push(saved),
                Err(e) => errors.push(format!("{}: {e}", point.folder)),
            }
        }
        let store = self.store();
        let folders: Vec<&str> = safety.iter().map(|p| p.folder.as_str()).collect();
        let _ = store.add_action(&NewAction {
            conversation_id: session.conversation_id,
            session_id: Some(id),
            source: &session.agent_name,
            tool: "restore_point",
            summary: &format!("Ripristinare le cartelle: {}", folders.join(", ")),
            approval: "user",
            is_error: !errors.is_empty(),
            result: &if errors.is_empty() {
                "Cartelle riportate a prima della sessione.".to_string()
            } else {
                errors.join("\n")
            },
        });
        if !safety.is_empty() {
            store.set_session_restore_points(id, &safety)?;
        }
        if !errors.is_empty() {
            return Err(Error::msg(format!(
                "Ripristino non riuscito per:\n{}",
                errors.join("\n")
            )));
        }
        store.session(id)
    }

    pub fn delete_session(&self, id: i64) -> Result<()> {
        if self.live_session(id).is_some() {
            return Err(Error::msg("Ferma l'agente prima di eliminare la sessione."));
        }
        let session = self.store().session(id)?;
        self.store().delete_session(id)?;
        // Le sue foto non servono più: si lasciano cancellare a git.
        for point in &session.restore_points {
            let exact = format!("refs/sessions/{id}");
            let derived = format!("refs/sessions/{id}-*");
            let _ = snapshots::forget(
                &self.data_dir,
                Path::new(&point.folder),
                &[&exact, &derived],
            );
        }
        Ok(())
    }

    /// Fa lavorare l'agente della sessione finché non ha finito, non viene
    /// fermato o non arriva al limite di passi. Più sessioni possono girare
    /// insieme (AG-6); la stessa sessione una volta sola.
    pub async fn run_session(&self, id: i64) -> Result<()> {
        let live = Arc::new(LiveSession::new());
        {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            if sessions.contains_key(&id) {
                return Err(Error::msg("L'agente sta già lavorando."));
            }
            sessions.insert(id, live.clone());
        }
        let _ = self
            .store()
            .set_session_status(id, SessionStatus::Running, None);

        let result = self.drive_session(id, &live).await;
        let (status, note) = match &result {
            Ok(outcome) if outcome.cancelled => (SessionStatus::Stopped, None),
            Ok(outcome) => match &outcome.notice {
                Some(notice) => (SessionStatus::Stopped, Some(notice.clone())),
                None => (SessionStatus::Done, None),
            },
            // Fermata mentre partivano i server MCP: non è un guasto.
            Err(_) if live.control.is_stopped() => (SessionStatus::Stopped, None),
            Err(error) => {
                log::error(format!("sessione {id} non riuscita: {error}"));
                (SessionStatus::Failed, Some(error.to_string()))
            }
        };
        let saved = self.store().set_session_status(id, status, note.as_deref());

        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        self.forget_approvals(&format!("s{id}"));
        saved?;
        result.map(|_| ())
    }

    pub(crate) async fn drive_session(
        &self,
        id: i64,
        live: &Arc<LiveSession>,
    ) -> Result<SendOutcome> {
        let session = self.store().session(id)?;
        let agent = session.definition.ok_or_else(|| {
            Error::msg("La definizione dell'agente di questa sessione è illeggibile.")
        })?;
        let target = self.target(&agent.model)?;
        let sampling = self.preset(&target.preset_key())?.sampling;
        let reads_images = target.reads_images();

        // Punto di ripristino: la prima volta che la sessione lavora, prima di
        // qualunque strumento. Riprendendo la sessione resta quello iniziale.
        if agent.restore_points && session.restore_points.is_empty() && !agent.folders.is_empty() {
            let data_dir = self.data_dir.clone();
            let folders = agent.folders.clone();
            let points = tokio::task::spawn_blocking(move || {
                if !snapshots::git_available() {
                    return Vec::new();
                }
                // Una cartella che non si riesce a fotografare non ferma l'agente:
                // per lei semplicemente non ci sarà un punto di ripristino.
                folders
                    .iter()
                    .filter_map(|f| {
                        snapshots::create(
                            &data_dir,
                            Path::new(f),
                            &format!("Prima della sessione {id}"),
                            &format!("refs/sessions/{id}"),
                        )
                        .ok()
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            if !points.is_empty() {
                self.store().set_session_restore_points(id, &points)?;
            }
        }

        let mut settings = self.conversation_tools(session.conversation_id)?;
        let folders: Vec<PathBuf> = agent.folders.iter().map(PathBuf::from).collect();
        let mut defs = tools::available(&settings, &folders);
        if settings.documents && self.rag_ready() {
            defs.push(rag::tool_def());
        }
        let (skill_defs, skill_prompt) = self.skill_offer(&settings);
        defs.extend(skill_defs);

        // I server MCP dell'agente partono con la sessione e si chiudono con lei.
        let mut servers = self.mcp_servers_for(&agent.mcp_servers)?;
        self.authorize_mcp(&mut servers).await?;
        let mcp_set = if servers.is_empty() {
            None
        } else {
            let start = mcp::McpSet::start(&servers, folders.first().map(PathBuf::as_path));
            let set = tokio::select! {
                set = start => set?,
                _ = live.control.cancel.notified() => {
                    return Err(Error::msg("Fermato durante l'avvio dei server MCP."));
                }
            };
            defs.extend(set.defs().iter().cloned());
            Some(set)
        };

        // Deleghe: agli agenti scelti, entro la profondità massima e mai a chi
        // ha già lavoro in questa catena (niente giri a vuoto).
        let delegation = if agent.delegates.is_empty() {
            None
        } else {
            let ancestors = self.session_ancestors(id)?;
            let depth = ancestors.len() as u32;
            let store = self.store();
            let agents: Vec<Agent> = agent
                .delegates
                .iter()
                .filter_map(|&d| store.agent(d).ok())
                .filter(|a| !ancestors.contains(&a.id) && Some(a.id) != session.agent_id)
                .collect();
            (depth < agents::MAX_DELEGATION_DEPTH && !agents.is_empty())
                .then_some(Delegation { agents })
        };
        if let Some(delegation) = &delegation {
            defs.push(agents::delegate_tool(&delegation.agents));
        }

        let system = join_prompt(&[
            agents::AGENT_PREAMBLE,
            &agent.instructions,
            &self.language_instruction(),
            &tools::system_preamble(&defs, &folders),
            &skill_prompt,
        ]);
        let chat = chat_history(
            self.store().messages(session.conversation_id)?,
            reads_images,
        );

        let scope = RunScope {
            conversation_id: session.conversation_id,
            session_id: Some(id),
            source: agent.name.clone(),
            key_prefix: format!("s{id}"),
            control: &live.control,
            max_rounds: agent.max_steps as usize,
            limit_notice: format!(
                "L'agente ha fatto {} passi, il massimo previsto, e si è fermato. \
                 Rispondigli per farlo continuare.",
                agent.max_steps
            ),
            mcp: mcp_set.as_ref(),
            delegation: delegation.as_ref(),
            step_tokens: Some(agent.max_step_tokens),
            fallback: Fallback::from_setting(&agent.fallback_models),
        };
        let mut on_event = |event| {
            let mut state = live.state();
            match event {
                ChatEvent::Text(text) => state.streaming.push_str(&text),
                ChatEvent::Reasoning(text) => state.reasoning.push_str(&text),
                ChatEvent::Saved => {
                    state.streaming.clear();
                    state.reasoning.clear();
                    state.revision += 1;
                }
                ChatEvent::ToolRequest {
                    id,
                    name,
                    summary,
                    arguments,
                } => {
                    state.status = Some(SessionStatus::Waiting);
                    state.request = Some(PendingRequest {
                        id,
                        name,
                        summary,
                        arguments,
                    });
                }
                // Il cambio resta visibile nel nome del modello su ogni messaggio
                // dell'agente e nel registro diagnostico.
                ChatEvent::ModelSwitch { .. } => {}
            }
        };
        self.run_rounds(
            &scope,
            &target,
            chat,
            &defs,
            &mut settings,
            &folders,
            &system,
            &sampling,
            &mut on_event,
        )
        .await
    }
}
