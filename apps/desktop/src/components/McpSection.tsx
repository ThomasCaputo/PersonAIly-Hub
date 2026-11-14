import { useEffect, useState } from "react";
import { api, type McpServer, type McpToolInfo } from "../api";
import { t, tc } from "../i18n";

const EMPTY: McpServer = { id: "", name: "", command: "", args: [], env: [] };

/** Testo del modulo: argomenti uno per riga, variabili come NOME=valore. */
type Draft = { server: McpServer; args: string; env: string };

const toDraft = (server: McpServer): Draft => ({
  server,
  args: server.args.join("\n"),
  env: server.env.map(([name, value]) => `${name}=${value}`).join("\n"),
});

function fromDraft(draft: Draft): McpServer {
  const lines = (text: string) =>
    text
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter(Boolean);
  return {
    ...draft.server,
    args: lines(draft.args),
    env: lines(draft.env).map((line): [string, string] => {
      const at = line.indexOf("=");
      return at < 0 ? [line, ""] : [line.slice(0, at).trim(), line.slice(at + 1)];
    }),
  };
}

/** Server MCP: programmi esterni che offrono strumenti agli agenti. */
export function McpSection() {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
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

  const edit = (server: McpServer) => {
    setDraft(toDraft(server));
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

  return (
    <section className="section">
      <div className="section-head">
        <h2>{t("Server MCP")}</h2>
        {!draft && (
          <button className="btn" onClick={() => edit(EMPTY)}>
            {t("Aggiungi server")}
          </button>
        )}
      </div>
      <p className="dim note">
        {t(
          "Programmi che aggiungono strumenti agli agenti (Model Context Protocol). Partono sul tuo computer con i tuoi permessi: aggiungi solo server di cui ti fidi. Ogni agente sceglie quali usare, e i loro strumenti chiedono conferma salvo che l'agente abbia «Consenti tutto».",
        )}
      </p>

      {servers.length === 0 && !draft && <p className="dim">{t("Nessun server MCP.")}</p>}
      {servers.map((s) => (
        <div key={s.id} className="folder">
          <span>
            <strong>{s.name}</strong>{" "}
            <span className="mono dim">
              {s.command} {s.args.join(" ")}
            </span>
          </span>
          <span className="row">
            <button className="btn-link" onClick={() => edit(s)}>
              {t("Modifica")}
            </button>
            <button className="btn-link" onClick={() => remove(s)}>
              {t("Elimina")}
            </button>
          </span>
        </div>
      ))}

      {draft && (
        <div className="card mcp-form">
          <div className="field">
            <label htmlFor="mcp-name">{t("Nome")}</label>
            <input
              id="mcp-name"
              type="text"
              value={draft.server.name}
              placeholder={t("Per esempio: GitHub")}
              onChange={(e) => setDraft({ ...draft, server: { ...draft.server, name: e.target.value } })}
            />
          </div>
          <div className="field">
            <label htmlFor="mcp-command">{t("Comando")}</label>
            <input
              id="mcp-command"
              type="text"
              className="mono"
              value={draft.server.command}
              placeholder={t("npx, uvx, oppure il percorso di un programma")}
              onChange={(e) => setDraft({ ...draft, server: { ...draft.server, command: e.target.value } })}
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
            />
            <p className="dim note">
              {t(
                "I valori vengono salvati nel Gestore credenziali di Windows, come le chiavi API: nell'archivio dell'app restano solo i nomi.",
              )}
            </p>
          </div>
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
          {probe.error && <p className="error pre">{tc(probe.error)}</p>}
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
                    {tool.description && <span className="dim"> — {tool.description}</span>}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
      {error && <p className="error pre">{tc(error)}</p>}
    </section>

  );
}
