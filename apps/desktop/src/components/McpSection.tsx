import { useEffect, useState } from "react";
import { api, type McpServer, type McpToolInfo } from "../api";
import { msg, t, tc } from "../i18n";

const EMPTY: McpServer = {
  id: "",
  name: "",
  transport: "stdio",
  command: "",
  args: [],
  env: [],
  url: "",
  headers: [],
};

/**
 * Server noti, da aggiungere con un clic: il modulo si riempie e l'utente
 * completa ciò che serve (una cartella, un token) prima di salvare.
 */
type CatalogEntry = {
  name: string;
  /** Che cosa fa, in italiano: passa da `t()`. */
  what: string;
  server: Partial<McpServer>;
  /** Cosa deve completare l'utente, se serve. */
  todo?: string;
};

const CATALOG: CatalogEntry[] = [
  {
    name: "Filesystem",
    what: msg("Legge e scrive i file di una cartella."),
    server: { command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "C:\\percorso\\della\\cartella"] },
    todo: msg("Sostituisci l'ultimo argomento con la cartella da usare. Serve Node.js."),
  },
  {
    name: "Memory",
    what: msg("Una memoria a grafo che l'agente conserva tra una sessione e l'altra."),
    server: { command: "npx", args: ["-y", "@modelcontextprotocol/server-memory"] },
    todo: msg("Serve Node.js."),
  },
  {
    name: "Sequential thinking",
    what: msg("Aiuta il modello a ragionare per passi sui problemi complessi."),
    server: { command: "npx", args: ["-y", "@modelcontextprotocol/server-sequential-thinking"] },
    todo: msg("Serve Node.js."),
  },
  {
    name: "Fetch",
    what: msg("Scarica pagine web e le converte in Markdown."),
    server: { command: "uvx", args: ["mcp-server-fetch"] },
    todo: msg("Serve uv (Python)."),
  },
  {
    name: "Git",
    what: msg("Legge la storia e lo stato di un repository git."),
    server: { command: "uvx", args: ["mcp-server-git", "--repository", "C:\\percorso\\del\\repository"] },
    todo: msg("Sostituisci l'ultimo argomento con il repository. Serve uv (Python)."),
  },
  {
    name: "Playwright",
    what: msg("Apre un browser e naviga le pagine per conto dell'agente."),
    server: { command: "npx", args: ["-y", "@playwright/mcp@latest"] },
    todo: msg("Serve Node.js."),
  },
  {
    name: "GitHub",
    what: msg("Issue, pull request e codice dei tuoi repository GitHub (server remoto)."),
    server: {
      transport: "http",
      url: "https://api.githubcopilot.com/mcp/",
      headers: [["Authorization", "Bearer "]],
    },
    todo: msg("Completa l'intestazione Authorization con un tuo token di accesso personale di GitHub."),
  },
];

/** Testo del modulo: argomenti uno per riga, variabili e intestazioni come NOME=valore. */
type Draft = { server: McpServer; args: string; env: string; headers: string; todo?: string };

const pairs = (list: [string, string][], sep: string) => list.map(([name, value]) => `${name}${sep}${value}`).join("\n");

const toDraft = (server: McpServer, todo?: string): Draft => ({
  server,
  args: server.args.join("\n"),
  env: pairs(server.env, "="),
  headers: pairs(server.headers ?? [], ": "),
  todo,
});

function fromDraft(draft: Draft): McpServer {
  const lines = (text: string) =>
    text
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter(Boolean);
  const split = (sep: string) => (line: string): [string, string] => {
    const at = line.indexOf(sep);
    return at < 0 ? [line, ""] : [line.slice(0, at).trim(), line.slice(at + 1).trim()];
  };
  const http = draft.server.transport === "http";
  return {
    ...draft.server,
    args: http ? [] : lines(draft.args),
    env: http ? [] : lines(draft.env).map(split("=")),
    command: http ? "" : draft.server.command,
    url: http ? draft.server.url.trim() : "",
    headers: http ? lines(draft.headers).map(split(":")) : [],
  };
}

