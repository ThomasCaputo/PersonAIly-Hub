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

type Props = {
  status: RuntimeStatus | null;
  providers: ProviderInfo[];
  onOpenModels: () => void;
};

type Tab = "sessions" | "agents" | "actions";

const LIST_INTERVAL_MS = 2000;
const LIVE_INTERVAL_MS = 500;

export const STATUS_LABELS: Record<SessionStatus, string> = {
  running: "Al lavoro",
  paused: "In pausa",
  waiting: "Aspetta la tua conferma",
  done: "Finito",
  stopped: "Fermato",
  failed: "Errore",
  interrupted: "Interrotto",
};

const PERMISSIONS: { value: PermissionLevel; label: string; hint: string }[] = [
  { value: "ask", label: "Chiedi sempre", hint: "Ogni modifica ai file e ogni comando chiedono la tua conferma" },
  {
    value: "edits",
    label: "Consenti modifiche ai file",
    hint: "Crea e modifica file nelle sue cartelle senza chiedere; i comandi chiedono conferma",
  },
  {
    value: "all",
    label: "Consenti tutto",
    hint: "Nessuna conferma, nemmeno per i comandi: un comando può toccare anche file fuori dalle cartelle",
  },
];

const baseName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** Data e ora dell'archivio (UTC, "AAAA-MM-GG HH:MM:SS") nell'ora di questo computer. */
function when(stamp: string): string {
  const date = new Date(`${stamp.replace(" ", "T")}Z`);
  return Number.isNaN(date.getTime())
    ? stamp
    : date.toLocaleString("it-IT", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
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
  };
}

