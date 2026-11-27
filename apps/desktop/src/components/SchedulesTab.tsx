// Agenti pianificati (ADR 0028): elenco, modulo, "Esegui ora" e "Avvia con Windows".

import { useCallback, useEffect, useState } from "react";
import {
  api,
  type Agent,
  type ModelInfo,
  type NotifyWhen,
  type Recurrence,
  type Schedule,
  type ScheduleOutcome,
} from "../api";
import { locale, msg, t, tc } from "../i18n";

const LIST_INTERVAL_MS = 3000;

const OUTCOME_LABELS: Record<ScheduleOutcome, string> = {
  done: msg("Finito"),
  failed: msg("Errore"),
  stopped: msg("Fermato"),
  skipped: msg("Saltata"),
};

const NOTIFY_CHOICES: { value: NotifyWhen; label: string }[] = [
  { value: "always", label: msg("Sempre") },
  { value: "problems", label: msg("Solo se qualcosa va storto") },
  { value: "never", label: msg("Mai") },
];

/** Il nome breve di un giorno, da 1 (lunedì) a 7 (domenica). Il 1° gennaio 2024 era lunedì. */
const weekdayName = (day: number) => new Date(2024, 0, day).toLocaleDateString(locale(), { weekday: "short" });

const baseName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** Un istante (secondi dal 1970) nell'ora di questo computer. */
function moment(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString(locale(), {
    weekday: "short",
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function daysLabel(weekdays: number[]): string {
  return weekdays.map(weekdayName).join(", ");
}

/** La ricorrenza in parole. */
export function recurrenceLabel(rec: Recurrence): string {
  switch (rec.kind) {
    case "once": {
      const date = new Date(rec.at);
      const text = Number.isNaN(date.getTime())
        ? rec.at
        : date.toLocaleString(locale(), { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" });
      return t("Una volta, il {when}", { when: text });
    }
    case "daily":
      return rec.weekdays.length === 0
        ? t("Ogni giorno alle {time}", { time: rec.time })
        : t("{days} alle {time}", { days: daysLabel(rec.weekdays), time: rec.time });
    case "everyHours": {
      const base =
        rec.hours === 1
          ? t("Ogni ora dalle {start}", { start: rec.start })
          : t("Ogni {n} ore dalle {start}", { n: rec.hours, start: rec.start });
      return rec.weekdays.length === 0 ? base : `${base} · ${daysLabel(rec.weekdays)}`;
    }
  }
}

/** Le azioni che, con i permessi attuali dell'agente, verranno rifiutate. */
export function refusedActions(agent: Agent): string[] {
  const { tools } = agent;
  const hasFolders = agent.folders.length > 0;
  const out: string[] = [];
  if (tools.permission === "ask" && tools.files && hasFolders) out.push(t("creare o modificare file"));
  if (tools.permission !== "all") {
    if (tools.terminal && hasFolders) out.push(t("comandi nel terminale"));
    // Ogni esecuzione è una sessione nuova: i "Consenti sempre" dati in una
    // sessione precedente non valgono.
    if (agent.mcpServers.length > 0) out.push(t("strumenti dei server MCP"));
    if (agent.delegates.length > 0) out.push(t("affidare lavoro ad altri agenti"));
    if (tools.files && tools.web && hasFolders) out.push(t("aprire pagine con un indirizzo scritto dal modello"));
  }
  return out;
}

/** Fra un'ora tonda, nel formato del campo data e ora. */
function inAnHour(): string {
  const date = new Date(Date.now() + 60 * 60 * 1000);
  date.setMinutes(0, 0, 0);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:00`;
}

function newSchedule(agentId: number): Schedule {
  return {
    id: 0,
    agentId,
    task: "",
    recurrence: { kind: "daily", time: "08:00", weekdays: [] },
    onConflict: "wait",
    onMissed: "catchUp",
    notify: "always",
    maxMinutes: 60,
    enabled: true,
    running: false,
  };
}

type Props = {
  agents: Agent[];
  /** Apri subito il modulo per una nuova pianificazione di questo agente. */
  newForAgent: number | null;
  onEditorClosed: () => void;
  onOpenSession: (id: number) => void;
};

export function SchedulesTab({ agents, newForAgent, onEditorClosed, onOpenSession }: Props) {
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [editing, setEditing] = useState<Schedule | null>(() => (newForAgent !== null ? newSchedule(newForAgent) : null));
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api.listSchedules().then(setSchedules).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, LIST_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const act = (action: () => Promise<unknown>) => {
    setError(null);
    action()
      .then(refresh)
      .catch((e) => setError(String(e)));
  };

  const close = () => {
    setEditing(null);
    onEditorClosed();
    refresh();
  };

  if (editing) {
    return <ScheduleEditor initial={editing} agents={agents} onCancel={close} onSaved={close} />;
  }

  const agentName = (id: number) => agents.find((a) => a.id === id)?.name ?? t("agente {n}", { n: id });

  return (
    <div className="page">
      <div className="section-head">
        <h1>{t("Pianificazioni")}</h1>
        <button className="btn primary" disabled={agents.length === 0} onClick={() => setEditing(newSchedule(agents[0].id))}>
          {t("Nuova pianificazione")}
        </button>
      </div>
      <p className="dim note">
        {t(
          "Un compito che un agente esegue da solo, a un orario o a intervalli. Se l'agente usa un modello locale, il modello si carica all'ora stabilita e si scarica a lavoro finito. Nessuno è lì a confermare: ciò che chiederebbe conferma viene rifiutato.",
        )}
      </p>
      <BackgroundSettings />
      {agents.length === 0 && <p className="dim">{t("Crea prima un agente.")}</p>}
      {agents.length > 0 && schedules.length === 0 && <p className="dim">{t("Nessuna pianificazione.")}</p>}
      {schedules.map((s) => (
        <div key={s.id} className="card">
          <div className="card-head">
            <div>
              <div className="card-title">
                {agentName(s.agentId)}
                {s.running && <span className="status-badge running">{t("Al lavoro")}</span>}
                {!s.enabled && <span className="status-badge">{t("Sospesa")}</span>}
              </div>
              <div className="dim">{recurrenceLabel(s.recurrence)}</div>
            </div>
            <div className="row">
              <button className="btn" disabled={s.running} onClick={() => act(() => api.runScheduleNow(s.id))}>
                {t("Esegui ora")}
              </button>
              <button className="btn" onClick={() => setEditing(s)}>
                {t("Modifica")}
              </button>
              <button className="btn" onClick={() => act(() => api.saveSchedule({ ...s, enabled: !s.enabled }))}>
                {s.enabled ? t("Sospendi") : t("Riattiva")}
              </button>
              <button
                className="btn"
                onClick={() => {
                  if (window.confirm(t("Eliminare questa pianificazione? Le sessioni che ha avviato restano."))) {
                    act(() => api.deleteSchedule(s.id));
                  }
                }}
              >
                {t("Elimina")}
              </button>
            </div>
          </div>
          <div className="card-body">
            <p className="schedule-task">{s.task}</p>
            <p className="dim note">
              {s.enabled && s.nextRun != null
                ? t("Prossima esecuzione: {when}", { when: moment(s.nextRun) })
                : s.enabled
                  ? t("Nessuna esecuzione in programma.")
                  : t("Sospesa: non parte finché non la riattivi.")}
              {s.localModel && ` · ${baseName(s.localModel)}`}
            </p>
            {s.lastRun != null && s.lastOutcome && (
              <p className="dim note">
                {t("Ultima: {when}, {outcome}", { when: moment(s.lastRun), outcome: t(OUTCOME_LABELS[s.lastOutcome]) })}
                {s.lastNote && ` — ${tc(s.lastNote)}`}{" "}
                {s.lastSessionId != null && (
                  <button className="btn-link" onClick={() => onOpenSession(s.lastSessionId!)}>
                    {t("Apri la sessione")}
                  </button>
                )}
              </p>
            )}
          </div>
        </div>
      ))}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </div>
  );
}

/** "Avvia con Windows", e un avviso se chiudere la finestra ferma tutto. */
function BackgroundSettings() {
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [staysInTray, setStaysInTray] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.autostartEnabled().then(setAutostart).catch(() => setAutostart(false));
    Promise.all([api.runsInTray(), api.preferences()])
      .then(([tray, prefs]) => setStaysInTray(tray && prefs.keepRunning))
      .catch(() => {});
  }, []);

  const toggle = (enabled: boolean) => {
    setError(null);
    api
      .setAutostart(enabled)
      .then(() => setAutostart(enabled))
      .catch((e) => setError(String(e)));
  };

  return (
    <div className="section">
      <label className="check">
        <input
          type="checkbox"
          checked={autostart === true}
          disabled={autostart === null}
          onChange={(e) => toggle(e.target.checked)}
        />
        <span>
          <strong>{t("Avvia con Windows")}</strong>
          <br />
          <span className="dim">
            {t(
              "All'accesso l'app parte da sola nell'area di notifica, senza finestra: le pianificazioni girano anche se non la apri. Se il PC è spento o in sospensione all'ora stabilita, il compito si recupera al riavvio (o si salta, a tua scelta).",
            )}
          </span>
        </span>
      </label>
      {!staysInTray && (
        <p className="notice">
          {t(
            "Chiudendo la finestra l'app si chiude del tutto e le pianificazioni si fermano: accendi «Resta attiva nell'area di notifica» nelle Impostazioni.",
          )}
        </p>
      )}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </div>
  );
}

function WeekdayPicker({ value, onChange }: { value: number[]; onChange: (days: number[]) => void }) {
  return (
    <fieldset className="field">
      <legend>{t("Giorni")}</legend>
      <div className="tools-bar">
        {[1, 2, 3, 4, 5, 6, 7].map((day) => (
          <label key={day}>
            <input
              type="checkbox"
              checked={value.length === 0 || value.includes(day)}
              onChange={(e) => {
                const all = value.length === 0 ? [1, 2, 3, 4, 5, 6, 7] : value;
                const next = e.target.checked ? [...all, day] : all.filter((d) => d !== day);
                // Tutti i giorni si salvano come "nessuna scelta".
                onChange(next.length === 7 ? [] : [...new Set(next)].sort());
              }}
            />
            {weekdayName(day)}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

function ScheduleEditor({
  initial,
  agents,
  onCancel,
  onSaved,
}: {
  initial: Schedule;
  agents: Agent[];
  onCancel: () => void;
  onSaved: () => void;
}) {
  const [schedule, setSchedule] = useState<Schedule>(initial);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Schedule>) => setSchedule((s) => ({ ...s, ...patch }));
  const rec = schedule.recurrence;
  const setRec = (patch: Partial<Recurrence>) => set({ recurrence: { ...rec, ...patch } as Recurrence });

  useEffect(() => {
    api.listModels().then(setModels).catch(() => {});
  }, []);

  const agent = agents.find((a) => a.id === schedule.agentId);
  const local = agent?.model.kind === "local";
  /** L'agente ha già un suo modello locale: la pianificazione usa quello. */
  const ownModel = agent?.model.kind === "local" ? agent.model.path : undefined;
  const refused = agent ? refusedActions(agent) : [];
  // Il modello salvato non c'è più nelle cartelle dei modelli: si mostra lo stesso.
  const modelChoices =
    schedule.localModel && !models.some((m) => m.path === schedule.localModel)
      ? [...models, { path: schedule.localModel, name: baseName(schedule.localModel) } as ModelInfo]
      : models;

  const changeKind = (kind: Recurrence["kind"]) => {
    if (kind === rec.kind) return;
    const weekdays = rec.kind === "once" ? [] : rec.weekdays;
    const time = rec.kind === "daily" ? rec.time : rec.kind === "everyHours" ? rec.start : "08:00";
    set({
      recurrence:
        kind === "once"
          ? { kind, at: inAnHour() }
          : kind === "daily"
            ? { kind, time, weekdays }
            : { kind, hours: 4, start: time, weekdays },
    });
  };

  const save = async () => {
    setError(null);
    try {
      await api.saveSchedule({ ...schedule, localModel: local ? (ownModel ?? schedule.localModel) : undefined });
      onSaved();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="page agent-editor">
      <h1>{initial.id === 0 ? t("Nuova pianificazione") : t("Modifica la pianificazione")}</h1>

      <div className="field">
        <label htmlFor="schedule-agent">{t("Agente")}</label>
        <select id="schedule-agent" value={schedule.agentId} onChange={(e) => set({ agentId: Number(e.target.value) })}>
          {agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      </div>

      {local && ownModel && (
        <p className="dim note">{t("Usa il modello dell'agente: {name}.", { name: baseName(ownModel) })}</p>
      )}
      {local && !ownModel && (
        <div className="field">
          <label htmlFor="schedule-model">{t("Modello locale da caricare")}</label>
          <select
            id="schedule-model"
            value={schedule.localModel ?? ""}
            onChange={(e) => set({ localModel: e.target.value || undefined })}
          >
            <option value="">{t("Scegli un modello…")}</option>
            {modelChoices.map((m) => (
              <option key={m.path} value={m.path}>
                {m.name}
              </option>
            ))}
          </select>
          <p className="dim note">
            {t("Si carica con il preset salvato per quel modello e si scarica a lavoro finito, se l'ha caricato la pianificazione.")}
          </p>
        </div>
      )}
      {local && (
        <fieldset className="field">
          <legend>{t("Se all'ora stabilita è caricato un altro modello")}</legend>
          <label className="check">
            <input type="radio" checked={schedule.onConflict === "wait"} onChange={() => set({ onConflict: "wait" })} />
            {t("Aspetta che venga scaricato (al massimo un'ora, poi salta)")}
          </label>
          <label className="check">
            <input type="radio" checked={schedule.onConflict === "replace"} onChange={() => set({ onConflict: "replace" })} />
            {t("Sostituiscilo appena nessuno lo sta usando")}
          </label>
        </fieldset>
      )}

      <div className="field">
        <label htmlFor="schedule-task">{t("Compito")}</label>
        <textarea
          id="schedule-task"
          rows={5}
          value={schedule.task}
          placeholder={t("Cosa deve fare ogni volta, scritto per intero: l'agente non ricorda le volte precedenti.")}
          onChange={(e) => set({ task: e.target.value })}
        />
      </div>

      <fieldset className="field">
        <legend>{t("Quando")}</legend>
        <div className="tools-bar">
          <label>
            <input type="radio" checked={rec.kind === "once"} onChange={() => changeKind("once")} />
            {t("Una volta")}
          </label>
          <label>
            <input type="radio" checked={rec.kind === "daily"} onChange={() => changeKind("daily")} />
            {t("Ogni giorno")}
          </label>
          <label>
            <input type="radio" checked={rec.kind === "everyHours"} onChange={() => changeKind("everyHours")} />
            {t("Ogni tot ore")}
          </label>
        </div>
      </fieldset>
      {rec.kind === "once" && (
        <div className="field">
          <label htmlFor="schedule-at">{t("Data e ora")}</label>
          <input id="schedule-at" type="datetime-local" value={rec.at} onChange={(e) => setRec({ at: e.target.value })} />
        </div>
      )}
      {rec.kind === "daily" && (
        <>
          <div className="field">
            <label htmlFor="schedule-time">{t("Ora")}</label>
            <input id="schedule-time" type="time" value={rec.time} onChange={(e) => setRec({ time: e.target.value })} />
          </div>
          <WeekdayPicker value={rec.weekdays} onChange={(weekdays) => setRec({ weekdays })} />
        </>
      )}
      {rec.kind === "everyHours" && (
        <>
          <div className="row">
            <div className="field">
              <label htmlFor="schedule-hours">{t("Ogni quante ore")}</label>
              <input
                id="schedule-hours"
                type="number"
                min={1}
                max={24}
                value={rec.hours}
                onChange={(e) => setRec({ hours: Math.min(24, Math.max(1, Number(e.target.value) || 1)) })}
              />
            </div>
            <div className="field">
              <label htmlFor="schedule-start">{t("A partire dalle")}</label>
              <input id="schedule-start" type="time" value={rec.start} onChange={(e) => setRec({ start: e.target.value })} />
            </div>
          </div>
          <p className="dim note">{t("Ricomincia ogni giorno dall'ora di partenza: ogni 5 ore dalle 08:00 vuol dire 08, 13, 18 e 23.")}</p>
          <WeekdayPicker value={rec.weekdays} onChange={(weekdays) => setRec({ weekdays })} />
        </>
      )}

      <fieldset className="field">
        <legend>{t("Se all'ora stabilita l'app era chiusa o il PC spento")}</legend>
        <label className="check">
          <input type="radio" checked={schedule.onMissed === "catchUp"} onChange={() => set({ onMissed: "catchUp" })} />
          {t("Eseguilo appena possibile, una volta sola")}
        </label>
        <label className="check">
          <input type="radio" checked={schedule.onMissed === "skip"} onChange={() => set({ onMissed: "skip" })} />
          {t("Saltalo")}
        </label>
      </fieldset>

      <div className="row">
        <div className="field">
          <label htmlFor="schedule-notify">{t("Notifica")}</label>
          <select id="schedule-notify" value={schedule.notify} onChange={(e) => set({ notify: e.target.value as NotifyWhen })}>
            {NOTIFY_CHOICES.map((c) => (
              <option key={c.value} value={c.value}>
                {t(c.label)}
              </option>
            ))}
          </select>
        </div>
        <div className="field">
          <label htmlFor="schedule-minutes">{t("Durata massima (minuti)")}</label>
          <input
            id="schedule-minutes"
            type="number"
            min={5}
            max={480}
            value={schedule.maxMinutes}
            onChange={(e) => set({ maxMinutes: Number(e.target.value) || 60 })}
          />
        </div>
      </div>

      {refused.length > 0 && (
        <p className="notice">
          {t("Con i permessi attuali dell'agente queste azioni verranno rifiutate: {list}. Per consentirle, cambia i permessi dell'agente.", {
            list: refused.join(", "),
          })}
        </p>
      )}
      {agent && agent.folders.length > 0 && !agent.restorePoints && (
        <p className="dim note">
          {t("Consiglio: accendi i punti di ripristino nell'agente, così puoi annullare ciò che ha cambiato mentre non c'eri.")}
        </p>
      )}

      <label className="check">
        <input type="checkbox" checked={schedule.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
        {t("Attiva")}
      </label>

      <div className="row">
        <button
          className="btn primary"
          onClick={save}
          disabled={!schedule.task.trim() || (local && !ownModel && !schedule.localModel)}
        >
          {t("Salva")}
        </button>
        <button className="btn" onClick={onCancel}>
          {t("Annulla")}
        </button>
      </div>
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </div>
  );
}
