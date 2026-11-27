//! Agenti pianificati (AG-9, ADR 0028): un compito che parte da solo a un
//! orario o a intervalli.
//!
//! Qui stanno solo la definizione di una pianificazione e il calcolo delle
//! date; l'esecuzione (caricare il modello, avviare la sessione, scaricarlo)
//! è in `ops::schedules`.

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Durata massima predefinita di un'esecuzione, in minuti.
pub const DEFAULT_MAX_MINUTES: u32 = 60;
/// Limiti accettati per la durata massima, in minuti.
pub const MAX_MINUTES_RANGE: (u32, u32) = (5, 480);
/// Quanto si aspetta, al massimo, che il motore locale si liberi (ADR 0028).
pub const WAIT_FOR_ENGINE_SECS: u64 = 60 * 60;
/// Un'esecuzione partita entro questo ritardo non conta come "persa": il
/// controllo gira ogni 30 secondi e il PC può essere lento ad avviarsi.
pub const LATE_GRACE_SECS: i64 = 2 * 60;
/// Lunghezza massima del compito.
pub const MAX_TASK_CHARS: usize = 20_000;

/// Quando parte un compito. Gli orari sono nell'ora locale del PC; i giorni
/// della settimana vanno da 1 (lunedì) a 7 (domenica), vuoto = tutti.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Recurrence {
    /// Una volta sola: `at` come `2026-11-28T08:00`.
    Once { at: String },
    /// Ogni giorno (o nei giorni scelti) all'ora `time`, come `08:00`.
    Daily {
        time: String,
        #[serde(default)]
        weekdays: Vec<u8>,
    },
    /// Ogni `hours` ore a partire da `start`, ricominciando ogni giorno da
    /// `start`: ogni 5 ore dalle 08:00 vuol dire 08, 13, 18 e 23.
    #[serde(rename_all = "camelCase")]
    EveryHours {
        hours: u32,
        start: String,
        #[serde(default)]
        weekdays: Vec<u8>,
    },
}

impl Default for Recurrence {
    fn default() -> Self {
        Self::Daily {
            time: "08:00".into(),
            weekdays: Vec::new(),
        }
    }
}

fn parse_time(text: &str) -> Result<Time> {
    text.trim()
        .parse::<Time>()
        .map_err(|_| Error::msg(format!("Ora non valida: \"{text}\" (scrivila come 08:00).")))
}

fn parse_datetime(text: &str) -> Result<DateTime> {
    text.trim().parse::<DateTime>().map_err(|_| {
        Error::msg(format!(
            "Data non valida: \"{text}\" (scrivila come 2026-11-28T08:00)."
        ))
    })
}

impl Recurrence {
    /// Controlla i valori e li riporta nella forma canonica.
    pub fn normalized(self) -> Result<Self> {
        let days = |mut weekdays: Vec<u8>| -> Result<Vec<u8>> {
            if weekdays.iter().any(|d| !(1..=7).contains(d)) {
                return Err(Error::msg("Giorni della settimana non validi."));
            }
            weekdays.sort_unstable();
            weekdays.dedup();
            // Tutti i giorni scelti equivale a nessuna scelta.
            if weekdays.len() == 7 {
                weekdays.clear();
            }
            Ok(weekdays)
        };
        let hm = |time: Time| time.strftime("%H:%M").to_string();
        Ok(match self {
            Self::Once { at } => Self::Once {
                at: parse_datetime(&at)?.strftime("%Y-%m-%dT%H:%M").to_string(),
            },
            Self::Daily { time, weekdays } => Self::Daily {
                time: hm(parse_time(&time)?),
                weekdays: days(weekdays)?,
            },
            Self::EveryHours {
                hours,
                start,
                weekdays,
            } => {
                if !(1..=24).contains(&hours) {
                    return Err(Error::msg("L'intervallo va da 1 a 24 ore."));
                }
                Self::EveryHours {
                    hours,
                    start: hm(parse_time(&start)?),
                    weekdays: days(weekdays)?,
                }
            }
        })
    }

