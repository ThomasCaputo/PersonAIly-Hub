import { useEffect, useState } from "react";
import { api, size, type Preferences, type UpdateInfo } from "../api";
import { t, tc } from "../i18n";

export const RELEASES_URL = "https://github.com/ThomasCaputo/personaily-hub/releases/latest";

/**
 * Scarica e installa un aggiornamento, oppure (exe portatile) apre la pagina
 * delle release. L'installatore chiude l'app e la riapre aggiornata.
 */
export function UpdateOffer({ update }: { update: UpdateInfo }) {
  const [progress, setProgress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const install = async () => {
    setError(null);
    setProgress(t("Download dell'aggiornamento…"));
    try {
      await api.installUpdate((p) =>
        setProgress(
          p.totalBytes
            ? t("Download dell'aggiornamento: {done} di {total}", {
                done: size(p.doneBytes),
                total: size(p.totalBytes),
              })
            : t("Download dell'aggiornamento: {done}", { done: size(p.doneBytes) }),
        ),
      );
      setProgress(t("Installazione: l'app si chiude e si riapre aggiornata."));
    } catch (e) {
      setProgress(null);
      setError(String(e));
    }
  };

  return (
    <div className="update-offer">
      <p>
        <strong>{t("È disponibile la versione {version}.", { version: update.version })}</strong>{" "}
        <span className="dim">{t("Ora hai la {version}.", { version: update.currentVersion })}</span>
      </p>
      {update.notes && (
        <details>
          <summary>{t("Novità")}</summary>
          <p className="pre note">{update.notes}</p>
        </details>
      )}
      {update.installable ? (
        <button className="btn primary" onClick={install} disabled={progress != null}>
          {t("Installa e riavvia")}
        </button>
      ) : (
        <>
          <p className="dim note">{t("Questa è la versione portatile: scarica la nuova e sostituisci il file.")}</p>
          <button className="btn" onClick={() => api.openExternal(RELEASES_URL)}>
            {t("Apri la pagina delle release")}
          </button>
        </>
      )}
      {progress && <p className="dim note">{progress}</p>}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </div>
  );
}

/** Impostazioni → Aggiornamenti. */
export function UpdateSection({
  preferences,
  onPreferences,
}: {
  preferences: Preferences;
  onPreferences: (change: Partial<Preferences>) => void;
}) {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    api.coreInfo().then((info) => setVersion(info.version)).catch(() => {});
  }, []);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [state, setState] = useState<"idle" | "checking" | "current">("idle");
  const [error, setError] = useState<string | null>(null);

  const check = async () => {
    setError(null);
    setState("checking");
    try {
      const found = await api.checkUpdate();
      setUpdate(found);
      setState(found ? "idle" : "current");
    } catch (e) {
      setState("idle");
      setError(String(e));
    }
  };

  return (
    <section className="section">
      <h2>{t("Aggiornamenti")}</h2>
      {version && <p>{t("Versione installata: {version}", { version })}</p>}
      <label className="check">
        <input
          type="checkbox"
          checked={preferences.checkUpdates}
          onChange={(e) => onPreferences({ checkUpdates: e.target.checked })}
        />
        <span>
          <strong>{t("Controlla gli aggiornamenti all'avvio")}</strong>
          <span className="dim note block">
            {t(
              "L'app chiede a GitHub qual è l'ultima versione pubblicata: non manda nulla di tuo. Gli aggiornamenti sono firmati e l'app rifiuta quelli con una firma non valida.",
            )}
          </span>
        </span>
      </label>
      <div className="row">
        <button className="btn" onClick={check} disabled={state === "checking"}>
          {state === "checking" ? t("Controllo…") : t("Controlla ora")}
        </button>
      </div>
      {state === "current" && <p className="dim note">{t("Hai già l'ultima versione.")}</p>}
      {update && <UpdateOffer update={update} />}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </section>
  );
}
