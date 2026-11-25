//! Il ciclo modello → strumenti → modello, con conferme e registro delle azioni.

use crate::*;

/// Gli indirizzi che il modello può aprire senza conferma anche mentre vede i
/// file: quelli scritti dall'utente e quelli trovati da una ricerca.
pub(crate) fn known_urls(chat: &[ChatMessage]) -> Vec<String> {
    let searches: HashSet<&str> = chat
        .iter()
        .flat_map(|m| &m.tool_calls)
        .filter(|call| call.name == tools::SEARCH_TOOL)
        .map(|call| call.id.as_str())
        .collect();
    chat.iter()
        .filter(|m| {
            m.role == "user"
                || (m.role == "tool"
                    && m.tool_call_id
                        .as_deref()
                        .is_some_and(|id| searches.contains(id)))
        })
        .flat_map(|m| tools::urls_in(&m.content))
        .collect()
}

/// Descrizione breve di una chiamata, per la conferma e il registro.
pub(crate) fn summary_of(run: &RunScope<'_>, call: &ToolCall) -> String {
    if call.name == rag::DOCUMENTS_TOOL {
        let query = call
            .arguments
            .get("query")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?");
        return format!("Cercare nei documenti: {query}");
    }
    if call.name == skills::LOAD_SKILL || call.name == skills::READ_SKILL_FILE {
        return skills::summary(&call.name, &call.arguments);
    }
    if call.name == agents::DELEGATE_TOOL {
        let get = |key: &str| {
            call.arguments
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?")
        };
        return format!("Affidare a {}: {}", get("agent"), get("task"));
    }
    run.mcp
        .and_then(|set| set.summary(&call.name))
        .unwrap_or_else(|| tools::summary(call))
}

/// Esito di una chiamata a uno strumento.
pub(crate) struct CallResult {
    /// `None` se l'utente ha interrotto nel frattempo.
    pub(crate) output: Option<ToolOutput>,
    /// Come è stata decisa, per il registro: `auto`, `user`, `denied`, `refused`.
    pub(crate) approval: &'static str,
}

impl Core {
    /// Risposta dell'utente a una richiesta di conferma ([`ChatEvent::ToolRequest`]).
    pub fn answer_tool_request(&self, id: &str, approval: Approval) {
        let waiting = self
            .approvals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if let Some(sender) = waiting {
            // Se nessuno aspetta più (risposta interrotta) non c'è nulla da fare.
            let _ = sender.send(approval);
        }
        // Una sessione che aspettava questa conferma torna al lavoro.
        for live in self.live_sessions() {
            let mut state = live.state();
            if state.request.as_ref().is_some_and(|r| r.id == id) {
                state.request = None;
                state.status = Some(SessionStatus::Running);
            }
        }
    }