export function AgentsArea({ status, providers, onOpenModels }: Props) {
  const [tab, setTab] = useState<Tab>("sessions");
  const [agents, setAgents] = useState<Agent[]>([]);
  const [sessions, setSessions] = useState<AgentSession[]>([]);
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
          Sessioni{running > 0 && ` (${running} al lavoro)`}
        </button>
        <button role="tab" aria-selected={tab === "agents"} className={tab === "agents" ? "tab active" : "tab"} onClick={() => setTab("agents")}>
          Agenti
        </button>
        <button role="tab" aria-selected={tab === "actions"} className={tab === "actions" ? "tab active" : "tab"} onClick={() => setTab("actions")}>
          Registro azioni
        </button>
      </div>
      {error && <p className="error pre">{error}</p>}

      {tab === "sessions" && (
        <div className="agents-split">
          <aside className="conversations">
            <button className="btn primary wide" onClick={() => setSelectedSession("new")}>
              Nuovo compito
            </button>
            <div className="conversation-list">
              {sessions.length === 0 && <p className="dim note">Nessuna sessione: affida un compito a un agente.</p>}
              {sessions.map((s) => (
                <button
                  key={s.id}
                  className={s.id === selectedSession ? "session-item active" : "session-item"}
                  onClick={() => setSelectedSession(s.id)}
                  title={s.task}
                >
                  <span className="session-item-head">
                    <span className="session-agent">{s.agentName}</span>
                    <StatusBadge status={s.status} />
                  </span>
                  <span className="session-task">{s.task}</span>
                  <span className="dim session-when">{when(s.updatedAt)}</span>
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
                <p className="dim">Scegli una sessione o affida un nuovo compito.</p>
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
  return <span className={`status-badge ${status}`}>{STATUS_LABELS[status]}</span>;
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
        <p className="dim">Non c'è ancora nessun agente.</p>
        <button className="btn primary" onClick={onCreateAgent}>
          Crea un agente
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
      <h1>Nuovo compito</h1>
      <div className="field">
        <label htmlFor="task-agent">Agente</label>
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
            Lavora in: {agent.folders.length > 0 ? agent.folders.map(baseName).join(", ") : "nessuna cartella"} ·{" "}
            {PERMISSIONS.find((p) => p.value === agent.tools.permission)?.label}
          </p>
        )}
      </div>
      <div className="field">
        <label htmlFor="task-text">Compito</label>
        <textarea
          id="task-text"
          rows={6}
          value={task}
          placeholder="Descrivi cosa deve fare l'agente, per esempio: «Aggiungi i test alla funzione somma in calcoli.py e falli passare»."
          onChange={(e) => setTask(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && e.ctrlKey) start();
          }}
        />
      </div>
      <button className="btn primary" disabled={busy || !task.trim() || agentId === null} onClick={start}>
        Avvia
      </button>
      {error && <p className="error pre">{error}</p>}
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

  if (!session) return error ? <p className="error pre">{error}</p> : null;

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
            Avviata {when(session.createdAt)} · {steps} {steps === 1 ? "passo" : "passi"}
            {cost > 0 && ` · circa ${usd(cost)}`}
          </div>
        </div>
        <div className="row">
          {session.status === "paused" ? (
            <button className="btn" onClick={() => act(() => api.resumeAgentSession(id))}>
              Riprendi
            </button>
          ) : (
            active && (
              <button
                className="btn"
                title="Si ferma prima del passo successivo"
                onClick={() => act(() => api.pauseAgentSession(id))}
              >
                Pausa
              </button>
            )
          )}
          {active && (
            <button className="btn" onClick={() => act(() => api.stopAgentSession(id))}>
              Ferma
            </button>
          )}
          {!active && (
            <button className="btn" onClick={remove}>
              Elimina
            </button>
          )}
        </div>
      </div>

      {files.length > 0 && (
        <div className="touched">
          <span className="dim">File toccati:</span>
          {files.map((f) => (
            <span key={f} className="attachment mono" title={f}>
              {baseName(f)}
            </span>
          ))}
        </div>
      )}

      <div className="messages">
        {shown.map((m, index) =>
          m.role === "tool" ? (
            <ToolResult key={m.id} message={m} />
          ) : (
            <div key={m.id} className={`message ${m.role}`}>
              <div className="message-role">
                {m.role === "user" ? (index === 0 ? "Compito" : "Tu") : (m.model ?? session.agentName)}
              </div>
              {m.role === "user" ? <div className="message-text">{m.content}</div> : <Markdown text={m.content} />}
            </div>
          ),
        )}
        {live && (live.streaming || (!live.request && session.status === "running")) && (
          <div className="message assistant">
            <div className="message-role">{session.agentName}</div>
            {live.reasoning && !live.streaming && <div className="reasoning">{live.reasoning}</div>}
            {live.streaming ? <Markdown text={live.streaming} /> : !live.reasoning && <div className="dim">Sto lavorando…</div>}
          </div>
        )}
        {session.status === "paused" && (
          <p className="notice">In pausa: l'agente riprende dal passo successivo quando premi «Riprendi».</p>
        )}
        {live?.request && (
          <ToolRequestCard request={live.request} onAnswer={answer} who="L'agente" scope="in questa sessione" />
        )}
        {session.note && !active && (
          <p className={session.status === "failed" ? "error pre" : "notice"}>{session.note}</p>
        )}
        {session.status === "interrupted" && (
          <p className="notice">L'app è stata chiusa mentre l'agente lavorava. Scrivigli per farlo continuare.</p>
        )}
        {error && <p className="error pre">{error}</p>}
        <div ref={bottomRef} />
      </div>

      <div className="composer">
        <textarea
          value={reply}
          rows={2}
          disabled={active}
          placeholder={active ? "L'agente sta lavorando: puoi scrivergli quando si ferma." : "Rispondi all'agente o dagli un nuovo compito…"}
          title="Invio per inviare, Maiusc+Invio per andare a capo"
          onChange={(e) => setReply(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              sendReply();
            }
          }}
        />
        <button className="btn primary" disabled={active || !reply.trim()} onClick={sendReply}>
          Invia
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
        <h1>Agenti</h1>
        <button className="btn primary" onClick={() => setEditing(newAgent())}>
          Nuovo agente
        </button>
      </div>
      <p className="dim note">
        Un agente è un modello con istruzioni, cartelle di lavoro e permessi. Gli affidi un compito e lavora da solo,
        passo dopo passo, chiedendoti conferma quando serve.
      </p>
      {agents.length === 0 && <p className="dim">Nessun agente: creane uno per iniziare.</p>}
      {agents.map((a) => (
        <div key={a.id} className="card">
          <div className="card-head">
            <div>
              <div className="card-title">{a.name}</div>
              {a.description && <div className="dim">{a.description}</div>}
            </div>
            <div className="row">
              <button className="btn primary" onClick={() => onStart(a.id)}>
                Affida un compito
              </button>
              <button className="btn" onClick={() => setEditing(a)}>
                Modifica
              </button>
              <button className="btn" onClick={() => act(() => api.duplicateAgent(a.id))}>
                Duplica
              </button>
              <button
                className="btn"
                onClick={() => {
                  if (window.confirm(`Eliminare l'agente «${a.name}»? Le sue sessioni restano.`)) {
                    act(() => api.deleteAgent(a.id));
                  }
                }}
              >
                Elimina
              </button>
            </div>
          </div>
          <div className="card-body dim">
            {modelLabel(a, providers)} · {a.folders.length > 0 ? a.folders.map(baseName).join(", ") : "nessuna cartella"} ·{" "}
            {[
              a.tools.files && "file",
              a.tools.terminal && "terminale",
              a.tools.web && "web",
              a.mcpServers.length > 0 && `${a.mcpServers.length} server MCP`,
            ]
              .filter(Boolean)
              .join(", ") ||
              "nessuno strumento"}{" "}
            · {PERMISSIONS.find((p) => p.value === a.tools.permission)?.label}
          </div>
        </div>
      ))}
      {error && <p className="error pre">{error}</p>}
    </div>
  );
}

