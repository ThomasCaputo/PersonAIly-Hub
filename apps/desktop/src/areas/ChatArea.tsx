import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  mcpToolLabel,
  usd,
  type Approval,
  type Conversation,
  type GenerationStats,
  type Message,
  type ModelRef,
  type Preset,
  type ProviderInfo,
  type RuntimeStatus,
  type ToolRequest,
  type ToolSettings,
} from "../api";
import { Markdown } from "../components/Markdown";
import { ParamsPanel } from "../components/ParamsPanel";
import { ToolRequestCard, ToolsBar } from "../components/ToolsBar";
import { msg, num, t, tc } from "../i18n";

const attachmentFilters = () => [
  {
    name: t("Testo, PDF e immagini"),
    extensions: ["txt", "md", "csv", "json", "pdf", "png", "jpg", "jpeg", "gif", "webp"],
  },
  { name: t("Tutti i file"), extensions: ["*"] },
];

/**
 * Ogni quanto si mostra il testo in arrivo. Ridisegnare il Markdown (con
 * formule e codice evidenziato) a ogni token rende l'interfaccia lenta con le
 * risposte lunghe: il lavoro cresce con il quadrato della lunghezza.
 */
const STREAM_FLUSH_MS = 60;

/** Nome del file da un percorso di Windows o di altri sistemi. */
const baseName = (path: string) => path.split(/[\\/]/).pop() ?? path;

type Props = {
  status: RuntimeStatus | null;
  providers: ProviderInfo[];
  activeModel: ModelRef;
  onModelChange: (model: ModelRef) => void;
  preset: Preset | null;
  onPresetChange: (preset: Preset) => void;
  onOpenModels: () => void;
  onOpenSettings: () => void;
};

/** Messaggio mostrato a schermo: salvato, oppure in arrivo dal modello. */
type Shown = { key: string; role: string; content: string; model?: string | null; saved?: Message };