    /// Esegue una chiamata a uno strumento, chiedendo prima conferma
    /// all'utente se il livello di permesso lo richiede.
    pub(crate) async fn run_call(
        &self,
        run: &RunScope<'_>,
        call: &ToolCall,
        settings: &mut ToolSettings,
        folders: &[PathBuf],
        known_urls: &[String],
        on_event: &mut impl FnMut(ChatEvent),
    ) -> CallResult {
        // Uno strumento di un server MCP della sessione.
        let external = run.mcp.filter(|set| set.has(&call.name));
        // Il lavoro affidato a un altro agente.
        let delegation = run
            .delegation
            .filter(|_| call.name == agents::DELEGATE_TOOL);
        // La ricerca nei documenti consultabili.
        let documents = call.name == rag::DOCUMENTS_TOOL && settings.documents;
        // Le skill installate dall'utente (ADR 0023): sola lettura.
        let skill = settings.skills
            && (call.name == skills::LOAD_SKILL || call.name == skills::READ_SKILL_FILE);
        let risk = if documents || skill {
            Risk::Read
        } else if delegation.is_some() {
            Risk::Execute
        } else if external.is_some() {
            if !call.arguments.is_object() {
                return CallResult {
                    output: Some(ToolOutput::error(
                        "I parametri della chiamata non sono un oggetto JSON valido: riprova.",
                    )),
                    approval: "refused",
                };
            }
            Risk::Execute
        } else {
            // Una chiamata che fallirebbe comunque non merita di disturbare l'utente.
            if let Some(refused) = tools::precheck(call, settings, folders) {
                return CallResult {
                    output: Some(refused),
                    approval: "refused",
                };
            }
            let Some(risk) = tools::risk_of(&call.name) else {
                return CallResult {
                    output: Some(ToolOutput::error("Strumento sconosciuto.")),
                    approval: "refused",
                };
            };
            risk
        };
        let cancel = &run.control.cancel;
        // "Consenti sempre" su uno strumento MCP vale solo per quello strumento.
        let allowed_always = external.is_some() && settings.allowed_tools.contains(&call.name);
        // Un indirizzo inventato dal modello mentre vede i file dell'utente.
        let guarded = external.is_none()
            && self.exfiltration_guard()
            && tools::needs_exfiltration_check(call, settings, folders, known_urls);

        // In chat il livello parla delle cartelle concesse: non copre gli
        // strumenti MCP, che agiscono altrove (ADR 0022).
        let mcp_in_chat = external.is_some() && run.session_id.is_none();
        let mut approval = "auto";
        if guarded
            || (!allowed_always
                && (mcp_in_chat || tools::decide(settings.permission, risk) == Decision::Ask))
        {
            let key = format!("{}:{}", run.key_prefix, call.id);
            let (sender, receiver) = oneshot::channel();
            self.approvals
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key.clone(), sender);
            let mut summary = summary_of(run, call);
            if guarded {
                summary.push_str(
                    " — indirizzo scritto dal modello, non da te né da una ricerca: controlla \
                     che non contenga dati dei tuoi file.",
                );
            }
            on_event(ChatEvent::ToolRequest {
                id: key,
                name: call.name.clone(),
                summary,
                arguments: call.arguments.clone(),
            });
            let answer = tokio::select! {
                answer = receiver => answer.unwrap_or(Approval::Deny),
                _ = cancel.notified() => return CallResult { output: None, approval: "interrupted" },
            };
            match answer {
                Approval::Deny => {
                    return CallResult {
                        output: Some(ToolOutput::error(
                            "L'utente ha negato il permesso per questa azione. Non riprovare: \
                             chiedigli come vuole procedere.",
                        )),
                        approval: "denied",
                    };
                }
                Approval::Once => {}
                Approval::Always => {
                    // Vale per il resto della conversazione (o della sessione),
                    // anche dopo il riavvio.
                    if external.is_some() || guarded {
                        settings.allowed_tools.push(call.name.clone());
                    } else {
                        settings.permission = match risk {
                            Risk::Execute => PermissionLevel::All,
                            _ => settings.permission.max_with(PermissionLevel::Edits),
                        };
                    }
                    let _ = self.set_conversation_tools(run.conversation_id, settings);
                }
            }
            approval = "user";
        }