    /// Dopo quanti secondi un'esecuzione persa non si recupera più: il turno
    /// successivo è già vicino (ADR 0028).
    pub fn period_secs(&self) -> i64 {
        match self {
            Self::Once { .. } | Self::Daily { .. } => 24 * 60 * 60,
            Self::EveryHours { hours, .. } => i64::from(*hours) * 60 * 60,
        }
    }

    /// Gli orari della giornata in cui parte, in ordine.
    fn times_of_day(&self) -> Result<Vec<Time>> {
        Ok(match self {
            Self::Once { .. } => Vec::new(),
            Self::Daily { time, .. } => vec![parse_time(time)?],
            Self::EveryHours { hours, start, .. } => {
                let start = parse_time(start)?;
                let first = i64::from(start.hour()) * 60 + i64::from(start.minute());
                let step = i64::from((*hours).max(1)) * 60;
                (0..)
                    .map(|k| first + k * step)
                    .take_while(|&minutes| minutes < 24 * 60)
                    .map(|minutes| Time::constant((minutes / 60) as i8, (minutes % 60) as i8, 0, 0))
                    .collect()
            }
        })
    }

    fn allows(&self, date: Date) -> bool {
        let weekdays = match self {
            Self::Once { .. } => return true,
            Self::Daily { weekdays, .. } | Self::EveryHours { weekdays, .. } => weekdays,
        };
        let day = date.weekday().to_monday_one_offset() as u8;
        weekdays.is_empty() || weekdays.contains(&day)
    }

    /// La prima esecuzione dopo `after` (esclusa), nel fuso `tz`. `None` se
    /// non ce ne sono più (una volta sola, già passata).
    ///
    /// Un orario che quel giorno non esiste (passaggio all'ora legale)
    /// diventa il primo valido dopo; uno che esiste due volte vale solo la
    /// prima: è la regola "compatibile" di jiff.
    pub fn next_after(&self, after: Timestamp, tz: &TimeZone) -> Result<Option<Timestamp>> {
        if let Self::Once { at } = self {
            let when = parse_datetime(at)?
                .to_zoned_datetime(tz.clone())
                .map_err(|e| Error::msg(e.to_string()))?
                .timestamp();
            return Ok((when > after).then_some(when));
        }
        let times = self.times_of_day()?;
        let mut date = after.to_zoned_datetime(tz.clone()).date();
        // Otto giorni bastano: ogni giorno della settimana compare almeno una volta.
        for _ in 0..8 {
            if self.allows(date) {
                for time in &times {
                    let when = date
                        .to_datetime(*time)
                        .to_zoned_datetime(tz.clone())
                        .map_err(|e| Error::msg(e.to_string()))?
                        .timestamp();
                    if when > after {
                        return Ok(Some(when));
                    }
                }
            }
            date = date
                .checked_add(1.day())
                .map_err(|e| Error::msg(e.to_string()))?;
        }
        Ok(None)
    }
}

/// Cosa fare se all'ora stabilita è caricato un altro modello locale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum OnConflict {
    /// Aspetta che quel modello venga scaricato (a mano o per inattività).
    #[default]
    Wait,
    /// Lo sostituisce appena nessuno lo sta usando.
    Replace,
}

/// Cosa fare con un'esecuzione persa (app chiusa, PC spento o in sospensione).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum OnMissed {
    /// La esegue appena possibile, una volta sola anche se ne ha perse diverse.
    #[default]
    CatchUp,
    Skip,
}

/// Quando avvisare con una notifica.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum NotifyWhen {
    #[default]
    Always,
    /// Solo se l'esecuzione non è andata a buon fine, è stata saltata o ha
    /// avuto strumenti rifiutati.
    Problems,
    Never,
}

/// Com'è andata l'ultima esecuzione.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ScheduleOutcome {
    Done,
    Failed,
    Stopped,
    Skipped,
}

impl ScheduleOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "done" => Self::Done,
            "failed" => Self::Failed,
            "stopped" => Self::Stopped,
            "skipped" => Self::Skipped,
            _ => return None,
        })
    }
}

