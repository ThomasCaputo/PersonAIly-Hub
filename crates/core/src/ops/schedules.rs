//! Agenti pianificati (ADR 0028): salvare le pianificazioni, accorgersi di
//! quelle dovute, preparare il modello locale, eseguire la sessione senza
//! sorveglianza e poi scaricare il modello, se l'aveva caricato la pianificazione.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::schedules::{
    LATE_GRACE_SECS, NotifyWhen, OnConflict, OnMissed, Schedule, ScheduleEvent, ScheduleOutcome,
    WAIT_FOR_ENGINE_SECS, local_label,
};
use crate::*;

/// Ogni quanto si riguarda il motore mentre si aspetta che si liberi.
const ENGINE_POLL: Duration = Duration::from_secs(30);
/// Caratteri del resoconto mostrati nella notifica.
const SUMMARY_CHARS: usize = 200;

/// Due percorsi dello stesso file, su Windows dove maiuscole e barre non contano.
fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

/// Il motore locale pronto per l'esecuzione, oppure il motivo per saltarla.
enum Prepared {
    /// `true` se il modello l'ha caricato la pianificazione: a fine lavoro si scarica.
    Ready(bool),
    Skip(String),
}

/// Com'è finita un'esecuzione.
struct Finished {
    outcome: ScheduleOutcome,
    note: Option<String>,
    session_id: Option<i64>,
}

impl Core {
    /// Qualcuno che non è una sessione pianificata sta usando (o sta per
    /// usare) il modello locale.
    pub(crate) fn note_local_activity(&self) {
        self.local_activity.fetch_add(1, Ordering::Relaxed);
    }

    /// Le pianificazioni, dalla prossima a partire.
    pub fn schedules(&self) -> Result<Vec<Schedule>> {
        let running = self
            .schedules_running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        Ok(self
            .store()
            .schedules()?
            .into_iter()
            .map(|mut s| {
                s.running = running.contains(&s.id);
                s
            })
            .collect())
    }

    /// Crea (con `id` 0) o aggiorna una pianificazione. La prossima
    /// esecuzione si calcola da adesso: riattivarne una sospesa non fa
    /// recuperare quelle saltate nel frattempo.
    pub fn save_schedule(&self, schedule: Schedule) -> Result<Schedule> {
        let mut schedule = schedule.normalized()?;
        let store = self.store();
        let agent = store.agent(schedule.agent_id)?;
        if let ModelRef::Local { path: Some(own) } = &agent.model {
            // L'agente ha già il suo modello: vale quello.
            schedule.local_model = Some(own.clone());
        } else if matches!(agent.model, ModelRef::Local { .. }) {
            if schedule.local_model.is_none() {
                return Err(Error::msg(
                    "L'agente usa un modello locale: scegli quale caricare.",
                ));
            }
        } else {
            schedule.local_model = None;
        }
        let next = if schedule.enabled {
            let next = schedule
                .recurrence
                .next_after(Timestamp::now(), &TimeZone::system())?;
            if next.is_none() {
                return Err(Error::msg("La data scelta è già passata."));
            }
            next.map(|t| t.as_second())
        } else {
            None
        };
        store.save_schedule(&schedule, next)
    }

    /// Elimina la pianificazione. Le sessioni che ha avviato restano; se sta
    /// lavorando, quel lavoro finisce comunque.
    pub fn delete_schedule(&self, id: i64) -> Result<()> {
        self.store().delete_schedule(id)
    }