const describe = (s: McpServer) => (s.transport === "http" ? s.url : `${s.command} ${s.args.join(" ")}`);

/** Server MCP: programmi esterni che offrono strumenti agli agenti. */
export function McpSection() {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [catalog, setCatalog] = useState(false);
  const [probe, setProbe] = useState<{ busy: boolean; tools: McpToolInfo[] | null; error: string | null }>({
    busy: false,
    tools: null,
    error: null,
  });
  const [error, setError] = useState<string | null>(null);

  const refresh = () => api.listMcpServers().then(setServers).catch((e) => setError(String(e)));
  useEffect(() => {
    refresh();
  }, []);

  const edit = (server: McpServer, todo?: string) => {
    setDraft(toDraft({ ...EMPTY, ...server }, todo));
    setCatalog(false);
    setProbe({ busy: false, tools: null, error: null });
    setError(null);
  };

  const save = async () => {
    if (!draft) return;
    try {
      await api.saveMcpServer(fromDraft(draft));
      setDraft(null);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const remove = async (server: McpServer) => {
    const question = t("Eliminare il server MCP «{name}»? Gli agenti che lo usano non lo troveranno più.", {
      name: server.name,
    });
    if (!window.confirm(question)) return;
    await api.deleteMcpServer(server.id).catch((e) => setError(String(e)));
    refresh();
  };

  const test = async () => {
    if (!draft) return;
    setProbe({ busy: true, tools: null, error: null });
    try {
      setProbe({ busy: false, tools: await api.probeMcpServer(fromDraft(draft)), error: null });
    } catch (e) {
      setProbe({ busy: false, tools: null, error: String(e) });
    }
  };

  const setServer = (change: Partial<McpServer>) => draft && setDraft({ ...draft, server: { ...draft.server, ...change } });
  const http = draft?.server.transport === "http";

  return (
    <section className="section" aria-labelledby="mcp-title">
      <div className="section-head">
        <h2 id="mcp-title">{t("Server MCP")}</h2>
        {!draft && (
          <span className="row">
            <button className="btn" aria-expanded={catalog} onClick={() => setCatalog(!catalog)}>
              {t("Server noti")}
            </button>
            <button className="btn" onClick={() => edit(EMPTY)}>
              {t("Aggiungi server")}
            </button>
          </span>
        )}
      </div>
      <p className="dim note">
        {t(
          "Programmi che aggiungono strumenti agli agenti (Model Context Protocol). Partono sul tuo computer con i tuoi permessi: aggiungi solo server di cui ti fidi. Ogni agente sceglie quali usare, e i loro strumenti chiedono conferma salvo che l'agente abbia «Consenti tutto».",
        )}
      </p>

      {catalog && !draft && (
        <ul className="mcp-catalog" aria-label={t("Server noti")}>
          {CATALOG.map((entry) => (
            <li key={entry.name} className="folder">
              <span>
                <strong>{entry.name}</strong> <span className="dim">— {t(entry.what)}</span>
              </span>
              <button
                className="btn-link"
                aria-label={t("Aggiungi {name}", { name: entry.name })}
                onClick={() => edit({ ...EMPTY, name: entry.name, ...entry.server }, entry.todo)}
              >
                {t("Aggiungi")}
              </button>
            </li>
          ))}
        </ul>
      )}

      {servers.length === 0 && !draft && <p className="dim">{t("Nessun server MCP.")}</p>}
      {servers.map((s) => (
        <div key={s.id} className="folder">
          <span>
            <strong>{s.name}</strong> <span className="mono dim">{describe(s)}</span>
          </span>
          <span className="row">
            <button className="btn-link" aria-label={t("Modifica {name}", { name: s.name })} onClick={() => edit(s)}>
              {t("Modifica")}
            </button>
            <button className="btn-link" aria-label={t("Elimina {name}", { name: s.name })} onClick={() => remove(s)}>
              {t("Elimina")}
            </button>
          </span>
        </div>
      ))}

      {draft && (
        <div className="card mcp-form">
          {draft.todo && (
            <p className="note" role="status">
              {t(draft.todo)}
            </p>
          )}
          <div className="field">
            <label htmlFor="mcp-name">{t("Nome")}</label>
            <input
              id="mcp-name"
              type="text"
              value={draft.server.name}
              placeholder={t("Per esempio: GitHub")}
              onChange={(e) => setServer({ name: e.target.value })}
            />
          </div>
          <fieldset className="field">
            <legend>{t("Collegamento")}</legend>
            <label className="check">
              <input
                type="radio"
                name="mcp-transport"
                checked={!http}
                onChange={() => setServer({ transport: "stdio" })}
              />
              <span>{t("Programma su questo computer (stdio)")}</span>
            </label>
            <label className="check">
              <input type="radio" name="mcp-transport" checked={http} onChange={() => setServer({ transport: "http" })} />
              <span>{t("Server a un indirizzo (HTTP)")}</span>
            </label>
          </fieldset>
          {http ? (
            <>
              <div className="field">
                <label htmlFor="mcp-url">{t("Indirizzo")}</label>
                <input
                  id="mcp-url"
                  type="url"
                  className="mono"
                  value={draft.server.url}
                  placeholder="https://esempio.it/mcp"
                  onChange={(e) => setServer({ url: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="mcp-headers">{t("Intestazioni, una per riga (Nome: valore)")}</label>
                <textarea
                  id="mcp-headers"
                  className="mono"
                  rows={2}
                  value={draft.headers}
                  placeholder="Authorization: Bearer …"
                  onChange={(e) => setDraft({ ...draft, headers: e.target.value })}
                  aria-describedby="mcp-secret-note"
                />
              </div>
            </>
          ) : (
            <>
              <div className="field">
                <label htmlFor="mcp-command">{t("Comando")}</label>
                <input
                  id="mcp-command"
                  type="text"
                  className="mono"
                  value={draft.server.command}
                  placeholder={t("npx, uvx, oppure il percorso di un programma")}
                  onChange={(e) => setServer({ command: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="mcp-args">{t("Argomenti, uno per riga")}</label>
                <textarea
                  id="mcp-args"
                  className="mono"
                  rows={3}
                  value={draft.args}
                  placeholder={"-y\n@nome/server-mcp"}
                  onChange={(e) => setDraft({ ...draft, args: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="mcp-env">{t("Variabili d'ambiente, una per riga (NOME=valore)")}</label>
                <textarea
                  id="mcp-env"
                  className="mono"
                  rows={2}
                  value={draft.env}
                  onChange={(e) => setDraft({ ...draft, env: e.target.value })}
                  aria-describedby="mcp-secret-note"
                />
              </div>
            </>
          )}
          <p className="dim note" id="mcp-secret-note">
            {t(
              "I valori vengono salvati nel Gestore credenziali di Windows, come le chiavi API: nell'archivio dell'app restano solo i nomi.",
            )}
          </p>
          <div className="row">
            <button className="btn primary" onClick={save}>
              {t("Salva")}
            </button>
            <button className="btn" disabled={probe.busy} onClick={test}>
              {probe.busy ? t("Avvio in corso…") : t("Prova")}
            </button>
            <button className="btn" onClick={() => setDraft(null)}>
              {t("Annulla")}
            </button>
          </div>
          <div aria-live="polite">
            {probe.error && (
              <p className="error pre" role="alert">
                {tc(probe.error)}
              </p>
            )}
            {probe.tools && (
              <div className="note">
                <p>
                  {probe.tools.length === 1
                    ? t("Il server risponde e offre 1 strumento:")
                    : t("Il server risponde e offre {n} strumenti:", { n: probe.tools.length })}
                </p>
                <ul className="mcp-tools">
                  {probe.tools.map((tool) => (
                    <li key={tool.name}>
                      <span className="mono">{tool.name}</span>
                      {tool.description && <span className="dim"> — {tc(tool.description)}</span>}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        </div>
      )}
      {error && (
        <p className="error pre" role="alert">
          {tc(error)}
        </p>
      )}
    </section>
  );
}
