import { useEffect, useState } from "react";
import { api, type ApiInfo } from "../api";

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
    if (!confirm("Creare un token nuovo? I programmi configurati con quello attuale smetteranno di funzionare.")) return;
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
      <h2>API locale</h2>
      <p className="dim note">
        L'estensione per VS Code e i programmi compatibili con OpenAI usano i modelli dell'app tramite questo
        indirizzo, raggiungibile solo da questo computer. Come chiave API si usa il token. Nome del modello:{" "}
        <span className="mono">local</span> per quello locale caricato, <span className="mono">provider/modello</span>{" "}
        per quelli cloud.
      </p>
      {!info && <p className="dim">Avvio in corso…</p>}
      {info?.error && <p className="error pre">L'API locale non è attiva: {info.error}</p>}
      {info && !info.running && !info.error && <p className="dim">Avvio in corso…</p>}
      {address && info?.token && (
        <div className="api-grid">
          <span className="dim">Indirizzo</span>
          <span className="mono">{address}</span>
          <button className="btn-link" onClick={() => copy("address", address)}>
            {copied === "address" ? "Copiato" : "Copia"}
          </button>

          <span className="dim">Token</span>
          <span className="mono api-token">{showToken ? info.token : "•".repeat(24)}</span>
          <span className="api-actions">
            <button className="btn-link" onClick={() => setShowToken((v) => !v)}>
              {showToken ? "Nascondi" : "Mostra"}
            </button>
            <button className="btn-link" onClick={() => copy("token", info.token!)}>
              {copied === "token" ? "Copiato" : "Copia"}
            </button>
            <button className="btn-link" onClick={regenerate}>
              Rigenera
            </button>
          </span>
        </div>
      )}
      {error && <p className="error pre">{error}</p>}
    </section>
  );
}
