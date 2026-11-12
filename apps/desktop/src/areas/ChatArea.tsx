import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  usd,
  type Conversation,
  type GenerationStats,
  type Message,
  type ModelRef,
  type Preset,
  type ProviderInfo,
  type RuntimeStatus,
} from "../api";
import { Markdown } from "../components/Markdown";
import { ParamsPanel } from "../components/ParamsPanel";

type Props = {
  status: RuntimeStatus | null;
  providers: ProviderInfo[];
  activeModel: ModelRef;
  onModelChange: (model: ModelRef) => void;
  preset: Preset | null;
  onPresetChange: (preset: Preset) => void;
  onOpenModels: () => void;
};

/** Messaggio mostrato a schermo: salvato, oppure in arrivo dal modello. */
type Shown = { key: string; role: string; content: string; model?: string | null };

export function ChatArea({
  status,
  providers,
  activeModel,
  onModelChange,
  preset,
  onPresetChange,
  onOpenModels,
}: Props) {
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
      ? "Il provider di questo modello non esiste più: scegline un altro."
      : !provider.hasKey
        ? `Manca la chiave API di ${provider.name}.`
        : null
    : loaded
      ? null
      : "Nessun modello locale caricato.";

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
      setDraft("");
      setPendingUser(text);
      setStreaming("");
      setReasoning("");

      const outcome = await api.sendMessage(
        conversationId,
        text,
        preset.sampling,
        preset.systemPrompt,
        activeModel,
        (event) => {
          if (event.type === "text") setStreaming((s) => (s ?? "") + event.text);
          else setReasoning((r) => r + event.text);
        },
      );
      setStats(outcome.stats);
      setNotice(outcome.notice);
    } catch (e) {
      setError(String(e));
    } finally {
      // La fonte di verità è l'archivio: si rilegge quello che è stato salvato.
      if (conversationId !== null) {
        await api.getMessages(conversationId).then(setMessages).catch(() => {});
      }
      setPendingUser(null);
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

  const shown: Shown[] = messages.map((m) => ({ ...m, key: `m${m.id}` }));
  if (pendingUser !== null) {
    shown.push({ key: "pending-user", role: "user", content: pendingUser });
  }
  if (streaming !== null) {
    shown.push({ key: "streaming", role: "assistant", content: streaming, model: modelName });
  }

  return (
    <div className="chat">
      <aside className="conversations">
        <button className="btn primary wide" onClick={newConversation}>
          Nuova conversazione
        </button>
        <input
          type="search"
          className="conversation-search"
          placeholder="Cerca nelle conversazioni"
          aria-label="Cerca nelle conversazioni"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="conversation-list">
          {conversations.length === 0 && query && <p className="dim note">Nessun risultato.</p>}
          {conversations.map((c) => (
            <div key={c.id} className={c.id === activeId ? "conversation active" : "conversation"}>
              {renaming?.id === c.id ? (
                <input
                  className="conversation-rename"
                  aria-label="Nuovo titolo"
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
                  title={`${c.title} — doppio clic per rinominare`}
                >
                  {c.title}
                </button>
              )}
              <button
                className="conversation-delete"
                title="Elimina conversazione"
                aria-label={`Elimina la conversazione ${c.title}`}
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

        <div className="messages">
          {shown.length === 0 && (
            <div className="empty">
              {canSend ? (
                <p className="dim">Scrivi un messaggio per iniziare.</p>
              ) : (
                <>
                  <p className="dim">{blockedReason}</p>
                  <button className="btn" onClick={onOpenModels}>
                    Vai ai modelli
                  </button>
                </>
              )}
            </div>
          )}
          {shown.map((m) => (
            <div key={m.key} className={`message ${m.role}`}>
              <div className="message-role">{m.role === "user" ? "Tu" : (m.model ?? "Modello")}</div>
              {m.key === "streaming" && reasoning && !m.content && <div className="reasoning">{reasoning}</div>}
              {m.role === "user" ? (
                <div className="message-text">{m.content}</div>
              ) : m.content ? (
                <Markdown text={m.content} />
              ) : (
                !reasoning && <div className="dim">Sto elaborando…</div>
              )}
            </div>
          ))}
          {stats && !generating && <StatsLine stats={stats} cost={lastCost} />}
          {notice && !generating && <p className="notice">{notice}</p>}
          {error && <p className="error pre">{error}</p>}
          <div ref={bottomRef} />
        </div>

        <UsageBar
          contextUsed={canSend ? contextUsed : null}
          contextTotal={contextTotal}
          cost={conversationCost}
        />
        <div className="composer">
          <textarea
            value={draft}
            placeholder={canSend ? "Scrivi un messaggio…" : (blockedReason ?? "")}
            title="Invio per inviare, Maiusc+Invio per andare a capo"
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
            <button className="btn" onClick={() => api.stopGeneration()}>
              Interrompi
            </button>
          ) : (
            <button className="btn primary" disabled={!canSend || !preset || !draft.trim()} onClick={send}>
              Invia
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
          <p className="dim">I parametri compaiono quando c'è un modello pronto a rispondere.</p>
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
function ModelPicker({ providers, loadedName, value, disabled, onChange }: PickerProps) {
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
      <label htmlFor="model-picker">Modello</label>
      <select id="model-picker" value={current} disabled={disabled} onChange={(e) => onChange(decode(e.target.value))}>
        <optgroup label="Su questo computer">
          <option value={encode({ kind: "local" })}>{loadedName ?? "Nessun modello locale caricato"}</option>
        </optgroup>
        {providers
          .filter((p) => p.models.length > 0)
          .map((p) => (
            <optgroup key={p.id} label={p.hasKey ? p.name : `${p.name} (chiave mancante)`}>
              {p.models.map((m) => (
                <option key={m.id} value={encode({ kind: "cloud", providerId: p.id, model: m.id })}>
                  {m.name}
                </option>
              ))}
            </optgroup>
          ))}
        {!known && value.kind === "cloud" && <option value={current}>{value.model} (non disponibile)</option>}
      </select>
    </div>
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
            title="Quando il contesto si riempie il modello non può più leggere tutta la conversazione"
          >
            Contesto occupato: {contextUsed.toLocaleString("it-IT")} / {contextTotal.toLocaleString("it-IT")} token
          </span>
          <div className="meter-track">
            <div className={percent > 85 ? "meter-fill high" : "meter-fill"} style={{ width: `${percent}%` }} />
          </div>
        </>
      )}
      {cost > 0 && (
        <span
          className="dim"
          title="Stima dai prezzi di listino: la cifra addebitata dal servizio può essere diversa"
        >
          Costo stimato della conversazione: {usd(cost)}
        </span>
      )}
    </div>
  );
}

function StatsLine({ stats, cost }: { stats: GenerationStats; cost: number | null }) {
  const parts = [
    stats.tokensPerSecond != null &&
      `${stats.tokensPerSecond.toLocaleString("it-IT", { maximumFractionDigits: 1 })} token/s`,
    stats.completionTokens != null && `${stats.completionTokens.toLocaleString("it-IT")} token generati`,
    stats.promptTokens != null && `${stats.promptTokens.toLocaleString("it-IT")} token letti`,
    cost != null && cost > 0 && `circa ${usd(cost)}`,
  ].filter(Boolean);
  return parts.length > 0 ? <div className="stats dim">{parts.join(" · ")}</div> : null;
}
