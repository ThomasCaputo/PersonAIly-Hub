import { forwardRef, useEffect, useImperativeHandle, useRef, useState, type KeyboardEvent } from "react";
import { api, type McpPrompt, type PluginCommandInfo } from "../api";
import { t } from "../i18n";

/** Ciò che la casella del messaggio chiede al menu: gestire un tasto. */
export type PromptMenuHandle = { handleKey: (e: KeyboardEvent) => boolean };

type Props = {
  conversationId: number | null;
  /** I server MCP scelti: cambiandoli i prompt si rileggono. */
  servers: string[];
  draft: string;
  setDraft: (text: string) => void;
  onError: (message: string) => void;
};

type Arg = { name: string; label: string; description: string; required: boolean };

/** Una voce del menu: un prompt MCP o un comando di un plugin. */
type Entry = {
  key: string;
  name: string;
  description: string;
  /** Da dove viene: il server MCP o il plugin. */
  source: string;
  args: Arg[];
  text: (values: Record<string, string>) => Promise<string>;
  /** Solo se la fonte li offre: i valori suggeriti per un argomento. */
  complete?: (argument: string, value: string, values: Record<string, string>) => Promise<string[]>;
};

/** Attesa dopo l'ultimo tasto prima di chiedere i suggerimenti. */
const COMPLETION_DELAY_MS = 200;

/**
 * Comandi `/`: i prompt dei server MCP della conversazione (ADR 0025) e i comandi
 * dei plugin installati (ADR 0026). Si apre scrivendo `/` all'inizio del
 * messaggio; scelta una voce (e compilati i suoi argomenti) il suo testo prende
 * il posto del messaggio, da rivedere e inviare.
 */
