import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  cloudPresetKey,
  usd,
  type GenerationStats,
  type Message,
  type ModelRef,
  type ProviderInfo,
  type RuntimeStatus,
  type StreamEvent,
} from "../api";
import { Markdown } from "../components/Markdown";
import { num, t, tc } from "../i18n";
import { ModelPicker } from "./ChatArea";

/** Ogni quanto si mostra il testo in arrivo (come in chat). */
const FLUSH_MS = 60;

/** Lo stato di una delle due colonne. */
type Side = {
  model: ModelRef;
  conversationId: number | null;
  messages: Message[];
  streaming: string | null;
  stats: GenerationStats | null;
  notice: string | null;
  error: string | null;
  /** Salvata come conversazione: da qui compare nella cronologia della chat. */
  saved: boolean;
};

const emptySide = (model: ModelRef): Side => ({
  model,
  conversationId: null,
  messages: [],
  streaming: null,
  stats: null,
  notice: null,
  error: null,
  saved: false,
});

/** Strumenti spenti: si confrontano le risposte, non le azioni. */
const NO_TOOLS = { files: false, web: false, terminal: false, permission: "ask" as const };

type Props = { status: RuntimeStatus | null; providers: ProviderInfo[] };

/**
 * Confronto tra due modelli: lo stesso messaggio va a entrambi, le risposte
 * arrivano affiancate e in parallelo. Ogni colonna è una conversazione
 * temporanea (senza strumenti): non compare nella cronologia della chat
 * finché l'utente non la salva, una o entrambe. "Nuovo confronto" cancella
 * quelle non salvate; il nucleo cancella le rimaste a ogni avvio. Di modelli
 * locali ne gira uno alla volta: si confronta di solito un locale con un
 * cloud, o due cloud.
 */