    /// Una pianificazione è in coda o al lavoro: chi si chiude per inattività
    /// (`app-server`) deve aspettare.
    pub fn has_scheduled_work(&self) -> bool {
        !self
            .schedules_running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    /// Per le notifiche dell'app: un messaggio per ogni esecuzione conclusa o saltata.
    pub fn schedule_events(&self) -> tokio::sync::broadcast::Receiver<ScheduleEvent> {
        self.schedule_events.subscribe()
    }

    /// Esegue subito il compito, con le stesse regole di un'esecuzione
    /// pianificata (strumenti da confermare rifiutati compresi). Va chiamata
    /// dentro il runtime di tokio.
    pub fn run_schedule_now(self: &Arc<Self>, id: i64) -> Result<()> {
        self.store().schedule(id)?;
        if !self.claim_schedule(id) {
            return Err(Error::msg("Questa pianificazione sta già lavorando."));
        }
        tokio::spawn(self.clone().execute_schedule(id));
        Ok(())
    }

    /// Segna la pianificazione come in coda; `false` se lo era già.
    fn claim_schedule(&self, id: i64) -> bool {
        self.schedules_running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id)
    }

    /// Dal compito di manutenzione: avvia le pianificazioni dovute e salta
    /// quelle perse da troppo tempo.
    pub(crate) fn check_schedules(self: &Arc<Self>) {
        let now = Timestamp::now();
        let tz = TimeZone::system();
        let Ok(list) = self.store().schedules() else {
            return;
        };
        for schedule in list.into_iter().filter(|s| s.enabled) {
            let Some(due) = schedule.next_run else {
                continue;
            };
            if due > now.as_second() {
                continue;
            }
            // Si guarda avanti da adesso: le esecuzioni perse non si accumulano.
            let next = schedule
                .recurrence
                .next_after(now, &tz)
                .ok()
                .flatten()
                .map(|t| t.as_second());
            if self
                .store()
                .set_schedule_next_run(schedule.id, next)
                .is_err()
            {
                continue;
            }
            let late = now.as_second() - due;
            if late > LATE_GRACE_SECS
                && (schedule.on_missed == OnMissed::Skip
                    || late > schedule.recurrence.period_secs())
            {
                self.finish_schedule(
                    &schedule,
                    Finished {
                        outcome: ScheduleOutcome::Skipped,
                        note: Some(
                            "Esecuzione persa: all'ora stabilita l'app era chiusa o il PC spento."
                                .into(),
                        ),
                        session_id: None,
                    },
                );
                continue;
            }
            if !self.claim_schedule(schedule.id) {
                self.finish_schedule(
                    &schedule,
                    Finished {
                        outcome: ScheduleOutcome::Skipped,
                        note: Some(
                            "Il lavoro della volta precedente non era ancora finito.".into(),
                        ),
                        session_id: None,
                    },
                );
                continue;
            }
            tokio::spawn(self.clone().execute_schedule(schedule.id));
        }
    }

    /// Esegue una pianificazione già segnata come in coda, quando arriva il suo turno.
    async fn execute_schedule(self: Arc<Self>, id: i64) {
        let turn = self.schedule_turn.lock().await;
        // Un lucchetto dell'archivio alla volta: niente chiamate annidate.
        let schedule = self.store().schedule(id);
        let agent = schedule
            .as_ref()
            .ok()
            .map(|s| self.store().agent(s.agent_id));
        if let (Ok(schedule), Some(Ok(agent))) = (schedule, agent) {
            log::info(format!(
                "pianificazione {id}: parte il lavoro di {}",
                agent.name
            ));
            let finished = match self.execute(&schedule, &agent).await {
                Ok(finished) => finished,
                Err(error) => {
                    log::error(format!("pianificazione {id} non riuscita: {error}"));
                    Finished {
                        outcome: ScheduleOutcome::Failed,
                        note: Some(error.to_string()),
                        session_id: None,
                    }
                }
            };
            self.finish_schedule(&schedule, finished);
        }
        drop(turn);
        self.schedules_running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }

    async fn execute(&self, schedule: &Schedule, agent: &Agent) -> Result<Finished> {
        // Il modello locale: quello scelto, caricato se serve.
        let mut ours = None;
        let mut run_agent = agent.clone();
        if matches!(agent.model, ModelRef::Local { .. }) {
            let path = schedule.local_model.clone().ok_or_else(|| {
                Error::msg("L'agente usa un modello locale ma la pianificazione non dice quale.")
            })?;
            // La sessione usa proprio questo modello, anche se non è il principale.
            run_agent.model = ModelRef::Local {
                path: Some(path.clone()),
            };
            match self.prepare_local(&path, schedule.on_conflict).await? {
                Prepared::Ready(loaded_here) => {
                    if loaded_here {
                        ours = Some(path);
                    }
                }
                Prepared::Skip(note) => {
                    return Ok(Finished {
                        outcome: ScheduleOutcome::Skipped,
                        note: Some(note),
                        session_id: None,
                    });
                }
            }
        }
        let activity = self.local_activity.load(Ordering::Relaxed);

        let when = local_label(Timestamp::now(), &TimeZone::system());
        let header = if self.language().ok().flatten().as_deref() == Some("en") {
            format!("Scheduled run, {when}.")
        } else {
            format!("Esecuzione pianificata del {when}.")
        };
        let task = format!("{header}\n\n{}", schedule.task);
        let session_id = {
            let store = self.store();
            let session = store.create_session(&run_agent, &task)?;
            store.set_session_schedule(session.id, schedule.id)?;
            store.set_session_unattended(session.id, true)?;
            session.id
        };

        let limit = Duration::from_secs(u64::from(schedule.max_minutes) * 60);
        let run = self.run_session(session_id);
        tokio::pin!(run);
        let timed_out = tokio::select! {
            _ = &mut run => false,
            _ = tokio::time::sleep(limit) => {
                self.stop_session(session_id);
                let _ = (&mut run).await;
                true
            }
        };

        if let Some(path) = ours {
            self.unload_if_unused(&path, activity).await;
        }

        let session = self.store().session(session_id)?;
        let (outcome, note) = if timed_out {
            let note = format!(
                "Fermata dopo {} minuti, la durata massima scelta.",
                schedule.max_minutes
            );
            self.store()
                .set_session_status(session_id, SessionStatus::Stopped, Some(&note))?;
            (ScheduleOutcome::Stopped, Some(note))
        } else {
            let outcome = match session.status {
                SessionStatus::Done => ScheduleOutcome::Done,
                SessionStatus::Failed => ScheduleOutcome::Failed,
                _ => ScheduleOutcome::Stopped,
            };
            (outcome, session.note)
        };
        Ok(Finished {
            outcome,
            note,
            session_id: Some(session_id),
        })
    }

    /// Porta il modello `path` fra quelli caricati, rispettando la scelta su
    /// cosa fare se non c'è posto. Aspetta al massimo un'ora.
    async fn prepare_local(&self, path: &str, on_conflict: OnConflict) -> Result<Prepared> {
        let deadline = Instant::now() + Duration::from_secs(WAIT_FOR_ENGINE_SECS);
        loop {
            // Un caricamento in corso: si riguarda al giro dopo.
            if !self.engine_list().iter().any(|e| e.is_busy()) {
                if let Some(loaded_here) = self.load_alongside(path).await? {
                    return Ok(Prepared::Ready(loaded_here));
                }
                if on_conflict == OnConflict::Replace && !self.is_busy() {
                    log::info("pianificazione: faccio posto al suo modello");
                    self.make_room_for(path).await?;
                    return Ok(Prepared::Ready(true));
                }
            }
            if Instant::now() >= deadline {
                return Ok(Prepared::Skip(match on_conflict {
                    OnConflict::Wait => "Era caricato un altro modello e nessuno l'ha scaricato \
                                         entro un'ora."
                        .into(),
                    OnConflict::Replace => {
                        "Il modello locale è rimasto occupato per un'ora.".into()
                    }
                }));
            }
            tokio::time::sleep(ENGINE_POLL).await;
        }
    }

