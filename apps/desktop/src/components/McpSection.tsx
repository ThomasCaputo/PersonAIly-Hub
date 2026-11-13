import { useEffect, useState } from "react";
import { api, type McpServer, type McpToolInfo } from "../api";

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
    if (!window.confirm(`Eliminare il server MCP «${server.name}»? Gli agenti che lo usano non lo troveranno più.`)) return;
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
        <h2>Server MCP</h2>
        {!draft && (
          <button className="btn" onClick={() => edit(EMPTY)}>
            Aggiungi server
          </button>
        )}
      </div>
      <p className="dim note">
        Programmi che aggiungono strumenti agli agenti (Model Context Protocol). Partono sul tuo computer con i tuoi
        permessi: aggiungi solo server di cui ti fidi. Ogni agente sceglie quali usare, e i loro strumenti chiedono
        conferma salvo che l'agente abbia «Consenti tutto».
      </p>

      {servers.length === 0 && !draft && <p className="dim">Nessun server MCP.</p>}
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
              Modifica
            </button>
            <button className="btn-link" onClick={() => remove(s)}>
              Elimina
            </button>
          </span>
        </div>
      ))}

      {draft && (
        <div className="card mcp-form">
          <div className="field">
            <label htmlFor="mcp-name">Nome</label>
            <input
              id="mcp-name"
              type="text"
              value={draft.server.name}
              placeholder="Per esempio: GitHub"
              onChange={(e) => setDraft({ ...draft, server: { ...draft.server, name: e.target.value } })}
            />
          </div>
          <div className="field">
            <label htmlFor="mcp-command">Comando</label>
            <input
              id="mcp-command"
              type="text"
              className="mono"
              value={draft.server.command}
              placeholder="npx, uvx, oppure il percorso di un programma"
              onChange={(e) => setDraft({ ...draft, server: { ...draft.server, command: e.target.value } })}
            />
          </div>
          <div className="field">
            <label htmlFor="mcp-args">Argomenti, uno per riga</label>
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
            <label htmlFor="mcp-env">Variabili d'ambiente, una per riga (NOME=valore)</label>
            <textarea
              id="mcp-env"
              className="mono"
              rows={2}
              value={draft.env}
              onChange={(e) => setDraft({ ...draft, env: e.target.value })}
            />
            <p className="dim note">Vengono salvate nell'archivio dell'app, non nel Gestore credenziali.</p>
          </div>
          <div className="row">
            <button className="btn primary" onClick={save}>
              Salva
            </button>
            <button className="btn" disabled={probe.busy} onClick={test}>
              {probe.busy ? "Avvio in corso…" : "Prova"}
            </button>
            <button className="btn" onClick={() => setDraft(null)}>
              Annulla
            </button>
          </div>
          {probe.error && <p className="error pre">{probe.error}</p>}
          {probe.tools && (
            <div className="note">
              <p>
                Il server risponde e offre {probe.tools.length}{" "}
                {probe.tools.length === 1 ? "strumento" : "strumenti"}:
              </p>
              <ul className="mcp-tools">
                {probe.tools.map((t) => (
                  <li key={t.name}>
                    <span className="mono">{t.name}</span>
                    {t.description && <span className="dim"> — {t.description}</span>}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
      {error && <p className="error pre">{error}</p>}
    </section>
  );
}