function modelLabel(agent: Agent, providers: ProviderInfo[]): string {
  if (agent.model.kind === "local") return "Modello locale caricato";
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
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  useEffect(() => {
    api.listMcpServers().then(setMcpServers).catch(() => {});
  }, []);
  const toggleMcp = (id: string, on: boolean) =>
    set({ mcpServers: on ? [...agent.mcpServers, id] : agent.mcpServers.filter((s) => s !== id) });
  const missingMcp = agent.mcpServers.filter((id) => !mcpServers.some((s) => s.id === id));
  const set = (change: Partial<Agent>) => setAgent((a) => ({ ...a, ...change }));
  const setTools = (change: Partial<Agent["tools"]>) => setAgent((a) => ({ ...a, tools: { ...a.tools, ...change } }));

  const addFolder = async () => {
    const picked = await open({ directory: true, title: "Scegli una cartella su cui l'agente può lavorare" });
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
      <h1>{initial.id === 0 ? "Nuovo agente" : `Modifica «${initial.name}»`}</h1>

      <div className="field">
        <label htmlFor="agent-name">Nome</label>
        <input id="agent-name" type="text" value={agent.name} onChange={(e) => set({ name: e.target.value })} />
      </div>
      <div className="field">
        <label htmlFor="agent-description">Descrizione</label>
        <input
          id="agent-description"
          type="text"
          value={agent.description}
          placeholder="A cosa serve, in una riga"
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
          Usa il modello locale caricato quando parte la sessione.{" "}
          <button className="btn-link" onClick={onOpenModels}>
            Vai ai modelli
          </button>
        </p>
      )}

      <div className="field">
        <label htmlFor="agent-instructions">Istruzioni</label>
        <textarea
          id="agent-instructions"
          rows={6}
          value={agent.instructions}
          placeholder="Come deve lavorare: stile del codice, comandi per provare, cosa evitare…"
          onChange={(e) => set({ instructions: e.target.value })}
        />
      </div>

      <section className="section">
        <div className="section-head">
          <h2>Cartelle di lavoro</h2>
          <button className="btn" onClick={addFolder}>
            Aggiungi cartella
          </button>
        </div>
        <p className="dim note">
          L'agente legge e scrive file solo qui dentro; i comandi partono dalla prima cartella.
        </p>
        {agent.folders.length === 0 && <p className="dim">Nessuna cartella.</p>}
        {agent.folders.map((folder) => (
          <div key={folder} className="folder">
            <span className="mono">{folder}</span>
            <button className="btn-link" onClick={() => set({ folders: agent.folders.filter((f) => f !== folder) })}>
              Togli
            </button>
          </div>
        ))}
        {needsFolder && <p className="notice">Senza una cartella l'agente non può usare file e terminale.</p>}
      </section>

      <section className="section">
        <h2>Strumenti e permessi</h2>
        <div className="tools-bar">
          <label>
            <input type="checkbox" checked={agent.tools.files} onChange={(e) => setTools({ files: e.target.checked })} />
            File
          </label>
          <label title="Comandi PowerShell nella cartella di lavoro">
            <input
              type="checkbox"
              checked={agent.tools.terminal}
              onChange={(e) => setTools({ terminal: e.target.checked })}
            />
            Terminale
          </label>
          <label title="Le parole cercate vengono inviate a DuckDuckGo">
            <input type="checkbox" checked={agent.tools.web} onChange={(e) => setTools({ web: e.target.checked })} />
            Ricerca web
          </label>
          <select
            aria-label="Permessi"
            value={agent.tools.permission}
            onChange={(e) => setTools({ permission: e.target.value as PermissionLevel })}
          >
            {PERMISSIONS.map((p) => (
              <option key={p.value} value={p.value}>
                {p.label}
              </option>
            ))}
          </select>
        </div>
        <p className="dim note">{PERMISSIONS.find((p) => p.value === agent.tools.permission)?.hint}</p>
        {agent.tools.terminal && agent.tools.permission === "all" && (
          <p className="notice">
            Attenzione: con «Consenti tutto» i comandi partono senza chiederti nulla, e un comando può agire anche fuori
            dalle cartelle di lavoro.
          </p>
        )}
      </section>

      <section className="section">
        <h2>Server MCP</h2>
        {mcpServers.length === 0 ? (
          <p className="dim note">Nessun server MCP configurato: si aggiungono nelle Impostazioni.</p>
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
              Partono con ogni sessione e si chiudono con lei. I loro strumenti chiedono conferma salvo con «Consenti
              tutto».
            </p>
          </>
        )}
        {missingMcp.length > 0 && (
          <p className="notice">
            {missingMcp.length === 1 ? "Un server scelto non esiste più" : "Alcuni server scelti non esistono più"}.{" "}
            <button className="btn-link" onClick={() => set({ mcpServers: agent.mcpServers.filter((id) => !missingMcp.includes(id)) })}>
              Togli
            </button>
          </p>
        )}
      </section>

      <div className="field">
        <label htmlFor="agent-steps">Passi massimi per volta</label>
        <input
          id="agent-steps"
          type="number"
          min={1}
          max={500}
          value={agent.maxSteps}
          onChange={(e) => set({ maxSteps: Number(e.target.value) || 1 })}
        />
        <p className="dim note">Arrivato qui l'agente si ferma e aspetta che tu gli risponda.</p>
      </div>

      <div className="row">
        <button className="btn primary" onClick={save} disabled={!agent.name.trim()}>
          Salva
        </button>
        <button className="btn" onClick={onCancel}>
          Annulla
        </button>
      </div>
      {error && <p className="error pre">{error}</p>}
    </div>
  );
}

