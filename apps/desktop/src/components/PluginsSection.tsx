import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, type PluginRecord } from "../api";
import { t, tc } from "../i18n";

/** Le parti di un plugin in una riga: "2 skill · 1 server MCP · …". */
function parts(p: PluginRecord): string {
  const count = (n: number, one: string, many: string) => (n === 0 ? null : n === 1 ? one : many);
  return [
    count(p.skills.length, t("1 skill"), t("{n} skill", { n: p.skills.length })),
    count(p.mcpServers.length, t("1 server MCP"), t("{n} server MCP", { n: p.mcpServers.length })),
    count(p.agents.length, t("1 agente"), t("{n} agenti", { n: p.agents.length })),
    count(p.commands.length, t("1 comando /"), t("{n} comandi /", { n: p.commands.length })),
  ]
    .filter(Boolean)
    .join(" · ");
}

/** Plugin (ADR 0026): skill, server MCP, agenti e comandi in un pacchetto. */
export function PluginsSection({ onChanged }: { onChanged?: () => void }) {
  const [plugins, setPlugins] = useState<PluginRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [installed, setInstalled] = useState<PluginRecord | null>(null);

  const refresh = () => api.listPlugins().then(setPlugins).catch((e) => setError(String(e)));
  useEffect(() => {
    refresh();
  }, []);

  const importFrom = async (directory: boolean) => {
    setError(null);
    setInstalled(null);
    try {
      const picked = await open(
        directory
          ? { directory: true, title: t("Cartella del plugin (con .claude-plugin/plugin.json)") }
          : { title: t("Plugin in un file .zip"), filters: [{ name: "Zip", extensions: ["zip"] }] },
      );
      if (typeof picked !== "string") return;
      let outcome = await api.importPlugin(picked, false);
      if (outcome.existing) {
        const question = t(
          "Il plugin «{name}» è già installato. Sostituirlo? Skill, server MCP e agenti che aveva creato verranno ricreati: le modifiche fatte a loro andranno perse.",
          { name: outcome.existing },
        );
        if (!window.confirm(question)) return;
        outcome = await api.importPlugin(picked, true);
      }
      if (outcome.installed) setInstalled(outcome.installed);
    } catch (e) {
      setError(String(e));
    }
    refresh();
    onChanged?.();
  };

  const uninstall = async (p: PluginRecord) => {
    const question = t(
      "Disinstallare il plugin «{name}»? Si eliminano anche le sue skill, i suoi server MCP (con i loro segreti) e i suoi agenti.",
      { name: p.name },
    );
    if (!window.confirm(question)) return;
    setError(null);
    setInstalled(null);
    await api.uninstallPlugin(p.name).catch((e) => setError(String(e)));
    refresh();
    onChanged?.();
  };

  return (
    <section className="section" aria-labelledby="plugins-title">
      <div className="section-head">
        <h2 id="plugins-title">{t("Plugin")}</h2>
        <span className="row">
          <button className="btn" onClick={() => importFrom(true)}>
            {t("Importa una cartella")}
          </button>
          <button className="btn" onClick={() => importFrom(false)}>
            {t("Importa un .zip")}
          </button>
        </span>
      </div>
      <p className="dim note">
        {t(
          "Pacchetti con skill, server MCP, agenti e comandi /, nel formato dei plugin di Claude Code. Installare non avvia nulla, ma un plugin si fida come un programma: i suoi server MCP eseguono codice quando li scegli. Installa solo plugin di cui ti fidi.",
        )}
      </p>

      {installed && (
        <div className="note" role="status">
          <p>{t("Plugin «{name}» installato: {parts}.", { name: installed.name, parts: parts(installed) || t("nessuna parte") })}</p>
          {installed.agents.length > 0 && (
            <p className="dim">{t("Scegli le cartelle dei suoi agenti nella pagina Agenti prima di usarli.")}</p>
          )}
          {installed.notes.length > 0 && (
            <ul>
              {installed.notes.map((n) => (
                <li key={n}>{tc(n)}</li>
              ))}
            </ul>
          )}
        </div>
      )}

      {plugins.length === 0 && <p className="dim">{t("Nessun plugin.")}</p>}
      {plugins.map((p) => (
        <div key={p.name} className="folder plugin-row">
          <details>
            <summary>
              <strong className="mono">{p.name}</strong>
              {p.version && <span className="dim"> {p.version}</span>}
              {p.description && <span className="dim"> — {p.description}</span>}
              <span className="dim"> ({parts(p) || t("nessuna parte")})</span>
            </summary>
            <div className="plugin-parts">
              {p.author && <p className="dim">{t("Autore: {name}", { name: p.author })}</p>}
              {p.skills.length > 0 && <p>{t("Skill: {list}", { list: p.skills.join(", ") })}</p>}
              {p.commands.length > 0 && (
                <p>{t("Comandi: {list}", { list: p.commands.map((c) => `/${c.name}`).join(", ") })}</p>
              )}
              {p.notes.length > 0 && (
                <ul className="dim">
                  {p.notes.map((n) => (
                    <li key={n}>{tc(n)}</li>
                  ))}
                </ul>
              )}
            </div>
          </details>
          <button className="btn-link" aria-label={t("Disinstalla {name}", { name: p.name })} onClick={() => uninstall(p)}>
            {t("Disinstalla")}
          </button>
        </div>
      ))}
      {error && (
        <p className="error pre" role="alert">
          {tc(error)}
        </p>
      )}
    </section>
  );
}
