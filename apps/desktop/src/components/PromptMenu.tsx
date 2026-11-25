import { forwardRef, useEffect, useImperativeHandle, useState, type KeyboardEvent } from "react";
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
};

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
              value={form.values[arg.name] ?? ""}
              onChange={(e) => setForm({ ...form, values: { ...form.values, [arg.name]: e.target.value } })}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !missing) fetchText(form.entry, form.values);
                if (e.key === "Escape") setForm(null);
              }}
            />
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