export function CompareArea({ status, providers }: Props) {
  const firstCloud = providers.find((p) => p.hasKey && p.models.length > 0);
  const [sides, setSides] = useState<[Side, Side]>(() => [
    emptySide({ kind: "local" }),
    emptySide(firstCloud ? { kind: "cloud", providerId: firstCloud.id, model: firstCloud.models[0].id } : { kind: "local" }),
  ]);
  const [draft, setDraft] = useState("");
  const buffers = useRef<[string, string]>(["", ""]);
  const timer = useRef<number | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  const loadedName = status?.loaded?.modelName ?? null;
  const update = useCallback((index: 0 | 1, change: Partial<Side>) => {
    setSides((current) => {
      const next: [Side, Side] = [current[0], current[1]];
      next[index] = { ...current[index], ...change };
      return next;
    });
  }, []);

  // Il primo provider con chiave può arrivare dopo l'apertura della pagina.
  useEffect(() => {
    if (!firstCloud) return;
    setSides((current) =>
      current[1].conversationId === null && current[1].model.kind === "local"
        ? [current[0], { ...current[1], model: { kind: "cloud", providerId: firstCloud.id, model: firstCloud.models[0].id } }]
        : current,
    );
  }, [firstCloud]);

  const flush = useCallback(() => {
    timer.current = null;
    const [a, b] = buffers.current;
    buffers.current = ["", ""];
    setSides((current) => [
      a ? { ...current[0], streaming: (current[0].streaming ?? "") + a } : current[0],
      b ? { ...current[1], streaming: (current[1].streaming ?? "") + b } : current[1],
    ]);
  }, []);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [sides]);

  const ready = (side: Side) => {
    if (side.model.kind === "local") return loadedName !== null;
    const model = side.model;
    return providers.some((p) => p.id === model.providerId && p.hasKey);
  };
  const generating = sides.some((s) => s.streaming !== null);
  const bothLocal = sides[0].model.kind === "local" && sides[1].model.kind === "local";
  const canSend = !generating && sides.every(ready) && draft.trim() !== "";

  const nameOf = (model: ModelRef) => {
    if (model.kind === "local") return loadedName ?? t("Modello locale");
    const provider = providers.find((p) => p.id === model.providerId);
    return provider?.models.find((m) => m.id === model.model)?.name ?? model.model;
  };

  /** Manda il testo a una colonna e segue la risposta. */
  const run = async (index: 0 | 1, text: string, side: Side) => {
    let conversationId = side.conversationId;
    try {
      const key = side.model.kind === "cloud" ? cloudPresetKey(side.model.providerId, side.model.model) : status?.loaded?.modelPath;
      if (!key) throw new Error(t("Nessun modello locale caricato"));
      const preset = await api.getPreset(key);
      const first = conversationId === null;
      if (conversationId === null) {
        conversationId = (await api.createTemporaryConversation()).id;
        await api.setConversationTools(conversationId, NO_TOOLS);
        update(index, { conversationId });
      }
      const id = conversationId;
      const onEvent = (event: StreamEvent) => {
        if (event.type === "text") {
          buffers.current[index] += event.text;
          if (timer.current === null) timer.current = window.setTimeout(flush, FLUSH_MS);
        } else if (event.type === "toolRequest") {
          // Gli strumenti sono spenti: per sicurezza, una richiesta si nega.
          api.answerToolRequest(event.id, "deny").catch(() => {});
        }
      };
      const outcome = await api.sendMessage(id, text, [], preset.sampling, preset.systemPrompt, side.model, onEvent);
      if (first) {
        // Nella cronologia si riconosce come confronto e si sa con chi.
        await api
          .renameConversation(id, `⚖ ${outcome.conversation.title} · ${nameOf(side.model)}`)
          .catch(() => {});
      }
      update(index, { stats: outcome.stats, notice: outcome.notice });
    } catch (e) {
      update(index, { error: String(e) });
    } finally {
      if (timer.current !== null) {
        window.clearTimeout(timer.current);
        flush();
      }
      buffers.current[index] = "";
      const messages = conversationId !== null ? await api.getMessages(conversationId).catch(() => null) : null;
      update(index, { streaming: null, ...(messages ? { messages } : {}) });
    }
  };

  const send = () => {
    const text = draft.trim();
    if (!canSend) return;
    setDraft("");
    const current = sides;
    // Il messaggio compare subito in entrambe le colonne.
    setSides(
      current.map((side) => ({
        ...side,
        streaming: "",
        stats: null,
        notice: null,
        error: null,
        messages: [...side.messages, pendingMessage(text)],
      })) as [Side, Side],
    );
    run(0, text, current[0]);
    run(1, text, current[1]);
  };

  /** Salva una colonna come conversazione normale. */
  const save = async (index: 0 | 1) => {
    const id = sides[index].conversationId;
    if (id === null) return;
    try {
      await api.keepConversation(id);
      update(index, { saved: true });
    } catch (e) {
      update(index, { error: String(e) });
    }
  };
  const unsaved = ([0, 1] as const).filter((i) => sides[i].conversationId !== null && !sides[i].saved);

  // Le colonne non salvate si buttano: erano temporanee.
  const restart = () => {
    for (const side of sides) {
      if (side.conversationId !== null && !side.saved) api.deleteConversation(side.conversationId).catch(() => {});
    }
    setSides((current) => [emptySide(current[0].model), emptySide(current[1].model)]);
  };

  const stop = () => {
    for (const side of sides) if (side.conversationId !== null) api.stopGeneration(side.conversationId).catch(() => {});
  };

  return (
    <div className="compare">
      <div className="compare-head">
        <h1>{t("Confronto tra modelli")}</h1>
        {unsaved.length === 2 && (
          <button className="btn" disabled={generating} onClick={() => unsaved.forEach(save)}>
            {t("Salva entrambe")}
          </button>
        )}
        <button className="btn" disabled={generating} onClick={restart}>
          {t("Nuovo confronto")}
        </button>
      </div>
      <p className="dim note">
        {t("Le due conversazioni restano solo qui: salva quelle che vuoi tenere nella chat. Le altre si cancellano con «Nuovo confronto» o chiudendo l'app.")}
      </p>
      {bothLocal && (
        <p className="notice">
          {t("Di modelli locali ne è caricato uno alla volta: le due colonne useranno lo stesso modello.")}
        </p>
      )}
      <div className="compare-columns">
        {sides.map((side, index) => {
          const cost = side.messages.reduce((sum, m) => sum + (m.costUsd ?? 0), 0);
          return (
            <section key={index} className="compare-column" aria-label={t("Colonna {n}", { n: index + 1 })}>
              <ModelPicker
                id={`compare-model-${index}`}
                providers={providers}
                loadedName={loadedName}
                value={side.model}
                disabled={generating || side.conversationId !== null}
                onChange={(model) => update(index as 0 | 1, { model })}
              />
              <div className="compare-messages">
                {side.messages.map((m) =>
                  m.role === "user" ? (
                    <div key={m.id} className="message user">
                      <div className="message-text">{m.content}</div>
                    </div>
                  ) : m.role === "assistant" && m.content ? (
                    <div key={m.id} className="message assistant">
                      <Markdown text={m.content} />
                    </div>
                  ) : null,
                )}
                {side.streaming !== null && (
                  <div className="message assistant">
                    {side.streaming ? (
                      <Markdown text={side.streaming} streaming />
                    ) : (
                      <div className="dim">{t("Sto elaborando…")}</div>
                    )}
                  </div>
                )}
                {side.stats && side.streaming === null && <CompareStats stats={side.stats} cost={cost} />}
                {side.notice && <p className="notice">{tc(side.notice)}</p>}
                {side.error && <p className="error pre" role="alert">{tc(side.error)}</p>}
                {side.conversationId !== null &&
                  (side.saved ? (
                    <p className="dim note">{t("Salvata nella chat.")}</p>
                  ) : (
                    <button
                      className="btn compare-save"
                      disabled={side.streaming !== null}
                      onClick={() => save(index as 0 | 1)}
                    >
                      {t("Salva come conversazione")}
                    </button>
                  ))}
                {!ready(side) && (
                  <p className="dim note">
                    {side.model.kind === "local"
                      ? t("Carica un modello nella pagina Modelli.")
                      : t("Manca la chiave API di {name}.", { name: nameOf(side.model) })}
                  </p>
                )}
              </div>
            </section>
          );
        })}
      </div>
      <div ref={bottomRef} />
      <div className="composer compare-composer">
        <textarea
          value={draft}
          aria-label={t("Messaggio per entrambi i modelli")}
          placeholder={t("Scrivi un messaggio per entrambi i modelli…")}
          title={t("Invio per inviare, Maiusc+Invio per andare a capo")}
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
          <button className="btn" onClick={stop}>
            {t("Interrompi")}
          </button>
        ) : (
          <button className="btn primary" disabled={!canSend} onClick={send}>
            {t("Invia")}
          </button>
        )}
      </div>
    </div>
  );
}

/** Un messaggio dell'utente già mostrato, in attesa che l'archivio lo salvi. */
let pendingIds = -1;
function pendingMessage(text: string): Message {
  return {
    id: pendingIds--,
    conversationId: 0,
    role: "user",
    content: text,
    model: null,
    createdAt: "",
    promptTokens: null,
    completionTokens: null,
    costUsd: null,
    toolCalls: [],
    toolCallId: null,
    toolName: null,
    isError: false,
    attachments: [],
    parentId: null,
    siblings: [],
  };
}

function CompareStats({ stats, cost }: { stats: GenerationStats; cost: number }) {
  const parts = [
    stats.tokensPerSecond != null && `${num(stats.tokensPerSecond, { maximumFractionDigits: 1 })} token/s`,
    stats.completionTokens != null && t("{n} token generati", { n: stats.completionTokens }),
    cost > 0 && t("circa {cost}", { cost: usd(cost) }),
  ].filter(Boolean);
  return parts.length > 0 ? <div className="stats dim">{parts.join(" · ")}</div> : null;
}