/// Una pianificazione: un agente, un compito e quando eseguirlo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Schedule {
    /// 0 per una non ancora salvata.
    pub id: i64,
    pub agent_id: i64,
    pub task: String,
    pub recurrence: Recurrence,
    /// Il file GGUF da caricare, per gli agenti con modello locale.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub local_model: Option<String>,
    pub on_conflict: OnConflict,
    pub on_missed: OnMissed,
    pub notify: NotifyWhen,
    /// Oltre questi minuti la sessione si ferma.
    pub max_minutes: u32,
    /// Sospesa: resta salvata ma non parte.
    pub enabled: bool,
    // ── Stato, scritto dal nucleo: ignorato quando si salva ──
    /// Prossima esecuzione, in secondi dal 1970 (UTC).
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run: Option<i64>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<i64>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_outcome: Option<ScheduleOutcome>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_note: Option<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_session_id: Option<i64>,
    /// In coda o al lavoro in questo momento.
    pub running: bool,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            id: 0,
            agent_id: 0,
            task: String::new(),
            recurrence: Recurrence::default(),
            local_model: None,
            on_conflict: OnConflict::Wait,
            on_missed: OnMissed::CatchUp,
            notify: NotifyWhen::Always,
            max_minutes: DEFAULT_MAX_MINUTES,
            enabled: true,
            next_run: None,
            last_run: None,
            last_outcome: None,
            last_note: None,
            last_session_id: None,
            running: false,
        }
    }
}

impl Schedule {
    /// Ripulisce e controlla i campi scritti dall'utente.
    pub fn normalized(mut self) -> Result<Self> {
        self.task = self.task.trim().to_string();
        if self.task.is_empty() {
            return Err(Error::msg("Scrivi il compito da eseguire."));
        }
        if self.task.chars().count() > MAX_TASK_CHARS {
            return Err(Error::msg("Il compito è troppo lungo."));
        }
        self.recurrence = self.recurrence.normalized()?;
        self.local_model = self
            .local_model
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty());
        self.max_minutes = self
            .max_minutes
            .clamp(MAX_MINUTES_RANGE.0, MAX_MINUTES_RANGE.1);
        Ok(self)
    }

    /// Solo i campi scelti dall'utente, come si salvano in archivio.
    pub fn definition(&self) -> Self {
        Self {
            id: 0,
            next_run: None,
            last_run: None,
            last_outcome: None,
            last_note: None,
            last_session_id: None,
            running: false,
            ..self.clone()
        }
    }
}

/// Un'esecuzione conclusa o saltata, per le notifiche.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleEvent {
    pub schedule_id: i64,
    pub agent_name: String,
    pub outcome: ScheduleOutcome,
    /// Errore, avviso o motivo del salto.
    pub note: Option<String>,
    pub session_id: Option<i64>,
    /// Prima riga dell'ultima risposta dell'agente.
    pub summary: String,
    /// Strumenti rifiutati perché avrebbero chiesto conferma.
    pub refused: u32,
    /// Va mostrata una notifica, secondo la scelta della pianificazione.
    pub notify: bool,
}