        let ctx = ToolContext {
            folders,
            web_http: &self.web_http,
        };
        if let Some(delegation) = delegation {
            let output = self.delegate(run, delegation, call).await;
            return CallResult { output, approval };
        }
        if skill {
            let output = skills::run(&self.enabled_skills(), &call.name, &call.arguments);
            return CallResult {
                output: Some(output),
                approval,
            };
        }
        if documents {
            let query = call
                .arguments
                .get("query")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let output = if query.trim().is_empty() {
                ToolOutput::error("Manca il parametro \"query\".")
            } else {
                tokio::select! {
                    hits = self.search_documents(query) => match hits {
                        Ok(hits) => ToolOutput::ok(rag::format_hits(&hits)),
                        Err(e) => ToolOutput::error(e.to_string()),
                    },
                    _ = cancel.notified() => return CallResult { output: None, approval },
                }
            };
            return CallResult {
                output: Some(output),
                approval,
            };
        }
        let output = match external {
            Some(set) => tokio::select! {
                output = set.call(&call.name, &call.arguments) => Some(output),
                _ = cancel.notified() => None,
            },
            None => tokio::select! {
                output = tools::run(call, settings, &ctx) => Some(output),
                _ = cancel.notified() => None,
            },
        };
        CallResult { output, approval }
    }

    /// Dimentica le conferme rimaste in sospeso di un ciclo concluso.
    pub(crate) fn forget_approvals(&self, key_prefix: &str) {
        let prefix = format!("{key_prefix}:");
        self.approvals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|key, _| !key.starts_with(&prefix));
    }

    /// Il ciclo modello → strumenti → modello di una risposta.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run_rounds(
        &self,
        run: &RunScope<'_>,
        target: &Target,
        mut chat: Vec<ChatMessage>,
        defs: &[ToolDef],
        settings: &mut ToolSettings,
        folders: &[PathBuf],
        system: &str,
        sampling: &SamplingParams,
        on_event: &mut impl FnMut(ChatEvent),
    ) -> Result<SendOutcome> {
        let conversation_id = run.conversation_id;
        let cancel = run.control.cancel.clone();
        let mut last_reply: Option<Message> = None;
        let mut stats = GenerationStats::default();
        let mut notice = None;
        let mut cancelled = false;
        let capped;
        let sampling = match run.step_tokens {
            Some(limit) => {
                let mut s = sampling.clone();
                s.max_tokens = Some(s.max_tokens.map_or(limit, |m| m.min(limit)));
                capped = s;
                &capped
            }
            None => sampling,
        };
        let mut truncated_retries = 0;

        for round in 1..=run.max_rounds {
            // Una pausa chiesta dall'utente ferma il lavoro qui, tra un passo e l'altro.
            if !run.control.checkpoint().await {
                cancelled = true;
                break;
            }
            let (result, model_name) = match target {
                Target::Local(loaded) => {
                    let mut all = Vec::with_capacity(chat.len() + 1);
                    if !system.is_empty() {
                        all.push(ChatMessage::new("system", system));
                    }
                    all.extend(chat.iter().cloned());
                    let all = chat::fit_template(all, loaded.template);
                    // Un template senza strumenti non li presenterebbe al
                    // modello: llama.cpp ne scarterebbe le definizioni.
                    let defs = if loaded.template.no_tools { &[] } else { defs };
                    let url = format!("http://127.0.0.1:{}/v1/chat/completions", loaded.port);
                    let body = chat::request_body(&all, defs, sampling, None, chat::Flavor::Llama);
                    let result = chat::stream_chat(
                        &self.http,
                        &url,
                        Some(&loaded.api_key),
                        &body,
                        cancel.clone(),
                        &mut *on_event,
                    )
                    .await;
                    self.touch_local();
                    (result, loaded.model_name.clone())
                }
                Target::Cloud(provider, key, id) => {
                    self.check_spend(conversation_id)?;
                    let result = providers::stream(
                        &self.cloud_http,
                        provider,
                        key,
                        id,
                        &chat,
                        defs,
                        system,
                        sampling,
                        cancel.clone(),
                        &mut *on_event,
                    )
                    .await;
                    (result, id.clone())
                }
            };
            let mut outcome = result?;
            stats = outcome.stats.clone();
            cancelled = outcome.cancelled || run.control.is_stopped();
            // Una risposta troncata o rifiutata può contenere chiamate a metà:
            // i suoi strumenti non si eseguono.
            if outcome.notice.is_some() {
                notice = outcome.notice.take();
                outcome.tool_calls.clear();
            }
            let calls = std::mem::take(&mut outcome.tool_calls);
            // Un agente troncato a metà passo riprova in breve invece di fermarsi.
            let retry = run.step_tokens.is_some()
                && !cancelled
                && notice.as_deref() == Some(chat::NOTICE_TRUNCATED)
                && truncated_retries < MAX_TRUNCATED_RETRIES
                && round < run.max_rounds;

            if !outcome.text.is_empty() || !calls.is_empty() {
                // Il costo si stima solo per i modelli cloud di cui si conosce il prezzo.
                let cost_usd = match target {
                    Target::Cloud(provider, _, id) => {
                        provider.model(id).and_then(|m| m.cost_with_cache(&stats))
                    }
                    Target::Local(_) => None,
                };
                let tool = (!calls.is_empty()).then(|| ToolData {
                    calls: calls.clone(),
                    ..Default::default()
                });
                let store = self.store();
                let message = store.add_message_with(
                    conversation_id,
                    "assistant",
                    &outcome.text,
                    Some(&model_name),
                    tool.as_ref(),
                    &[],
                )?;
                last_reply = Some(store.set_message_usage(
                    message.id,
                    stats.prompt_tokens,
                    stats.completion_tokens,
                    cost_usd,
                )?);
            }
            if retry {
                truncated_retries += 1;
                notice = None;
                log::info(format!(
                    "passo troncato in {}: si riprova ({truncated_retries})",
                    run.source
                ));
                on_event(ChatEvent::Saved);
                if !outcome.text.is_empty() {
                    chat.push(ChatMessage::new("assistant", outcome.text));
                }
                chat.push(ChatMessage::new("user", NUDGE_TRUNCATED));
                continue;
            }
            truncated_retries = 0;
            if calls.is_empty() || cancelled {
                break;
            }
            if round == run.max_rounds {
                notice = Some(run.limit_notice.clone());
                // Le chiamate di quest'ultima risposta restano senza seguito:
                // ricevono comunque un risultato, per la coerenza della cronologia.
                for call in &calls {
                    self.save_tool_result(
                        conversation_id,
                        call,
                        &ToolOutput::error("Non eseguito: raggiunto il limite di passi."),
                    )?;
                }
                break;
            }

            on_event(ChatEvent::Saved);
            chat.push(ChatMessage {
                tool_calls: calls.clone(),
                native: outcome.native.take(),
                ..ChatMessage::new("assistant", outcome.text)
            });

            for call in &calls {
                // Dopo un'interruzione le chiamate rimaste ricevono comunque
                // un risultato: la cronologia deve restare coerente.
                if !cancelled && !run.control.checkpoint().await {
                    cancelled = true;
                }
                let output = if cancelled {
                    None
                } else {
                    let known = known_urls(&chat);
                    let result = self
                        .run_call(run, call, settings, folders, &known, &mut *on_event)
                        .await;
                    self.log_action(run, call, &result);
                    result.output
                };
                cancelled |= output.is_none();
                let output = output.unwrap_or_else(|| ToolOutput::error("Interrotto dall'utente."));
                self.save_tool_result(conversation_id, call, &output)?;
                chat.push(ChatMessage {
                    tool_call_id: Some(call.id.clone()),
                    is_error: output.is_error,
                    ..ChatMessage::new("tool", output.content)
                });
                on_event(ChatEvent::Saved);
            }
            if cancelled {
                break;
            }
        }

        // Strumenti attivi ma un modello che non sa usarli: lo si dice, invece
        // di lasciar credere che li abbia provati.
        let no_tools = matches!(target, Target::Local(loaded) if loaded.template.no_tools);
        if notice.is_none() && no_tools && !defs.is_empty() && !cancelled {
            notice = Some(chat::NOTICE_NO_TOOLS.to_string());
        }

        Ok(SendOutcome {
            conversation: self.store().conversation(conversation_id)?,
            assistant_message: last_reply,
            stats,
            cancelled,
            notice,
        })
    }

    pub(crate) fn save_tool_result(
        &self,
        conversation_id: i64,
        call: &ToolCall,
        output: &ToolOutput,
    ) -> Result<()> {
        self.store().add_message_with(
            conversation_id,
            "tool",
            &output.content,
            None,
            Some(&ToolData {
                call_id: Some(call.id.clone()),
                name: Some(call.name.clone()),
                is_error: output.is_error,
                ..Default::default()
            }),
            &[],
        )?;
        Ok(())
    }

    /// Annota l'azione nel registro (SI-6). Un registro che non si scrive non
    /// deve fermare il lavoro.
    pub(crate) fn log_action(&self, run: &RunScope<'_>, call: &ToolCall, result: &CallResult) {
        let (approval, is_error, text) = match &result.output {
            Some(output) => (result.approval, output.is_error, output.content.as_str()),
            None => ("interrupted", true, "Interrotto dall'utente."),
        };
        let _ = self.store().add_action(&NewAction {
            conversation_id: run.conversation_id,
            session_id: run.session_id,
            source: &run.source,
            tool: &call.name,
            summary: &summary_of(run, call),
            approval,
            is_error,
            result: text,
        });
        self.prune_actions_if_due();
    }

    /// Le azioni compiute dagli strumenti, dalla più recente; con `session_id`
    /// solo quelle di una sessione.
    pub fn actions(&self, limit: u32, session_id: Option<i64>) -> Result<Vec<ActionRecord>> {
        self.store().actions(limit.clamp(1, 1000), session_id)
    }

    /// Per quanti giorni si tengono le voci del registro; `0` = per sempre.
    pub fn action_log_days(&self) -> Result<u32> {
        self.store().action_log_days()
    }

    /// Cambia la durata del registro e toglie subito le voci troppo vecchie.
    /// Restituisce quante voci sono state eliminate.
    pub fn set_action_log_days(&self, days: u32) -> Result<usize> {
        let store = self.store();
        store.set_setting(SETTING_ACTION_LOG_DAYS, &days.to_string())?;
        store.prune_actions_by_setting()
    }

    /// Svuota il registro delle azioni; restituisce quante voci c'erano.
    pub fn clear_actions(&self) -> Result<usize> {
        self.store().clear_actions()
    }

    /// Ripulisce il registro se dall'ultima volta è passato abbastanza tempo:
    /// l'app può restare aperta per settimane.
    pub(crate) fn prune_actions_if_due(&self) {
        let mut last = self
            .last_action_prune
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if last.elapsed() >= ACTION_PRUNE_INTERVAL {
            *last = std::time::Instant::now();
            let _ = self.store().prune_actions_by_setting();
        }
    }
}