export const PromptMenu = forwardRef<PromptMenuHandle, Props>(function PromptMenu(
  { conversationId, servers, draft, setDraft, onError },
  ref,
) {
  const [prompts, setPrompts] = useState<McpPrompt[] | null>(null);
  const [commands, setCommands] = useState<PluginCommandInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const [form, setForm] = useState<{ entry: Entry; values: Record<string, string> } | null>(null);
  const [busy, setBusy] = useState(false);
  // Suggerimenti per gli argomenti del modulo aperto, per nome dell'argomento.
  const [suggestions, setSuggestions] = useState<Record<string, string[]>>({});
  // L'argomento che si sta scrivendo e il suggerimento evidenziato (-1: nessuno).
  const [activeArg, setActiveArg] = useState<string | null>(null);
  const [highlight, setHighlight] = useState(-1);
  const completionTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Solo l'ultima richiesta conta: le risposte arrivate dopo una più recente si scartano.
  const completionSeq = useRef(0);

  // Il modulo di adesso, per riconoscere le risposte che arrivano dopo la sua chiusura.
  const formRef = useRef(form);
  formRef.current = form;

  const cancelCompletion = () => {
    if (completionTimer.current) clearTimeout(completionTimer.current);
    completionTimer.current = null;
    completionSeq.current += 1;
  };
  useEffect(() => cancelCompletion, []);

  // Una richiesta alla volta: con un server lento le richieste non si accodano nel
  // nucleo (dove farebbero aspettare gli strumenti); quella scritta nel frattempo
  // parte alla fine della precedente, e solo l'ultima.
  const completionBusy = useRef(false);
  const completionNext = useRef<(() => void) | null>(null);

  const requestCompletion = (entry: Entry, argument: string, values: Record<string, string>) => {
    const complete = entry.complete;
    if (!complete) return;
    cancelCompletion();
    const seq = completionSeq.current;
    const current = () => seq === completionSeq.current && formRef.current?.entry === entry;
    const run = () => {
      if (!current()) return;
      if (completionBusy.current) {
        completionNext.current = run;
        return;
      }
      completionBusy.current = true;
      complete(argument, values[argument] ?? "", values)
        .then((list) => {
          if (!current()) return;
          setSuggestions((old) => ({ ...old, [argument]: list }));
          setHighlight(-1);
        })
        // Sono solo un aiuto: se il server non risponde si scrive a mano.
        .catch(() => {})
        .finally(() => {
          completionBusy.current = false;
          const next = completionNext.current;
          completionNext.current = null;
          next?.();
        });
    };
    completionTimer.current = setTimeout(() => {
      completionTimer.current = null;
      run();
    }, COMPLETION_DELAY_MS);
  };

  // I comandi dei plugin: all'apertura e quando la finestra torna in primo piano.
  useEffect(() => {
    const load = () =>
      api
        .pluginCommands()
        .then((list) => setCommands(list ?? []))
        .catch(() => {});
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, []);

  const withServers = conversationId !== null && servers.length > 0;
  const serversKey = servers.join(",");
  const typing = (withServers || commands.length > 0) && /^\/\S*$/.test(draft);
  const query = typing ? draft.slice(1).toLowerCase() : "";

  const entries: Entry[] = [
    ...commands.map((c) => ({
      key: `plugin/${c.plugin}/${c.name}`,
      name: c.name,
      description: c.description,
      source: c.plugin,
      args: c.takesArguments
        ? [{ name: "arguments", label: t("Argomenti"), description: c.argumentHint, required: false }]
        : [],
      text: (values: Record<string, string>) => api.pluginCommandText(c.plugin, c.name, values.arguments ?? ""),
    })),
    ...(withServers ? (prompts ?? []) : []).map((p) => ({
      key: `mcp/${p.server}/${p.name}`,
      name: p.name,
      description: p.description,
      source: p.serverName,
      args: p.arguments.map((a) => ({ ...a, label: a.name })),
      text: (values: Record<string, string>) => api.conversationPrompt(conversationId ?? 0, p.server, p.name, values),
      complete: p.completions
        ? (argument: string, value: string, values: Record<string, string>) => {
            const context = Object.fromEntries(
              Object.entries(values).filter(([name, v]) => name !== argument && v.trim() !== ""),
            );
            return api.conversationPromptCompletion(conversationId ?? 0, p.server, p.name, argument, value, context);
          }
        : undefined,
    })),
  ];
  const shown = entries.filter((e) => e.name.toLowerCase().includes(query) || e.source.toLowerCase().includes(query));
  const open = typing && !dismissed && !form;
  const waiting = withServers && (loading || prompts === null);

  // Cambiando conversazione o server i prompt si rileggono alla prossima "/".
  useEffect(() => {
    setPrompts(null);
    setForm(null);
  }, [conversationId, serversKey]);

  useEffect(() => {
    if (!typing) setDismissed(false);
    setSelected(0);
  }, [typing, query]);

  useEffect(() => {
    if (!open || !withServers || prompts !== null || loading || conversationId === null) return;
    setLoading(true);
    api
      .conversationPrompts(conversationId)
      .then(setPrompts)
      .catch((e) => {
        setPrompts([]);
        onError(String(e));
      })
      .finally(() => setLoading(false));
  }, [open, withServers, prompts, loading, conversationId]);

  const fetchText = async (entry: Entry, values: Record<string, string>) => {
    setBusy(true);
    try {
      setDraft(await entry.text(values));
      setForm(null);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const choose = (entry: Entry) => {
    if (entry.args.length > 0) {
      cancelCompletion();
      setSuggestions({});
      setActiveArg(null);
      setHighlight(-1);
      setForm({ entry, values: Object.fromEntries(entry.args.map((a) => [a.name, ""])) });
    } else {
      fetchText(entry, {});
    }
  };

  useImperativeHandle(ref, () => ({
    handleKey: (e) => {
      if (!open) return false;
      if (e.key === "Escape") {
        setDismissed(true);
        return true;
      }
      if (shown.length === 0) return false;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        const step = e.key === "ArrowDown" ? 1 : -1;
        setSelected((s) => (s + step + shown.length) % shown.length);
        return true;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        choose(shown[Math.min(selected, shown.length - 1)]);
        return true;
      }
      return false;
    },
  }));

  // Il suggerimento evidenziato con le frecce resta visibile nell'elenco.
  useEffect(() => {
    if (activeArg === null || highlight < 0) return;
    document.getElementById(`prompt-arg-${activeArg}-value-${highlight}`)?.scrollIntoView?.({ block: "nearest" });
  }, [activeArg, highlight]);

  /** I suggerimenti da mostrare per un argomento: non quello già scritto per intero. */
  const options = (name: string) => {
    const value = form?.values[name] ?? "";
    return (suggestions[name] ?? []).filter((v) => v !== value);
  };

  const pick = (name: string, value: string) => {
    if (!form) return;
    cancelCompletion();
    setForm({ ...form, values: { ...form.values, [name]: value } });
    setActiveArg(null);
    setHighlight(-1);
  };

  if (form) {
    const missing = form.entry.args.some((a) => a.required && !form.values[a.name]?.trim());
    return (
      <div className="prompt-menu" role="dialog" aria-label={t("Argomenti del prompt /{name}", { name: form.entry.name })}>
        <div className="prompt-menu-title">
          <span className="mono">/{form.entry.name}</span> <span className="dim">— {form.entry.source}</span>
        </div>
        {form.entry.args.map((arg, i) => (
          <div className="field" key={arg.name}>
            <label htmlFor={`prompt-arg-${arg.name}`}>
              {arg.label}
              {arg.required && " *"}
              {arg.description && <span className="dim"> — {arg.description}</span>}
            </label>
            <input
              id={`prompt-arg-${arg.name}`}
              type="text"
              autoFocus={i === 0}
              autoComplete="off"
              role={form.entry.complete ? "combobox" : undefined}
              aria-autocomplete={form.entry.complete ? "list" : undefined}
              aria-expanded={form.entry.complete ? activeArg === arg.name && options(arg.name).length > 0 : undefined}
              aria-controls={form.entry.complete ? `prompt-arg-${arg.name}-values` : undefined}
              aria-activedescendant={
                activeArg === arg.name && highlight >= 0 && highlight < options(arg.name).length
                  ? `prompt-arg-${arg.name}-value-${highlight}`
                  : undefined
              }
              value={form.values[arg.name] ?? ""}
              onFocus={() => {
                setActiveArg(arg.name);
                setHighlight(-1);
                requestCompletion(form.entry, arg.name, form.values);
              }}
              onBlur={() => setActiveArg((a) => (a === arg.name ? null : a))}
              onChange={(e) => {
                const values = { ...form.values, [arg.name]: e.target.value };
                setForm({ ...form, values });
                setActiveArg(arg.name);
                setHighlight(-1);
                requestCompletion(form.entry, arg.name, values);
              }}
              onKeyDown={(e) => {
                const list = activeArg === arg.name ? options(arg.name) : [];
                if (list.length > 0 && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
                  e.preventDefault();
                  const step = e.key === "ArrowDown" ? 1 : -1;
                  // Da -1 (nessuno) all'ultimo e di nuovo a -1, nei due versi.
                  setHighlight((h) => {
                    const next = h + step;
                    if (next >= list.length) return -1;
                    return next < -1 ? list.length - 1 : next;
                  });
                  return;
                }
                if (e.key === "Enter" && highlight >= 0 && highlight < list.length) {
                  e.preventDefault();
                  pick(arg.name, list[highlight]);
                  return;
                }
                if (e.key === "Escape" && list.length > 0) {
                  e.preventDefault();
                  setActiveArg(null);
                  return;
                }
                if (e.key === "Enter" && !missing) fetchText(form.entry, form.values);
                if (e.key === "Escape") setForm(null);
              }}
            />
            {activeArg === arg.name && options(arg.name).length > 0 && (
              <div className="prompt-suggestions" role="listbox" id={`prompt-arg-${arg.name}-values`} aria-label={arg.label}>
                {options(arg.name).map((value, k) => (
                  <button
                    key={value}
                    id={`prompt-arg-${arg.name}-value-${k}`}
                    role="option"
                    tabIndex={-1}
                    aria-selected={k === highlight}
                    className={k === highlight ? "prompt-item selected" : "prompt-item"}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => pick(arg.name, value)}
                  >
                    {value}
                  </button>
                ))}
              </div>
            )}
          </div>
        ))}
        <div className="row">
          <button className="btn primary" disabled={missing || busy} onClick={() => fetchText(form.entry, form.values)}>
            {busy ? t("Preparazione…") : t("Inserisci")}
          </button>
          <button className="btn" onClick={() => setForm(null)}>
            {t("Annulla")}
          </button>
        </div>
      </div>
    );
  }

  if (!open) return null;
  return (
    <div className="prompt-menu" role="listbox" aria-label={t("Prompt dei server MCP")}>
      {shown.map((entry, i) => (
        <button
          key={entry.key}
          role="option"
          aria-selected={i === selected}
          className={i === selected ? "prompt-item selected" : "prompt-item"}
          disabled={busy}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => choose(entry)}
        >
          <span className="mono">/{entry.name}</span>
          {entry.description && <span className="dim"> — {entry.description}</span>}
          <span className="dim prompt-server">{entry.source}</span>
        </button>
      ))}
      {waiting && <p className="dim">{t("Avvio dei server MCP…")}</p>}
      {!waiting && shown.length === 0 && (
        <p className="dim">
          {entries.length === 0
            ? t("I server MCP di questa conversazione non offrono prompt.")
            : t("Nessun prompt con questo nome.")}
        </p>
      )}
    </div>
  );
});
