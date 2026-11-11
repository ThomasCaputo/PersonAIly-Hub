import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  type Conversation,
  type GenerationStats,
  type Message,
  type Preset,
  type RuntimeStatus,
} from "../api";
import { Markdown } from "../components/Markdown";
import { ParamsPanel } from "../components/ParamsPanel";

type Props = {
  status: RuntimeStatus | null;
  preset: Preset | null;
  onPresetChange: (preset: Preset) => void;
  onOpenModels: () => void;
};

/** Messaggio mostrato a schermo: salvato, oppure in arrivo dal modello. */
type Shown = { key: string; role: string; content: string; model?: string | null };

export function ChatArea({ status, preset, onPresetChange, onOpenModels }: Props) {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [pendingUser, setPendingUser] = useState<string | null>(null);
  const [streaming, setStreaming] = useState<string | null>(null);
  const [reasoning, setReasoning] = useState("");
  const [stats, setStats] = useState<GenerationStats | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  const generating = streaming !== null;
  const loaded = status?.loaded ?? null;

  const refreshConversations = useCallback(async () => {
    const list = await api.listConversations();
    setConversations(list);
    return list;
  }, []);

  useEffect(() => {
    refreshConversations()
      .then((list) => setActiveId((current) => current ?? list[0]?.id ?? null))
      .catch((e) => setError(String(e)));
  }, [refreshConversations]);

  useEffect(() => {
    setStats(null);
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
    if (!text || generating || !preset) return;
    setError(null);
    setStats(null);

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
        (event) => {
          if (event.type === "text") setStreaming((s) => (s ?? "") + event.text);
          else setReasoning((r) => r + event.text);
        },
      );
      setStats(outcome.stats);
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

  const shown: Shown[] = messages.map((m) => ({ ...m, key: `m${m.id}` }));
  if (pendingUser !== null) {
    shown.push({ key: "pending-user", role: "user", content: pendingUser });
  }
  if (streaming !== null) {
    shown.push({ key: "streaming", role: "assistant", content: streaming, model: loaded?.modelName });
  }

  return (
    <div className="chat">
      <aside className="conversations">
        <button className="btn primary wide" onClick={newConversation}>
          Nuova conversazione
        </button>
        <div className="conversation-list">
          {conversations.map((c) => (
            <div key={c.id} className={c.id === activeId ? "conversation active" : "conversation"}>
              <button className="conversation-title" onClick={() => setActiveId(c.id)} title={c.title}>
                {c.title}
              </button>
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
        <div className="messages">
          {shown.length === 0 && (
            <div className="empty">
              {loaded ? (
                <p className="dim">Scrivi un messaggio per iniziare.</p>
              ) : (
                <>
                  <p className="dim">Per chattare serve un modello caricato.</p>
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
              {m.key === "streaming" && reasoning && !m.content && (
                <div className="reasoning">{reasoning}</div>
              )}
              {m.role === "user" ? (
                <div className="message-text">{m.content}</div>
              ) : m.content ? (
                <Markdown text={m.content} />
              ) : (
                !reasoning && <div className="dim">Sto elaborando…</div>
              )}
            </div>
          ))}
          {stats && !generating && <StatsLine stats={stats} />}
          {error && <p className="error pre">{error}</p>}
          <div ref={bottomRef} />
        </div>

        <div className="composer">
          <textarea
            value={draft}
            placeholder={loaded ? "Scrivi un messaggio…" : "Nessun modello caricato"}
            title="Invio per inviare, Maiusc+Invio per andare a capo"
            disabled={!loaded}
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
            <button className="btn primary" disabled={!loaded || !preset || !draft.trim()} onClick={send}>
              Invia
            </button>
          )}
        </div>
      </section>

      <aside className="params">
        {preset && loaded ? (
          <ParamsPanel preset={preset} loaded={loaded} onChange={onPresetChange} onOpenModels={onOpenModels} />
        ) : (
          <p className="dim">I parametri compaiono quando un modello è caricato.</p>
        )}
      </aside>
    </div>
  );
}

function StatsLine({ stats }: { stats: GenerationStats }) {
  const parts = [
    stats.tokensPerSecond != null &&
      `${stats.tokensPerSecond.toLocaleString("it-IT", { maximumFractionDigits: 1 })} token/s`,
    stats.completionTokens != null && `${stats.completionTokens} token generati`,
    stats.promptTokens != null && `${stats.promptTokens} token letti`,
  ].filter(Boolean);
  return parts.length > 0 ? <div className="stats dim">{parts.join(" · ")}</div> : null;
}