/// Ora locale leggibile di un istante, per il testo del compito.
pub fn local_label(at: Timestamp, tz: &TimeZone) -> String {
    at.to_zoned_datetime(tz.clone())
        .strftime("%d/%m/%Y %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rome() -> TimeZone {
        TimeZone::get("Europe/Rome").unwrap()
    }

    fn at(text: &str) -> Timestamp {
        parse_datetime(text)
            .unwrap()
            .to_zoned_datetime(rome())
            .unwrap()
            .timestamp()
    }

    fn next(rec: &Recurrence, after: &str) -> Option<String> {
        rec.next_after(at(after), &rome())
            .unwrap()
            .map(|t| local_label(t, &rome()))
    }

    #[test]
    fn ogni_giorno_alla_stessa_ora() {
        let rec = Recurrence::Daily {
            time: "08:00".into(),
            weekdays: vec![],
        };
        assert_eq!(next(&rec, "2026-11-27T07:59").unwrap(), "27/11/2026 08:00");
        // L'orario esatto è escluso: è appena partito.
        assert_eq!(next(&rec, "2026-11-27T08:00").unwrap(), "28/11/2026 08:00");
    }

    #[test]
    fn solo_nei_giorni_scelti() {
        // Il 27-11-2026 è un venerdì: il prossimo lunedì è il 30.
        let rec = Recurrence::Daily {
            time: "09:30".into(),
            weekdays: vec![1],
        };
        assert_eq!(next(&rec, "2026-11-27T10:00").unwrap(), "30/11/2026 09:30");
    }

    #[test]
    fn ogni_n_ore_ricomincia_ogni_giorno() {
        let rec = Recurrence::EveryHours {
            hours: 5,
            start: "08:00".into(),
            weekdays: vec![],
        };
        assert_eq!(next(&rec, "2026-11-27T08:00").unwrap(), "27/11/2026 13:00");
        assert_eq!(next(&rec, "2026-11-27T22:00").unwrap(), "27/11/2026 23:00");
        assert_eq!(next(&rec, "2026-11-27T23:30").unwrap(), "28/11/2026 08:00");
    }

    #[test]
    fn una_volta_sola() {
        let rec = Recurrence::Once {
            at: "2026-12-01T18:00".into(),
        };
        assert_eq!(next(&rec, "2026-11-27T10:00").unwrap(), "01/12/2026 18:00");
        assert_eq!(next(&rec, "2026-12-01T18:00"), None);
    }

    #[test]
    fn l_ora_che_non_esiste_diventa_la_prima_valida_dopo() {
        // Il 29-03-2026 in Italia si passa dalle 02:00 alle 03:00.
        let rec = Recurrence::Daily {
            time: "02:30".into(),
            weekdays: vec![],
        };
        assert_eq!(next(&rec, "2026-03-28T12:00").unwrap(), "29/03/2026 03:30");
        // Il 25-10-2026 le 02:30 capitano due volte: vale la prima.
        let first = rec
            .next_after(at("2026-10-24T12:00"), &rome())
            .unwrap()
            .unwrap();
        let after_first = rec.next_after(first, &rome()).unwrap().unwrap();
        assert_eq!(local_label(after_first, &rome()), "26/10/2026 02:30");
    }

    #[test]
    fn i_valori_vengono_controllati() {
        let all = Recurrence::Daily {
            time: "08:05".into(),
            weekdays: vec![1, 2, 3, 4, 5, 6, 7],
        }
        .normalized()
        .unwrap();
        assert_eq!(
            all,
            Recurrence::Daily {
                time: "08:05".into(),
                weekdays: vec![]
            }
        );
        let some = Recurrence::Daily {
            time: "08:05".into(),
            weekdays: vec![3, 1, 3],
        }
        .normalized()
        .unwrap();
        assert_eq!(
            some,
            Recurrence::Daily {
                time: "08:05".into(),
                weekdays: vec![1, 3]
            }
        );
        for bad in [
            Recurrence::Daily {
                time: "25:00".into(),
                weekdays: vec![],
            },
            Recurrence::Daily {
                time: "08:00".into(),
                weekdays: vec![8],
            },
            Recurrence::EveryHours {
                hours: 0,
                start: "08:00".into(),
                weekdays: vec![],
            },
            Recurrence::Once {
                at: "domani".into(),
            },
        ] {
            assert!(bad.normalized().is_err());
        }
        let schedule = Schedule {
            task: "   ".into(),
            ..Default::default()
        };
        assert!(schedule.normalized().is_err());
        let schedule = Schedule {
            task: " Riassumi ".into(),
            max_minutes: 1,
            local_model: Some("  ".into()),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(schedule.task, "Riassumi");
        assert_eq!(schedule.max_minutes, MAX_MINUTES_RANGE.0);
        assert_eq!(schedule.local_model, None);
    }
}