export function ChatArea({
  status,
  providers,
  activeModel,
  onModelChange,
  preset,
  onPresetChange,
  onOpenModels,
  onOpenSettings,
}: Props) {
  const [toolSettings, setToolSettings] = useState<ToolSettings | null>(null);
  const [folderCount, setFolderCount] = useState(0);
  const [attachments, setAttachments] = useState<string[]>([]);
  const [toolRequest, setToolRequest] = useState<ToolRequest | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [pendingUser, setPendingUser] = useState<string | null>(null);
  const [streaming, setStreaming] = useState<string | null>(null);
  const [reasoning, setReasoning] = useState("");
  const [stats, setStats] = useState<GenerationStats | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [renaming, setRenaming] = useState<{ id: number; title: string } | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  // Testo e ragionamento arrivati ma non ancora mostrati (vedi STREAM_FLUSH_MS).
  const streamBuffer = useRef({ text: "", reasoning: "", timer: null as number | null });

  const flushStream = useCallback(() => {
    const buffer = streamBuffer.current;
    if (buffer.timer !== null) window.clearTimeout(buffer.timer);
    buffer.timer = null;
    const { text, reasoning: thought } = buffer;
    buffer.text = "";
    buffer.reasoning = "";
    if (text) setStreaming((s) => (s ?? "") + text);
    if (thought) setReasoning((r) => r + thought);
  }, []);

  /** Butta il testo in attesa: è già nell'archivio, o la risposta è finita. */
  const discardStream = useCallback(() => {
    const buffer = streamBuffer.current;
    if (buffer.timer !== null) window.clearTimeout(buffer.timer);
    buffer.timer = null;
    buffer.text = "";
    buffer.reasoning = "";
  }, []);

  const queueStream = useCallback(
    (kind: "text" | "reasoning", piece: string) => {
      const buffer = streamBuffer.current;
      buffer[kind] += piece;
      if (buffer.timer === null) buffer.timer = window.setTimeout(flushStream, STREAM_FLUSH_MS);
    },
    [flushStream],
  );

  useEffect(() => discardStream, [discardStream]);

  const generating = streaming !== null;
  const loaded = status?.loaded ?? null;

  // Il modello scelto: quello locale caricato oppure uno di un provider cloud.
  const provider = activeModel.kind === "cloud" ? providers.find((p) => p.id === activeModel.providerId) : undefined;
  const cloudModel = activeModel.kind === "cloud" ? provider?.models.find((m) => m.id === activeModel.model) : undefined;
  const isCloud = activeModel.kind === "cloud";
  const modelName = isCloud ? (cloudModel?.name ?? activeModel.model) : (loaded?.modelName ?? null);
  const canSend = isCloud ? Boolean(provider?.hasKey) : loaded !== null;
  const contextTotal = isCloud ? (cloudModel?.contextLength ?? null) : (loaded?.params.contextLength ?? null);
  const blockedReason = isCloud
    ? !provider
      ? t("Il provider di questo modello non esiste più: scegline un altro.")
      : !provider.hasKey
        ? t("Manca la chiave API di {name}.", { name: provider.name })
        : null
    : loaded
      ? null
      : t("Nessun modello locale caricato.");

  const refreshConversations = useCallback(async () => {
    const list = await api.listConversations(query);
    setConversations(list);
    return list;
  }, [query]);

  const commitRename = async () => {
    if (!renaming) return;
    const title = renaming.title.trim();
    setRenaming(null);
    if (title) {
      await api.renameConversation(renaming.id, title).catch((e) => setError(String(e)));
      refreshConversations().catch(() => {});
    }
  };

  useEffect(() => {
    refreshConversations()
      .then((list) => setActiveId((current) => current ?? list[0]?.id ?? null))
      .catch((e) => setError(String(e)));
  }, [refreshConversations]);

  useEffect(() => {
    setStats(null);
    setNotice(null);
    if (activeId === null) {
      setMessages([]);
      return;
    }
    api.getMessages(activeId).then(setMessages).catch((e) => setError(String(e)));
  }, [activeId]);

  // Strumenti della conversazione aperta; quelli predefiniti per una nuova.
  useEffect(() => {
    api.grantedFolders().then((f) => setFolderCount(f.length)).catch(() => {});
    const load = activeId === null ? api.toolDefaults() : api.conversationTools(activeId);
    load.then(setToolSettings).catch(() => {});
  }, [activeId]);

  const changeTools = async (next: ToolSettings) => {
    setToolSettings(next);
    let id = activeId;
    if (id === null) {
      id = (await api.createConversation()).id;
      setActiveId(id);
      refreshConversations().catch(() => {});
    }
    await api.setConversationTools(id, next).catch((e) => setError(String(e)));
  };

  const pickAttachments = async () => {
    const picked = await open({ multiple: true, title: t("Allega file"), filters: attachmentFilters() });
    const paths = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
    setAttachments((current) => [...current, ...paths.filter((p) => !current.includes(p))]);
  };

  const answer = (approval: Approval) => {
    if (!toolRequest) return;
    api.answerToolRequest(toolRequest.id, approval).catch(() => {});
    setToolRequest(null);
  };

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [messages, streaming, pendingUser]);

  const newConversation = async () => {
    const conversation = await api.createConversation();
    await refreshConversations();
    setActiveId(conversation.id);
  };

  const removeConversation = async (id: number) => {
    await api.deleteConversation(id);
    const list = await refreshConversations();
    if (id === activeId) setActiveId(list[0]?.id ?? null);
  };

  const send = async () => {
    const text = draft.trim();
    if (!text || generating || !preset || !canSend) return;
    setError(null);
    setStats(null);
    setNotice(null);

    let conversationId = activeId;
    try {
      if (conversationId === null) {
        conversationId = (await api.createConversation()).id;
        setActiveId(conversationId);
      }
      const files = attachments;
      setDraft("");
      setAttachments([]);
      setPendingUser(text);
      setStreaming("");
      setReasoning("");

      const id = conversationId;
      const outcome = await api.sendMessage(
        id,
        text,
        files,
        preset.sampling,
        preset.systemPrompt,
        activeModel,
        (event) => {
          if (event.type === "text") queueStream("text", event.text);
          else if (event.type === "reasoning") queueStream("reasoning", event.text);
          else if (event.type === "toolRequest") {
            // Prima di chiedere conferma si mostra tutto ciò che il modello ha scritto.
            flushStream();
            setToolRequest(event);
          } else {
            discardStream();
            // Il nucleo ha salvato un pezzo di risposta o il risultato di uno
            // strumento: si rilegge l'archivio e si ricomincia a raccogliere il testo.
            api
              .getMessages(id)
              .then((saved) => {
                setMessages(saved);
                setPendingUser(null);
              })
              .catch(() => {});
            setStreaming("");
            setReasoning("");
          }
        },
      );
      setStats(outcome.stats);
      setNotice(outcome.notice);
    } catch (e) {
      setError(String(e));
    } finally {
      setToolRequest(null);
      // "Consenti sempre" cambia i permessi della conversazione nel nucleo.
      if (conversationId !== null) {
        api.conversationTools(conversationId).then(setToolSettings).catch(() => {});
      }
      // La fonte di verità è l'archivio: si rilegge quello che è stato salvato.
      if (conversationId !== null) {
        await api.getMessages(conversationId).then(setMessages).catch(() => {});
      }
      setPendingUser(null);
      discardStream();
      setStreaming(null);
      refreshConversations().catch(() => {});
    }
  };

  // Consumo, ricavato dai messaggi salvati: sopravvive al riavvio dell'app.
  // Il contesto occupato è quello dell'ultima risposta: token letti + generati.
  const replies = messages.filter((m) => m.role === "assistant" && m.promptTokens != null);
  const lastReply = replies.length > 0 ? replies[replies.length - 1] : null;
  const contextUsed = lastReply ? (lastReply.promptTokens ?? 0) + (lastReply.completionTokens ?? 0) : null;
  const conversationCost = messages.reduce((sum, m) => sum + (m.costUsd ?? 0), 0);
  const lastMessage = messages.length > 0 ? messages[messages.length - 1] : null;
  const lastCost = lastMessage?.role === "assistant" ? lastMessage.costUsd : null;

  // Una risposta fatta solo di chiamate a strumenti si vede già nelle righe dei risultati.
  const shown: Shown[] = messages
    .filter((m) => !(m.role === "assistant" && !m.content && m.toolCalls.length > 0))
    .map((m) => ({ ...m, key: `m${m.id}`, saved: m }));
  if (pendingUser !== null) {
    shown.push({ key: "pending-user", role: "user", content: pendingUser });
  }
  // Mentre si aspetta una conferma la risposta è ferma: niente bolla vuota.
  if (streaming !== null && !(toolRequest && !streaming)) {
    shown.push({ key: "streaming", role: "assistant", content: streaming, model: modelName });
  }

  return (
    <div className="chat">
      <aside className="conversations">
        <button className="btn primary wide" onClick={newConversation}>
          {t("Nuova conversazione")}
        </button>
        <input
          type="search"
          className="conversation-search"
          placeholder={t("Cerca nelle conversazioni")}
          aria-label={t("Cerca nelle conversazioni")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="conversation-list">
          {conversations.length === 0 && query && <p className="dim note">{t("Nessun risultato.")}</p>}
          {conversations.map((c) => (
            <div key={c.id} className={c.id === activeId ? "conversation active" : "conversation"}>
              {renaming?.id === c.id ? (
                <input
                  className="conversation-rename"
                  aria-label={t("Nuovo titolo")}
                  autoFocus
                  value={renaming.title}
                  onChange={(e) => setRenaming({ id: c.id, title: e.target.value })}
                  onBlur={commitRename}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commitRename();
                    if (e.key === "Escape") setRenaming(null);
                  }}
                />
              ) : (
                <button
                  className="conversation-title"
                  onClick={() => setActiveId(c.id)}
                  onDoubleClick={() => setRenaming({ id: c.id, title: c.title })}
                  title={t("{title} — doppio clic per rinominare", { title: tc(c.title) })}
                >
                  {tc(c.title)}
                </button>
              )}
              <button
                className="conversation-delete"
                title={t("Elimina conversazione")}
                aria-label={t("Elimina la conversazione {title}", { title: tc(c.title) })}
                onClick={() => removeConversation(c.id)}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      </aside>

      <section className="thread">
        <ModelPicker
          providers={providers}
          loadedName={loaded?.modelName ?? null}
          value={activeModel}
          disabled={generating}
          onChange={onModelChange}
        />
        {toolSettings && (
          <ToolsBar
            settings={toolSettings}
            folderCount={folderCount}
            disabled={generating}
            onChange={changeTools}
            onOpenSettings={onOpenSettings}
          />
        )}

        <div className="messages">
          {shown.length === 0 && (
            <div className="empty">
              {canSend ? (
                <p className="dim">{t("Scrivi un messaggio per iniziare.")}</p>
              ) : (
                <>
                  <p className="dim">{blockedReason}</p>
                  <button className="btn" onClick={onOpenModels}>
                    {t("Vai ai modelli")}
                  </button>
                </>
              )}
            </div>
          )}
          {shown.map((m) =>
            m.role === "tool" ? (
              <ToolResult key={m.key} message={m.saved!} />
            ) : (
              <div key={m.key} className={`message ${m.role}`}>
                <div className="message-role">{m.role === "user" ? t("Tu") : (m.model ?? t("Modello"))}</div>
                {m.key === "streaming" && reasoning && !m.content && <div className="reasoning">{reasoning}</div>}
                {m.saved && m.saved.attachments.length > 0 && (
                  <div className="attachment-list">
                    {m.saved.attachments.map((a) => (
                      <span key={a.name} className="attachment">
                        {a.kind === "image" ? "🖼" : "📄"} {a.name}
                      </span>
                    ))}
                  </div>
                )}
                {m.role === "user" ? (
                  <div className="message-text">{m.content}</div>
                ) : m.content ? (
                  <Markdown text={m.content} />
                ) : m.saved && m.saved.toolCalls.length > 0 ? null : (
                  !reasoning && !toolRequest && <div className="dim">{t("Sto elaborando…")}</div>
                )}
              </div>
            ),
          )}
          {toolRequest && <ToolRequestCard request={toolRequest} onAnswer={answer} />}
          {stats && !generating && <StatsLine stats={stats} cost={lastCost} />}
          {notice && !generating && <p className="notice">{tc(notice)}</p>}
          {error && <p className="error pre">{tc(error)}</p>}
          <div ref={bottomRef} />
        </div>

        <UsageBar
          contextUsed={canSend ? contextUsed : null}
          contextTotal={contextTotal}
          cost={conversationCost}
        />
        {attachments.length > 0 && (
          <div className="attachment-list composer-attachments">
            {attachments.map((path) => (
              <span key={path} className="attachment" title={path}>
                {baseName(path)}
                <button
                  className="conversation-delete"
                  aria-label={t("Togli {name}", { name: baseName(path) })}
                  onClick={() => setAttachments(attachments.filter((p) => p !== path))}
                >
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="composer">
          <button
            className="btn"
            title={t("Allega file di testo, PDF o immagini")}
            aria-label={t("Allega file")}
            disabled={!canSend || generating}
            onClick={pickAttachments}
          >
            📎
          </button>
          <textarea
            value={draft}
            placeholder={canSend ? t("Scrivi un messaggio…") : (blockedReason ?? "")}
            title={t("Invio per inviare, Maiusc+Invio per andare a capo")}
            disabled={!canSend}
            rows={3}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
          />
          {generating ? (
            <button className="btn" onClick={() => api.stopGeneration(activeId)}>
              {t("Interrompi")}
            </button>
          ) : (
            <button className="btn primary" disabled={!canSend || !preset || !draft.trim()} onClick={send}>
              {t("Invia")}
            </button>
          )}
        </div>
      </section>

      <aside className="params">
        {preset && canSend ? (
          <ParamsPanel
            // Cambiando modello i campi di testo ripartono dai valori del nuovo preset.
            key={isCloud ? `${activeModel.providerId}/${activeModel.model}` : (loaded?.modelPath ?? "local")}
            preset={preset}
            loaded={isCloud ? null : loaded}
            supported={isCloud ? (provider?.supportedParams ?? []) : null}
            onChange={onPresetChange}
            onOpenModels={onOpenModels}
          />
        ) : (
          <p className="dim">{t("I parametri compaiono quando c'è un modello pronto a rispondere.")}</p>
        )}
      </aside>
    </div>
  );
}

type PickerProps = {
  providers: ProviderInfo[];
  loadedName: string | null;
  value: ModelRef;
  disabled: boolean;
  onChange: (model: ModelRef) => void;
};

/** Scelta del modello a cui inviare i messaggi; si può cambiare a metà conversazione. */
export function ModelPicker({ providers, loadedName, value, disabled, onChange }: PickerProps) {
  const encode = (model: ModelRef) => JSON.stringify(model.kind === "local" ? ["local"] : [model.providerId, model.model]);
  const decode = (text: string): ModelRef => {
    const parts = JSON.parse(text) as string[];
    return parts.length === 2 ? { kind: "cloud", providerId: parts[0], model: parts[1] } : { kind: "local" };
  };

  const current = encode(value);
  const known =
    value.kind === "local" ||
    providers.some((p) => p.id === value.providerId && p.models.some((m) => m.id === value.model));

  return (
    <div className="model-picker">
      <label htmlFor="model-picker">{t("Modello")}</label>
      <select id="model-picker" value={current} disabled={disabled} onChange={(e) => onChange(decode(e.target.value))}>
        <optgroup label={t("Su questo computer")}>
          <option value={encode({ kind: "local" })}>{loadedName ?? t("Nessun modello locale caricato")}</option>
        </optgroup>
        {providers
          .filter((p) => p.models.length > 0)
          .map((p) => (
            <optgroup key={p.id} label={p.hasKey ? p.name : t("{name} (chiave mancante)", { name: p.name })}>
              {p.models.map((m) => (
                <option key={m.id} value={encode({ kind: "cloud", providerId: p.id, model: m.id })}>
                  {m.name}
                </option>
              ))}
            </optgroup>
          ))}
        {!known && value.kind === "cloud" && (
          <option value={current}>{t("{model} (non disponibile)", { model: value.model })}</option>
        )}
      </select>
    </div>
  );
}

export const TOOL_LABELS: Record<string, string> = {
  list_folder: msg("Elenco della cartella"),
  read_file: msg("Lettura del file"),
  write_file: msg("Scrittura del file"),
  edit_file: msg("Modifica del file"),
  web_search: msg("Ricerca web"),
  read_web_page: msg("Lettura della pagina"),
  run_command: msg("Comando nel terminale"),
};

/** Nome leggibile di uno strumento dell'app o di un server MCP. */
export function toolLabel(name: string): string {
  return (TOOL_LABELS[name] && t(TOOL_LABELS[name])) ?? mcpToolLabel(name) ?? name;
}

/** Il risultato di uno strumento: una riga, con il contenuto da aprire a richiesta. */
export function ToolResult({ message }: { message: Message }) {
  const label = message.toolName ? toolLabel(message.toolName) : t("Strumento");
  const firstLine = message.content.split("\n")[0];
  return (
    <details className={message.isError ? "tool-result failed" : "tool-result"}>
      <summary>
        <span className="tool-result-label">
          {message.isError
            ? message.toolName?.startsWith("mcp__")
              ? t("{label} · errore", { label })
              : message.toolName === "run_command"
                ? t("{label} non riuscito", { label })
                : t("{label} non riuscita", { label })
            : label}
        </span>
        <span className="dim"> · {firstLine.length > 90 ? `${tc(firstLine).slice(0, 90)}…` : tc(firstLine)}</span>
      </summary>
      <pre className="tool-preview">{message.isError ? tc(message.content) : message.content}</pre>
    </details>
  );
}

type UsageProps = { contextUsed: number | null; contextTotal: number | null; cost: number };

/** Contesto occupato dalla conversazione e, per i modelli a pagamento, costo stimato finora. */
function UsageBar({ contextUsed, contextTotal, cost }: UsageProps) {
  const showContext = contextUsed !== null && contextTotal !== null;
  if (!showContext && cost <= 0) return null;
  const percent = showContext ? Math.min(100, (contextUsed / Math.max(1, contextTotal)) * 100) : 0;

  return (
    <div className="context-meter">
      {showContext && (
        <>
          <span
            className="dim"
            title={t("Quando il contesto si riempie il modello non può più leggere tutta la conversazione")}
          >
            {t("Contesto occupato: {used} / {total} token", { used: contextUsed, total: contextTotal })}
          </span>
          <div className="meter-track">
            <div className={percent > 85 ? "meter-fill high" : "meter-fill"} style={{ width: `${percent}%` }} />
          </div>
        </>
      )}
      {cost > 0 && (
        <span
          className="dim"
          title={t("Stima dai prezzi di listino: la cifra addebitata dal servizio può essere diversa")}
        >
          {t("Costo stimato della conversazione: {cost}", { cost: usd(cost) })}
        </span>
      )}
    </div>
  );
}

function StatsLine({ stats, cost }: { stats: GenerationStats; cost: number | null }) {
  const parts = [
    stats.tokensPerSecond != null && `${num(stats.tokensPerSecond, { maximumFractionDigits: 1 })} token/s`,
    stats.completionTokens != null && t("{n} token generati", { n: stats.completionTokens }),
    stats.promptTokens != null && t("{n} token letti", { n: stats.promptTokens }),
    cost != null && cost > 0 && t("circa {cost}", { cost: usd(cost) }),

  ].filter(Boolean);
  return parts.length > 0 ? <div className="stats dim">{parts.join(" · ")}</div> : null;
}
