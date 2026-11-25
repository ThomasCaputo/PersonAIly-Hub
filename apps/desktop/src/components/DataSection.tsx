import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { t, tc } from "../i18n";

const ZIP_FILTER = [{ name: "Backup", extensions: ["zip"] }];

/** Data di oggi per il nome del file, `2026-11-20`. */
function today(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Impostazioni → Backup dei dati e Segnalare un problema. */
export function DataSection() {
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async (work: () => Promise<string | undefined>) => {
    setMessage(null);
    setError(null);
    setBusy(true);
    try {
      const done = await work();
      if (done) setMessage(done);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const backup = () =>
    run(async () => {
      const path = await save({ defaultPath: `personaily-hub-backup-${today()}.zip`, filters: ZIP_FILTER });
      if (!path) return;
      await api.createBackup(path);
      return t("Backup salvato in {path}", { path });
    });

  const restore = () =>
    run(async () => {
      const path = await open({ filters: ZIP_FILTER, title: t("Ripristina da un backup") });
      if (typeof path !== "string") return;
      const ok = confirm(
        t(
          "Sostituire i dati attuali con quelli del backup?\n\n{path}\n\nL'app si riavvia: gli agenti al lavoro si fermano e il modello locale viene scaricato. I dati di adesso restano in una copia nella cartella backups.",
          { path },
        ),
      );
      if (!ok) return;
      await api.restoreBackup(path);
      return t("Riavvio in corso…");
    });

  const copyDiagnostics = () =>
    run(async () => {
      await navigator.clipboard.writeText(await api.diagnostics());
      return t("Informazioni copiate: incollale nella segnalazione.");
    });

  return (
    <>
      <section className="section">
        <h2>{t("Backup dei dati")}</h2>
        <p className="dim note">
          {t(
            "Il backup contiene conversazioni, agenti, impostazioni, preset, skill e plugin. Non contiene le chiavi API e le variabili dei server MCP, che restano nel Gestore credenziali di Windows, né i modelli e llama.cpp: su un altro computer vanno inseriti e scaricati di nuovo.",
          )}
        </p>
        <div className="row">
          <button className="btn" onClick={backup} disabled={busy}>
            {t("Crea un backup…")}
          </button>
          <button className="btn" onClick={restore} disabled={busy}>
            {t("Ripristina da un backup…")}
          </button>
        </div>
      </section>

      <section className="section">
        <h2>{t("Segnalare un problema")}</h2>
        <p className="dim note">
          {t(
            "Le informazioni diagnostiche dicono versione, sistema, hardware, modello caricato e le ultime righe del registro. Non contengono chiavi, token, conversazioni né nomi di cartelle; il tuo nome utente viene tolto. Escono dal computer solo se le incolli tu da qualche parte.",
          )}
        </p>
        <div className="row">
          <button className="btn" onClick={copyDiagnostics} disabled={busy}>
            {t("Copia le informazioni diagnostiche")}
          </button>
          <button className="btn" onClick={() => run(async () => void (await api.openLogsFolder()))} disabled={busy}>
            {t("Apri la cartella dei registri")}
          </button>
        </div>
      </section>

      {message && <p className="dim note pre">{message}</p>}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </>
  );
}
