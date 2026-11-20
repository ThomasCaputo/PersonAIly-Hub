import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  isActive,
  usd,
  type ActionRecord,
  type Agent,
  type AgentSession,
  type Approval,
  type LiveState,
  type McpServer,
  type Message,
  type PermissionLevel,
  type ProviderInfo,
  type RuntimeStatus,
  type SessionStatus,
} from "../api";
import { Markdown } from "../components/Markdown";
import { ToolRequestCard } from "../components/ToolsBar";
import { ModelPicker, ToolResult, toolLabel } from "./ChatArea";
import { locale, msg, t, tc } from "../i18n";

type Props = {
  status: RuntimeStatus | null;
  providers: ProviderInfo[];
  onOpenModels: () => void;
};

type Tab = "sessions" | "agents" | "actions";

const LIST_INTERVAL_MS = 2000;
const LIVE_INTERVAL_MS = 500;

export const STATUS_LABELS: Record<SessionStatus, string> = {
  running: msg("Al lavoro"),
  paused: msg("In pausa"),
  waiting: msg("Aspetta la tua conferma"),
  done: msg("Finito"),
  stopped: msg("Fermato"),
  failed: msg("Errore"),
  interrupted: msg("Interrotto"),
};

const PERMISSIONS: { value: PermissionLevel; label: string; hint: string }[] = [
  {
    value: "ask",
    label: msg("Chiedi sempre"),
    hint: msg("Ogni modifica ai file e ogni comando chiedono la tua conferma"),
  },
  {
    value: "edits",
    label: msg("Consenti modifiche ai file"),
    hint: msg("Crea e modifica file nelle sue cartelle senza chiedere; i comandi chiedono conferma"),
  },
  {
    value: "all",
    label: msg("Consenti tutto"),
    hint: msg("Nessuna conferma, nemmeno per i comandi: un comando può toccare anche file fuori dalle cartelle"),
  },
];

const permissionLabel = (level: PermissionLevel) => {
  const found = PERMISSIONS.find((p) => p.value === level);
  return found ? t(found.label) : level;
};

