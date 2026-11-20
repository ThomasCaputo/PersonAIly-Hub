import { useEffect, useState } from "react";
import { api, type ApiInfo } from "../api";
import { t, tc, tn } from "../i18n";

/** Indirizzo e token dell'API locale, per VS Code e per i programmi compatibili con OpenAI. */
export function ApiSection() {
  const [info, setInfo] = useState<ApiInfo | null>(null);
  const [showToken, setShowToken] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stop = false;
    // L'API parte insieme all'app, in sottofondo: si riprova finché non è pronta.
    const refresh = async (attempt: number) => {
      try {
        const next = await api.apiInfo();
        if (stop) return;
        setInfo(next);
        if (!next.running && !next.error && attempt < 20) setTimeout(() => refresh(attempt + 1), 500);
      } catch (e) {
        if (!stop) setError(String(e));
      }
    };
    refresh(0);
    return () => {
      stop = true;
    };
  }, []);

  const copy = async (label: string, text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(label);
    setTimeout(() => setCopied(null), 1500);
  };

  const regenerate = async () => {
    if (!confirm(t("Creare un token nuovo? I programmi configurati con quello attuale smetteranno di funzionare."))) return;
    try {
      const token = await api.regenerateApiToken();
      setInfo((current) => (current ? { ...current, token } : current));
      setShowToken(true);
    } catch (e) {
      setError(String(e));
    }
  };

  const address = info?.port ? `http://127.0.0.1:${info.port}/v1` : null;

  return (
    <section className="section">
      <h2>{t("API locale")}</h2>
      <p className="dim note">
        {tn(
          "L'estensione per VS Code e i programmi compatibili con OpenAI usano i modelli dell'app tramite questo indirizzo, raggiungibile solo da questo computer. Come chiave API si usa il token. Nome del modello: {local} per quello locale caricato, {cloud} per quelli cloud.",
          {
            local: <span className="mono">local</span>,
            cloud: <span className="mono">{t("provider/modello")}</span>,
          },
        )}
      </p>
      <p className="dim note">
        {t(
          "Chi conosce il token può usare tutto ciò che l'app sa fare, agenti e terminale compresi: non condividerlo. Il token è salvato anche nel file core.lock della cartella dati, leggibile solo dal tuo utente di Windows.",
        )}
      </p>
      {!info && <p className="dim">{t("Avvio in corso…")}</p>}
      {info?.error && <p className="error pre" role="alert">{t("L'API locale non è attiva: {error}", { error: tc(info.error) })}</p>}
      {info && !info.running && !info.error && <p className="dim">{t("Avvio in corso…")}</p>}
      {address && info?.token && (
        <div className="api-grid">
          <span className="dim">{t("Indirizzo")}</span>
          <span className="mono">{address}</span>
          <button className="btn-link" onClick={() => copy("address", address)}>
            {copied === "address" ? t("Copiato") : t("Copia")}
          </button>

          <span className="dim">{t("Token")}</span>
          <span className="mono api-token">{showToken ? info.token : "•".repeat(24)}</span>
          <span className="api-actions">
            <button className="btn-link" onClick={() => setShowToken((v) => !v)}>
              {showToken ? t("Nascondi") : t("Mostra")}
            </button>
            <button className="btn-link" onClick={() => copy("token", info.token!)}>
              {copied === "token" ? t("Copiato") : t("Copia")}
            </button>
            <button className="btn-link" onClick={regenerate}>
              {t("Rigenera")}
            </button>
          </span>
        </div>
      )}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </section>
  );
}