// ── Registro delle azioni ─────────────────────────────────────────────────

const APPROVAL_LABELS: Record<ActionRecord["approval"], string> = {
  auto: "consentita dai permessi",
  user: "confermata da te",
  denied: "negata da te",
  refused: "rifiutata dall'app",
  interrupted: "interrotta",
};

/** Durate proposte per il registro, in giorni (0 = per sempre). */
const RETENTION_CHOICES = [7, 30, 90, 365, 0];

function retentionLabel(days: number): string {
  if (days === 0) return "per sempre";
  if (days === 365) return "per un anno";
  return `per ${days} giorni`;
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
      setMessage(removed > 0 ? `Eliminate ${removed} voci più vecchie.` : null);
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  };

  const clear = async () => {
    if (!window.confirm("Eliminare tutte le voci del registro? Non si possono recuperare.")) return;
    try {
      const removed = await api.clearActions();
      setMessage(`Eliminate ${removed} voci.`);
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  };

  // Un valore scritto a mano nell'archivio resta tra le scelte.
  const choices = days !== null && !RETENTION_CHOICES.includes(days) ? [...RETENTION_CHOICES, days] : RETENTION_CHOICES;

  return (
    <div className="retention">
      <label htmlFor="log-days">Conserva le voci</label>
      <select id="log-days" value={days ?? ""} disabled={days === null} onChange={(e) => choose(Number(e.target.value))}>
        {choices.map((d) => (
          <option key={d} value={d}>
            {retentionLabel(d)}
          </option>
        ))}
      </select>
      <button className="btn-link" disabled={empty} onClick={clear}>
        Svuota registro
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
      <h1>Registro azioni</h1>
      <p className="dim note">
        Ogni strumento usato dai modelli, in chat e dagli agenti, dalla più recente. Le voci restano anche se elimini la
        conversazione o la sessione.
      </p>
      <ActionLogRetention empty={actions.length === 0} onChanged={reload} onError={setError} />
      {actions.length === 0 && <p className="dim">Ancora nessuna azione.</p>}
      {actions.length > 0 && (
        <table className="actions-table">
          <thead>
            <tr>
              <th>Quando</th>
              <th>Dove</th>
              <th>Azione</th>
              <th>Esito</th>
            </tr>
          </thead>
          <tbody>
            {actions.map((a) => (
              <tr key={a.id} className={a.isError ? "failed" : undefined}>
                <td className="nowrap">{when(a.createdAt)}</td>
                <td>
                  <span className="dim">{a.kind === "agent" ? "Agente" : "Chat"}</span> {a.source}
                </td>
                <td>
                  <div>{toolLabel(a.tool)}</div>
                  <div className="dim mono action-summary">{a.summary}</div>
                </td>
                <td title={a.result}>
                  {a.isError ? "Errore" : "Riuscita"} · <span className="dim">{APPROVAL_LABELS[a.approval]}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {error && <p className="error pre">{error}</p>}
    </div>
  );
}