const baseName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** Data e ora dell'archivio (UTC, "AAAA-MM-GG HH:MM:SS") nell'ora di questo computer. */
function when(stamp: string): string {
  const date = new Date(`${stamp.replace(" ", "T")}Z`);
  return Number.isNaN(date.getTime())
    ? stamp
    : date.toLocaleString(locale(), { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
}

function newAgent(): Agent {
  return {
    id: 0,
    name: "",
    description: "",
    instructions: "",
    model: { kind: "local" },
    folders: [],
    tools: { files: true, web: false, terminal: true, permission: "ask" },
    mcpServers: [],
    maxSteps: 50,
    maxStepTokens: 8192,
    restorePoints: true,
    delegates: [],
  };
}

export function AgentsArea({ status, providers, onOpenModels }: Props) {
  const [tab, setTab] = useState<Tab>("sessions");
  const [agents, setAgents] = useState<Agent[]>([]);
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  /** Chi ha affidato una sessione: il nome del suo agente, o il numero. */
  const parentName = (id: number) =>
    sessions.find((s) => s.id === id)?.agentName ?? t("sessione {n}", { n: id });
  const [selectedSession, setSelectedSession] = useState<number | "new" | null>(null);
  const [preselected, setPreselected] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refreshAgents = useCallback(() => {
    api.listAgents().then(setAgents).catch((e) => setError(String(e)));
  }, []);
  const refreshSessions = useCallback(() => {
    api.listAgentSessions().then(setSessions).catch(() => {});
  }, []);

  useEffect(() => {
    refreshAgents();
    refreshSessions();
    const timer = window.setInterval(refreshSessions, LIST_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [refreshAgents, refreshSessions]);

  // Alla prima apertura si mostra la sessione più recente.
  useEffect(() => {
    if (selectedSession === null && sessions.length > 0) setSelectedSession(sessions[0].id);
  }, [sessions, selectedSession]);

  const started = (session: AgentSession) => {
    refreshSessions();
    setSelectedSession(session.id);
  };

  const running = sessions.filter((s) => isActive(s.status)).length;

  return (
    <div className="agents">
      <div className="tabs" role="tablist">
        <button role="tab" aria-selected={tab === "sessions"} className={tab === "sessions" ? "tab active" : "tab"} onClick={() => setTab("sessions")}>
          {t("Sessioni")}
          {running > 0 && ` (${t("{n} al lavoro", { n: running })})`}
        </button>
        <button role="tab" aria-selected={tab === "agents"} className={tab === "agents" ? "tab active" : "tab"} onClick={() => setTab("agents")}>
          {t("Agenti")}
        </button>
        <button role="tab" aria-selected={tab === "actions"} className={tab === "actions" ? "tab active" : "tab"} onClick={() => setTab("actions")}>
          {t("Registro azioni")}
        </button>
      </div>
      {error && <p className="error pre" role="alert">{tc(error)}</p>}

      {tab === "sessions" && (
        <div className="agents-split">
          <aside className="conversations">
            <button className="btn primary wide" onClick={() => setSelectedSession("new")}>
              {t("Nuovo compito")}
            </button>
            <div className="conversation-list">
              {sessions.length === 0 && (
                <p className="dim note">{t("Nessuna sessione: affida un compito a un agente.")}</p>
              )}
              {sessions.map((s) => (
                <button
                  key={s.id}
                  aria-label={
                    s.parentSessionId != null
                      ? t("{agent}, lavoro affidato da {parent}", { agent: s.agentName, parent: parentName(s.parentSessionId) })
                      : undefined
                  }
                  className={s.id === selectedSession ? "session-item active" : "session-item"}
                  onClick={() => setSelectedSession(s.id)}
                  title={s.task}
                >
                  <span className="session-item-head">
                    <span className="session-agent">{s.agentName}</span>
                    <StatusBadge status={s.status} />
                  </span>
                  <span className="session-task">{s.task}</span>
                  <span className="dim session-when">
                    {s.parentSessionId != null && `↳ ${parentName(s.parentSessionId)} · `}
                    {when(s.updatedAt)}
                  </span>
                </button>
              ))}
            </div>
          </aside>
          <section className="thread">
            {selectedSession === "new" ? (
              <NewTask
                key={preselected ?? "none"}
                agents={agents}
                initialAgent={preselected}
                onStarted={started}
                onCreateAgent={() => setTab("agents")}
              />
            ) : selectedSession !== null ? (
              <SessionView
                key={selectedSession}
                id={selectedSession}
                onChanged={refreshSessions}
                onDeleted={() => {
                  setSelectedSession(null);
                  refreshSessions();
                }}
              />
            ) : (
              <div className="empty">
                <p className="dim">{t("Scegli una sessione o affida un nuovo compito.")}</p>
              </div>
            )}
          </section>
        </div>
      )}

      {tab === "agents" && (
        <AgentsTab
          agents={agents}
          providers={providers}
          status={status}
          onChanged={refreshAgents}
          onOpenModels={onOpenModels}
          onStart={(agentId) => {
            setPreselected(agentId);
            setTab("sessions");
            setSelectedSession("new");
          }}
        />
      )}

      {tab === "actions" && <ActionsTab />}
    </div>
  );
}

function StatusBadge({ status }: { status: SessionStatus }) {
  return <span className={`status-badge ${status}`}>{t(STATUS_LABELS[status])}</span>;
}

// ── Nuovo compito ─────────────────────────────────────────────────────────

function NewTask({
  agents,
  initialAgent,
  onStarted,
  onCreateAgent,
}: {
  agents: Agent[];
  initialAgent: number | null;
  onStarted: (session: AgentSession) => void;
  onCreateAgent: () => void;
}) {
  const [agentId, setAgentId] = useState<number | null>(initialAgent ?? agents[0]?.id ?? null);
  const [task, setTask] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (agentId === null && agents.length > 0) setAgentId(agents[0].id);
  }, [agents, agentId]);

  if (agents.length === 0) {
    return (
      <div className="empty">
        <p className="dim">{t("Non c'è ancora nessun agente.")}</p>
        <button className="btn primary" onClick={onCreateAgent}>
          {t("Crea un agente")}
        </button>
      </div>
    );
  }

  const agent = agents.find((a) => a.id === agentId);
  const start = async () => {
    if (agentId === null || !task.trim()) return;
    setBusy(true);
    setError(null);
    try {
      onStarted(await api.startAgentSession(agentId, task));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page new-task">
      <h1>{t("Nuovo compito")}</h1>
      <div className="field">
        <label htmlFor="task-agent">{t("Agente")}</label>
        <select id="task-agent" value={agentId ?? ""} onChange={(e) => setAgentId(Number(e.target.value))}>
          {agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
        {agent?.description && <p className="dim note">{agent.description}</p>}
        {agent && (
          <p className="dim note">
            {t("Lavora in: {folders}", {
              folders: agent.folders.length > 0 ? agent.folders.map(baseName).join(", ") : t("nessuna cartella"),
            })}{" "}
            · {permissionLabel(agent.tools.permission)}
          </p>
        )}
      </div>
      <div className="field">
        <label htmlFor="task-text">{t("Compito")}</label>
        <textarea
          id="task-text"
          rows={6}
          value={task}
          placeholder={t(
            "Descrivi cosa deve fare l'agente, per esempio: «Aggiungi i test alla funzione somma in calcoli.py e falli passare».",
          )}
          onChange={(e) => setTask(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && e.ctrlKey) start();
          }}
        />
      </div>
      <button className="btn primary" disabled={busy || !task.trim() || agentId === null} onClick={start}>
        {t("Avvia")}
      </button>
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </div>
  );
}

// ── Una sessione ──────────────────────────────────────────────────────────

/** File creati o modificati dall'agente, ricavati dalle chiamate agli strumenti. */
function touchedFiles(messages: Message[]): string[] {
  const results = new Map(messages.filter((m) => m.role === "tool").map((m) => [m.toolCallId, m]));
  const paths: string[] = [];
  for (const m of messages) {
    for (const call of m.toolCalls) {
      if (call.name !== "write_file" && call.name !== "edit_file") continue;
      const result = results.get(call.id);
      const path = (call.arguments as { path?: unknown })?.path;
      if (result && !result.isError && typeof path === "string" && !paths.includes(path)) paths.push(path);
    }
  }
  return paths;
}

function SessionView({ id, onChanged, onDeleted }: { id: number; onChanged: () => void; onDeleted: () => void }) {
  const [session, setSession] = useState<AgentSession | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [live, setLive] = useState<LiveState | null>(null);
  const [reply, setReply] = useState("");
  const [error, setError] = useState<string | null>(null);
  // Le cartelle sono appena state riportate a prima della sessione.
  const [restored, setRestored] = useState(false);
  const seen = useRef<{ revision: number; status: SessionStatus | null } | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  const reloadMessages = useCallback(async (conversationId: number) => {
    setMessages(await api.getMessages(conversationId));
  }, []);

  const poll = useCallback(async () => {
    try {
      const [current, state] = await Promise.all([api.getAgentSession(id), api.agentSessionLive(id)]);
      setSession(current);
      setLive(state);
      const mark = { revision: state?.revision ?? -1, status: current.status };
      const last = seen.current;
      // Si rilegge l'archivio solo quando c'è qualcosa di nuovo.
      if (!last || last.revision !== mark.revision || last.status !== mark.status) {
        seen.current = mark;
        await reloadMessages(current.conversationId);
        if (last && last.status !== mark.status) onChanged();
      }
    } catch (e) {
      setError(String(e));
    }
  }, [id, reloadMessages, onChanged]);

  useEffect(() => {
    poll();
    const timer = window.setInterval(poll, LIVE_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [poll]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [messages, live?.streaming, live?.request]);

  if (!session) return error ? <p className="error pre" role="alert">{tc(error)}</p> : null;

  const active = isActive(session.status);
  const act = (action: () => Promise<unknown>) => {
    setError(null);
    action()
      .then(poll)
      .catch((e) => setError(String(e)));
  };
  const answer = (approval: Approval) => {
    if (live?.request) act(() => api.answerToolRequest(live.request!.id, approval));
  };
  const sendReply = () => {
    const text = reply.trim();
    if (!text || active) return;
    setReply("");
    act(async () => {
      await api.replyAgentSession(id, text);
      onChanged();
    });
  };
  const remove = () =>
    act(async () => {
      await api.deleteAgentSession(id);
      onDeleted();
    });
  const restore = () => {
    if (!session) return;
    const folders = session.restorePoints.map((p) => p.folder).join("\n");
    const question = t(
      "Riportare queste cartelle a com'erano prima della sessione?\n\n{folders}\n\nI file cambiati tornano indietro e quelli creati dopo spariscono. Prima viene salvato lo stato attuale: ripetendo il comando si annulla il ripristino.",
      { folders },
    );
    if (!window.confirm(question)) return;
    act(async () => {
      setSession(await api.restoreAgentSession(id));
      setRestored(true);
    });
  };

  const files = touchedFiles(messages);
  const cost = messages.reduce((sum, m) => sum + (m.costUsd ?? 0), 0);
  const steps = messages.filter((m) => m.role === "assistant").length;
  const shown = messages.filter((m) => !(m.role === "assistant" && !m.content && m.toolCalls.length > 0));

  return (
    <>
      <div className="session-head">
        <div>
          <div className="session-title">
            {session.agentName} <StatusBadge status={session.status} />
          </div>
          <div className="dim">
            {t("Avviata {when}", { when: when(session.createdAt) })} ·{" "}
            {steps === 1 ? t("1 passo") : t("{n} passi", { n: steps })}
            {cost > 0 && ` · ${t("circa {cost}", { cost: usd(cost) })}`}
          </div>
        </div>
        <div className="row">
          {session.status === "paused" ? (
            <button className="btn" onClick={() => act(() => api.resumeAgentSession(id))}>
              {t("Riprendi")}
            </button>
          ) : (
            active && (
              <button
                className="btn"
                title={t("Si ferma prima del passo successivo")}
                onClick={() => act(() => api.pauseAgentSession(id))}
              >
                {t("Pausa")}
              </button>
            )
          )}
          {active && (
            <button className="btn" onClick={() => act(() => api.stopAgentSession(id))}>
              {t("Ferma")}
            </button>
          )}
          {!active && session.restorePoints.length > 0 && (
            <button
              className="btn"
              title={
                restored
                  ? t("Rimette le cartelle com'erano prima del ripristino")
                  : t("Riporta le cartelle a com'erano prima della sessione")
              }
              onClick={restore}
            >
              {restored ? t("Rifai le modifiche") : t("Annulla le modifiche")}
            </button>
          )}
          {!active && (
            <button className="btn" onClick={remove}>
              {t("Elimina")}
            </button>
          )}
        </div>
      </div>

      {files.length > 0 && (
        <div className="touched">
          <span className="dim">{t("File toccati:")}</span>
          {files.map((f) => (
            <span key={f} className="attachment mono" title={f}>
              {baseName(f)}
            </span>
          ))}
        </div>
      )}

      <div className="messages" role="log" aria-label={t("Passi dell'agente")} aria-live="polite">
        {shown.map((m, index) =>
          m.role === "tool" ? (
            <ToolResult key={m.id} message={m} />
          ) : (
            <div key={m.id} className={`message ${m.role}`}>
              <div className="message-role">
                {m.role === "user" ? (index === 0 ? t("Compito") : t("Tu")) : (m.model ?? session.agentName)}
              </div>
              {m.role === "user" ? <div className="message-text">{m.content}</div> : <Markdown text={m.content} />}
            </div>
          ),
        )}
        {live && (live.streaming || (!live.request && session.status === "running")) && (
          <div className="message assistant">
            <div className="message-role">{session.agentName}</div>
            {live.reasoning && !live.streaming && <div className="reasoning">{live.reasoning}</div>}
            {live.streaming ? (
              <Markdown text={live.streaming} streaming />
            ) : (
              !live.reasoning && <div className="dim">{t("Sto lavorando…")}</div>
            )}
          </div>
        )}
        {session.status === "paused" && (
          <p className="notice">{t("In pausa: l'agente riprende dal passo successivo quando premi «Riprendi».")}</p>
        )}
        {live?.request && <ToolRequestCard request={live.request} onAnswer={answer} kind="agent" />}
        {session.note && !active && (
          <p className={session.status === "failed" ? "error pre" : "notice"}>{tc(session.note)}</p>
        )}
        {session.status === "interrupted" && (
          <p className="notice">{t("L'app è stata chiusa mentre l'agente lavorava. Scrivigli per farlo continuare.")}</p>
        )}
        {error && <p className="error pre" role="alert">{tc(error)}</p>}
        <div ref={bottomRef} />
      </div>

      <div className="composer">
        <textarea
          value={reply}
          aria-label={t("Messaggio all'agente")}
          rows={2}
          disabled={active}
          placeholder={
            active
              ? t("L'agente sta lavorando: puoi scrivergli quando si ferma.")
              : t("Rispondi all'agente o dagli un nuovo compito…")
          }
          title={t("Invio per inviare, Maiusc+Invio per andare a capo")}
          onChange={(e) => setReply(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              sendReply();
            }
          }}
        />
        <button className="btn primary" disabled={active || !reply.trim()} onClick={sendReply}>
          {t("Invia")}
        </button>
      </div>
    </>
  );
}

// ── Definizioni degli agenti ──────────────────────────────────────────────

function AgentsTab({
  agents,
  providers,
  status,
  onChanged,
  onOpenModels,
  onStart,
}: {
  agents: Agent[];
  providers: ProviderInfo[];
  status: RuntimeStatus | null;
  onChanged: () => void;
  onOpenModels: () => void;
  onStart: (agentId: number) => void;
}) {
  const [editing, setEditing] = useState<Agent | null>(null);
  const [error, setError] = useState<string | null>(null);

  const act = (action: () => Promise<unknown>) => {
    setError(null);
    action()
      .then(onChanged)
      .catch((e) => setError(String(e)));
  };

  if (editing) {
    return (
      <AgentEditor
        initial={editing}
        providers={providers}
        status={status}
        onOpenModels={onOpenModels}
        onCancel={() => setEditing(null)}
        onSaved={() => {
          setEditing(null);
          onChanged();
        }}
      />
    );
  }

  return (
    <div className="page">
      <div className="section-head">
        <h1>{t("Agenti")}</h1>
        <button className="btn primary" onClick={() => setEditing(newAgent())}>
          {t("Nuovo agente")}
        </button>
      </div>
      <p className="dim note">
        {t(
          "Un agente è un modello con istruzioni, cartelle di lavoro e permessi. Gli affidi un compito e lavora da solo, passo dopo passo, chiedendoti conferma quando serve.",
        )}
      </p>
      {agents.length === 0 && <p className="dim">{t("Nessun agente: creane uno per iniziare.")}</p>}
      {agents.map((a) => (
        <div key={a.id} className="card">
          <div className="card-head">
            <div>
              <div className="card-title">{a.name}</div>
              {a.description && <div className="dim">{a.description}</div>}
            </div>
            <div className="row">
              <button className="btn primary" onClick={() => onStart(a.id)}>
                {t("Affida un compito")}
              </button>
              <button className="btn" onClick={() => setEditing(a)}>
                {t("Modifica")}
              </button>
              <button className="btn" onClick={() => act(() => api.duplicateAgent(a.id))}>
                {t("Duplica")}
              </button>
              <button
                className="btn"
                onClick={() => {
                  if (window.confirm(t("Eliminare l'agente «{name}»? Le sue sessioni restano.", { name: a.name }))) {
                    act(() => api.deleteAgent(a.id));
                  }
                }}
              >
                {t("Elimina")}
              </button>
            </div>
          </div>
          <div className="card-body dim">
            {modelLabel(a, providers)} ·{" "}
            {a.folders.length > 0 ? a.folders.map(baseName).join(", ") : t("nessuna cartella")} ·{" "}
            {[
              a.tools.files && t("file"),
              a.tools.terminal && t("terminale"),
              a.tools.web && t("web"),
              a.mcpServers.length > 0 && t("{n} server MCP", { n: a.mcpServers.length }),
            ]
              .filter(Boolean)
              .join(", ") || t("nessuno strumento")}{" "}
            · {permissionLabel(a.tools.permission)}
          </div>
        </div>
      ))}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </div>
  );
}

function modelLabel(agent: Agent, providers: ProviderInfo[]): string {
  if (agent.model.kind === "local") return t("Modello locale caricato");
  const { providerId, model } = agent.model;
  const provider = providers.find((p) => p.id === providerId);
  return provider?.models.find((m) => m.id === model)?.name ?? model;
}

function AgentEditor({
  initial,
  providers,
  status,
  onOpenModels,
  onCancel,
  onSaved,
}: {
  initial: Agent;
  providers: ProviderInfo[];
  status: RuntimeStatus | null;
  onOpenModels: () => void;
  onCancel: () => void;
  onSaved: () => void;
}) {
  const [agent, setAgent] = useState<Agent>(initial);
  const [error, setError] = useState<string | null>(null);
  const [gitMissing, setGitMissing] = useState(false);
  useEffect(() => {
    api
      .restorePointsAvailable()
      .then((ok) => setGitMissing(!ok))
      .catch(() => {});
  }, []);
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  useEffect(() => {
    api.listMcpServers().then(setMcpServers).catch(() => {});
  }, []);
  // Gli altri agenti, che questo può scegliere come aiutanti (AG-8).
  const [otherAgents, setOtherAgents] = useState<Agent[]>([]);
  useEffect(() => {
    api
      .listAgents()
      .then((all) => setOtherAgents(all.filter((a) => a.id !== initial.id)))
      .catch(() => {});
  }, [initial.id]);
  const toggleMcp = (id: string, on: boolean) =>
    set({ mcpServers: on ? [...agent.mcpServers, id] : agent.mcpServers.filter((s) => s !== id) });
  const missingMcp = agent.mcpServers.filter((id) => !mcpServers.some((s) => s.id === id));
  const set = (change: Partial<Agent>) => setAgent((a) => ({ ...a, ...change }));
  const setTools = (change: Partial<Agent["tools"]>) => setAgent((a) => ({ ...a, tools: { ...a.tools, ...change } }));

  const addFolder = async () => {
    const picked = await open({ directory: true, title: t("Scegli una cartella su cui l'agente può lavorare") });
    if (typeof picked === "string" && !agent.folders.includes(picked)) set({ folders: [...agent.folders, picked] });
  };

  const save = async () => {
    setError(null);
    try {
      await api.saveAgent(agent);
      onSaved();
    } catch (e) {
      setError(String(e));
    }
  };

  const needsFolder = (agent.tools.files || agent.tools.terminal) && agent.folders.length === 0;

  return (
    <div className="page agent-editor">
      <h1>{initial.id === 0 ? t("Nuovo agente") : t("Modifica «{name}»", { name: initial.name })}</h1>

      <div className="field">
        <label htmlFor="agent-name">{t("Nome")}</label>
        <input id="agent-name" type="text" value={agent.name} onChange={(e) => set({ name: e.target.value })} />
      </div>
      <div className="field">
        <label htmlFor="agent-description">{t("Descrizione")}</label>
        <input
          id="agent-description"
          type="text"
          value={agent.description}
          placeholder={t("A cosa serve, in una riga")}
          onChange={(e) => set({ description: e.target.value })}
        />
      </div>

      <ModelPicker
        providers={providers}
        loadedName={status?.loaded?.modelName ?? null}
        value={agent.model}
        disabled={false}
        onChange={(model) => set({ model })}
      />
      {agent.model.kind === "local" && (
        <p className="dim note">
          {t("Usa il modello locale caricato quando parte la sessione.")}{" "}
          <button className="btn-link" onClick={onOpenModels}>
            {t("Vai ai modelli")}
          </button>
        </p>
      )}

      <div className="field">
        <label htmlFor="agent-instructions">{t("Istruzioni")}</label>
        <textarea
          id="agent-instructions"
          rows={6}
          value={agent.instructions}
          placeholder={t("Come deve lavorare: stile del codice, comandi per provare, cosa evitare…")}
          onChange={(e) => set({ instructions: e.target.value })}
        />
      </div>

      <section className="section">
        <div className="section-head">
          <h2>{t("Cartelle di lavoro")}</h2>
          <button className="btn" onClick={addFolder}>
            {t("Aggiungi cartella")}
          </button>
        </div>
        <p className="dim note">
          {t("L'agente legge e scrive file solo qui dentro; i comandi partono dalla prima cartella.")}
        </p>
        {agent.folders.length === 0 && <p className="dim">{t("Nessuna cartella.")}</p>}
        {agent.folders.map((folder) => (
          <div key={folder} className="folder">
            <span className="mono">{folder}</span>
            <button className="btn-link" onClick={() => set({ folders: agent.folders.filter((f) => f !== folder) })}>
              {t("Togli")}
            </button>
          </div>
        ))}
        {needsFolder && <p className="notice">{t("Senza una cartella l'agente non può usare file e terminale.")}</p>}
      </section>

      <section className="section">
        <h2>{t("Strumenti e permessi")}</h2>
        <div className="tools-bar">
          <label>
            <input type="checkbox" checked={agent.tools.files} onChange={(e) => setTools({ files: e.target.checked })} />
            {t("File")}
          </label>
          <label title={t("Comandi PowerShell nella cartella di lavoro")}>
            <input
              type="checkbox"
              checked={agent.tools.terminal}
              onChange={(e) => setTools({ terminal: e.target.checked })}
            />
            {t("Terminale")}
          </label>
          <label title={t("Le parole cercate vengono inviate a DuckDuckGo")}>
            <input type="checkbox" checked={agent.tools.web} onChange={(e) => setTools({ web: e.target.checked })} />
            {t("Ricerca web")}
          </label>
          <label title={t("Cerca nei documenti resi consultabili in Impostazioni")}>
            <input
              type="checkbox"
              checked={agent.tools.documents ?? false}
              onChange={(e) => setTools({ documents: e.target.checked })}
            />
            {t("Documenti")}
          </label>
          <select
            aria-label={t("Permessi")}
            value={agent.tools.permission}
            onChange={(e) => setTools({ permission: e.target.value as PermissionLevel })}
          >
            {PERMISSIONS.map((p) => (
              <option key={p.value} value={p.value}>
                {t(p.label)}
              </option>
            ))}
          </select>
        </div>
        <p className="dim note">{t(PERMISSIONS.find((p) => p.value === agent.tools.permission)?.hint ?? "")}</p>
        {agent.tools.terminal && agent.tools.permission === "all" && (
          <p className="notice">
            {t(
              "Attenzione: con «Consenti tutto» i comandi partono senza chiederti nulla, e un comando può agire anche fuori dalle cartelle di lavoro.",
            )}
          </p>
        )}
      </section>

      <section className="section">
        <h2>{t("Server MCP")}</h2>
        {mcpServers.length === 0 ? (
          <p className="dim note">{t("Nessun server MCP configurato: si aggiungono nelle Impostazioni.")}</p>
        ) : (
          <>
            <div className="tools-bar">
              {mcpServers.map((s) => (
                <label key={s.id}>
                  <input
                    type="checkbox"
                    checked={agent.mcpServers.includes(s.id)}
                    onChange={(e) => toggleMcp(s.id, e.target.checked)}
                  />
                  {s.name}
                </label>
              ))}
            </div>
            <p className="dim note">
              {t(
                "Partono con ogni sessione e si chiudono con lei. I loro strumenti chiedono conferma salvo con «Consenti tutto».",
              )}
            </p>
          </>
        )}
        {missingMcp.length > 0 && (
          <p className="notice">
            {missingMcp.length === 1 ? t("Un server scelto non esiste più.") : t("Alcuni server scelti non esistono più.")}{" "}
            <button className="btn-link" onClick={() => set({ mcpServers: agent.mcpServers.filter((id) => !missingMcp.includes(id)) })}>
              {t("Togli")}
            </button>
          </p>
        )}
      </section>

      <section className="section">
        <h2>{t("Aiutanti")}</h2>
        {otherAgents.length === 0 ? (
          <p className="dim note">{t("Crea un altro agente per poterlo scegliere come aiutante.")}</p>
        ) : (
          <>
            <div className="tools-bar">
              {otherAgents.map((a) => (
                <label key={a.id}>
                  <input
                    type="checkbox"
                    checked={agent.delegates.includes(a.id)}
                    onChange={(e) =>
                      set({
                        delegates: e.target.checked
                          ? [...agent.delegates, a.id]
                          : agent.delegates.filter((id) => id !== a.id),
                      })
                    }
                  />
                  {a.name}
                </label>
              ))}
            </div>
            <p className="dim note">
              {t(
                "L'agente può affidare a loro parti del compito: ognuno lavora in una sessione sua, con le sue cartelle e i suoi permessi, e gli restituisce un resoconto. Affidare lavoro chiede conferma salvo con «Consenti tutto». Al massimo due livelli di deleghe.",
              )}
            </p>
          </>
        )}
      </section>

      <div className="field">
        <label htmlFor="agent-steps">{t("Passi massimi per volta")}</label>
        <input
          id="agent-steps"
          type="number"
          min={1}
          max={500}
          value={agent.maxSteps}
          onChange={(e) => set({ maxSteps: Number(e.target.value) || 1 })}
        />
        <p className="dim note">{t("Arrivato qui l'agente si ferma e aspetta che tu gli risponda.")}</p>
      </div>

      <div className="field">
        <label htmlFor="agent-step-tokens">{t("Token massimi per passo")}</label>
        <input
          id="agent-step-tokens"
          type="number"
          min={256}
          max={131072}
          step={256}
          value={agent.maxStepTokens}
          onChange={(e) => set({ maxStepTokens: Number(e.target.value) || 8192 })}
        />
        <p className="dim note">
          {t(
            "Un passo più lungo viene interrotto e l'agente riprova in breve: evita che un modello locale resti incastrato per minuti.",
          )}
        </p>
      </div>

      <label className="check">
        <input
          type="checkbox"
          checked={agent.restorePoints}
          onChange={(e) => set({ restorePoints: e.target.checked })}
        />
        <span>
          <strong>{t("Punto di ripristino prima di ogni sessione")}</strong>
          <span className="dim note block">
            {gitMissing
              ? t("Serve git, che su questo computer non c'è: installalo per usare questa opzione.")
              : t(
                  "Fotografa le cartelle prima che l'agente lavori, così puoi annullare tutto con un clic. Il tuo repository git, se c'è, non viene toccato; i file ignorati da .gitignore e le cartelle come node_modules restano fuori.",
                )}
          </span>
        </span>
      </label>

      <div className="row">
        <button className="btn primary" onClick={save} disabled={!agent.name.trim()}>
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

// ── Registro delle azioni ─────────────────────────────────────────────────

const APPROVAL_LABELS: Record<ActionRecord["approval"], string> = {
  auto: msg("consentita dai permessi"),
  user: msg("confermata da te"),
  denied: msg("negata da te"),
  refused: msg("rifiutata dall'app"),
  interrupted: msg("interrotta"),
};

/** Durate proposte per il registro, in giorni (0 = per sempre). */
const RETENTION_CHOICES = [7, 30, 90, 365, 0];

function retentionLabel(days: number): string {
  if (days === 0) return t("per sempre");
  if (days === 365) return t("per un anno");
  return t("per {n} giorni", { n: days });
}

/** Per quanto tempo si tengono le voci del registro, e pulsante per svuotarlo. */
function ActionLogRetention({
  empty,
  onChanged,
  onError,
}: {
  empty: boolean;
  onChanged: () => void;
  onError: (message: string) => void;
}) {
  const [days, setDays] = useState<number | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    api.actionLogDays().then(setDays).catch((e) => onError(String(e)));
  }, [onError]);

  const choose = async (next: number) => {
    setDays(next);
    try {
      const removed = await api.setActionLogDays(next);
      setMessage(removed > 0 ? t("Eliminate {n} voci più vecchie.", { n: removed }) : null);
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  };

  const clear = async () => {
    if (!window.confirm(t("Eliminare tutte le voci del registro? Non si possono recuperare."))) return;
    try {
      const removed = await api.clearActions();
      setMessage(t("Eliminate {n} voci.", { n: removed }));
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  };

  // Un valore scritto a mano nell'archivio resta tra le scelte.
  const choices = days !== null && !RETENTION_CHOICES.includes(days) ? [...RETENTION_CHOICES, days] : RETENTION_CHOICES;

  return (
    <div className="retention">
      <label htmlFor="log-days">{t("Conserva le voci")}</label>
      <select id="log-days" value={days ?? ""} disabled={days === null} onChange={(e) => choose(Number(e.target.value))}>
        {choices.map((d) => (
          <option key={d} value={d}>
            {retentionLabel(d)}
          </option>
        ))}
      </select>
      <button className="btn-link" disabled={empty} onClick={clear}>
        {t("Svuota registro")}
      </button>
      {message && <span className="dim">{message}</span>}
    </div>
  );
}

function ActionsTab() {
  const [actions, setActions] = useState<ActionRecord[]>([]);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    api.listActions(300).then(setActions).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    reload();
    const timer = window.setInterval(reload, LIST_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [reload]);

  return (
    <div className="page">
      <h1>{t("Registro azioni")}</h1>
      <p className="dim note">
        {t(
          "Ogni strumento usato dai modelli, in chat e dagli agenti, dalla più recente. Le voci restano anche se elimini la conversazione o la sessione.",
        )}
      </p>
      <ActionLogRetention empty={actions.length === 0} onChanged={reload} onError={setError} />
      {actions.length === 0 && <p className="dim">{t("Ancora nessuna azione.")}</p>}
      {actions.length > 0 && (
        <table className="actions-table">
          <thead>
            <tr>
              <th>{t("Quando")}</th>
              <th>{t("Dove")}</th>
              <th>{t("Azione")}</th>
              <th>{t("Esito")}</th>
            </tr>
          </thead>
          <tbody>
            {actions.map((a) => (
              <tr key={a.id} className={a.isError ? "failed" : undefined}>
                <td className="nowrap">{when(a.createdAt)}</td>
                <td>
                  <span className="dim">{a.kind === "agent" ? t("Agente") : t("Chat")}</span> {tc(a.source)}
                </td>
                <td>
                  <div>{toolLabel(a.tool)}</div>
                  <div className="dim mono action-summary">{tc(a.summary)}</div>
                </td>
                <td title={a.result}>
                  {a.isError ? t("Errore") : t("Riuscita")} ·{" "}
                  <span className="dim">{t(APPROVAL_LABELS[a.approval])}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}

    </div>
  );
}