    /// Scarica altri modelli finché `path` non entra, poi lo carica: prima i
    /// modelli inattivi da più tempo, il principale per ultimo. Con un solo
    /// modello alla volta, semplicemente lo sostituisce.
    async fn make_room_for(&self, path: &str) -> Result<()> {
        let params = self.preset(path)?.load;
        let options = LoadOptions {
            make_primary: false,
            ..LoadOptions::default()
        };
        if self.preferences().multi_model {
            let primary = self
                .primary_model()
                .map(|m| local::pool::key(&m.model_path));
            let mut candidates = self.engine_list();
            candidates.sort_by_key(|e| {
                (
                    primary.as_deref() == Some(e.key.as_str()),
                    std::cmp::Reverse(e.idle()),
                )
            });
            for engine in candidates {
                let check = self.check_load_async(path, &params, false).await?;
                if check.fits {
                    break;
                }
                if let Some(model) = engine.loaded() {
                    log::info(format!(
                        "pianificazione: scarico {} per fare posto",
                        model.model_name
                    ));
                    self.unload_local(&model.model_path).await;
                }
            }
            self.load_model_with(path, &params, options).await?;
        } else {
            self.load_model_with(
                path,
                &params,
                LoadOptions {
                    replace_others: true,
                    ..options
                },
            )
            .await?;
        }
        Ok(())
    }

    /// Scarica il modello caricato dalla pianificazione, se c'è ancora e nel
    /// frattempo nessun altro ha lavorato.
    async fn unload_if_unused(&self, path: &str, activity: u64) {
        if self.local_activity.load(Ordering::Relaxed) != activity || self.is_busy() {
            return;
        }
        if self
            .loaded_models()
            .iter()
            .any(|m| same_path(&m.model_path, path))
        {
            log::info("pianificazione finita: scarico il modello che aveva caricato");
            self.unload_local(path).await;
        }
    }

    /// Annota l'esito e lo manda a chi mostra le notifiche.
    fn finish_schedule(&self, schedule: &Schedule, finished: Finished) {
        let store = self.store();
        let _ = store.set_schedule_result(
            schedule.id,
            Timestamp::now().as_second(),
            finished.outcome,
            finished.note.as_deref(),
            finished.session_id,
        );
        let agent_name = store
            .agent(schedule.agent_id)
            .map(|a| a.name)
            .unwrap_or_default();
        let (summary, refused) = match finished.session_id.and_then(|id| store.session(id).ok()) {
            Some(session) => {
                let summary = store
                    .messages(session.conversation_id)
                    .ok()
                    .and_then(|m| {
                        m.into_iter()
                            .rev()
                            .find(|m| m.role == "assistant" && !m.content.trim().is_empty())
                    })
                    .and_then(|m| {
                        m.content
                            .lines()
                            .map(str::trim)
                            .find(|l| !l.is_empty())
                            .map(|l| l.chars().take(SUMMARY_CHARS).collect())
                    })
                    .unwrap_or_default();
                let refused = store.count_actions(session.id, "unattended").unwrap_or(0);
                (summary, refused)
            }
            None => (String::new(), 0),
        };
        drop(store);
        let notify = match schedule.notify {
            NotifyWhen::Always => true,
            NotifyWhen::Problems => finished.outcome != ScheduleOutcome::Done || refused > 0,
            NotifyWhen::Never => false,
        };
        log::info(format!(
            "pianificazione {}: {} ({} strumenti rifiutati){}",
            schedule.id,
            finished.outcome.as_str(),
            refused,
            finished
                .note
                .as_deref()
                .map(|n| format!(" — {n}"))
                .unwrap_or_default()
        ));
        // Nessuno in ascolto (app-server senza finestra) non è un errore.
        let _ = self.schedule_events.send(ScheduleEvent {
            schedule_id: schedule.id,
            agent_name,
            outcome: finished.outcome,
            note: finished.note,
            session_id: finished.session_id,
            summary,
            refused,
            notify,
        });
    }
}
