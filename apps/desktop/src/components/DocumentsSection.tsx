import { useCallback, useEffect, useState } from "react";
import { api, type ModelInfo, type RagStatus } from "../api";
import { t, tc } from "../i18n";

/** Ogni quanto si rilegge l'avanzamento mentre si indicizza. */
const POLL_MS = 1000;

/**
 * Impostazioni → Documenti consultabili (RAG locale): modello di embedding,
 * cartelle da indicizzare, aggiornamento dell'indice e una ricerca di prova.
 */
export function DocumentsSection({ grantedFolders }: { grantedFolders: string[] }) {
  const [status, setStatus] = useState<RagStatus | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<[string, string, number][] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api
      .ragStatus()
      .then(setStatus)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    api
      .listModels()
      .then(setModels)
      .catch(() => {});
  }, [refresh]);

  // Mentre si indicizza, l'avanzamento si aggiorna da solo.
  useEffect(() => {
    if (!status?.indexing) return;
    const timer = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(timer);
  }, [status?.indexing, refresh]);

  if (!status) return null;
  const settings = status.settings;

  const save = async (change: Partial<RagStatus["settings"]>) => {
    setError(null);
    try {
      await api.setRagSettings({ ...settings, ...change });
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const index = async () => {
    setError(null);
    await api.indexDocuments().catch((e) => setError(String(e)));
    // Il nucleo segna subito "in corso": lo si legge per far partire l'aggiornamento.
    window.setTimeout(refresh, 200);
  };

  const search = async () => {
    if (!query.trim()) return;
    setError(null);
    setResults(null);
    try {
      setResults(await api.searchDocuments(query.trim()));
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleFolder = (folder: string, on: boolean) =>
    save({ folders: on ? [...settings.folders, folder] : settings.folders.filter((f) => f !== folder) });

  return (
    <section className="section">
      <h2>{t("Documenti consultabili")}</h2>
      <p className="dim note">
        {t(
          "I modelli possono cercare nei tuoi documenti (testo, Markdown, PDF, codice) con lo strumento «Documenti» della chat e degli agenti. Tutto resta sul computer: i file vengono letti da un modello di embedding locale, che gira sul processore e si chiude da solo dopo qualche minuto.",
        )}
      </p>

      <div className="field">
        <label htmlFor="rag-model">{t("Modello di embedding")}</label>
        <select
          id="rag-model"
          value={settings.embeddingModel ?? ""}
          disabled={status.indexing}
          onChange={(e) => save({ embeddingModel: e.target.value || null })}
        >
          <option value="">{t("Nessuno")}</option>
          {models.map((m) => (
            <option key={m.path} value={m.path}>
              {m.name}
            </option>
          ))}
        </select>
        <p className="dim note">
          {t(
            "Serve un modello GGUF fatto per gli embedding, per esempio nomic-embed-text o multilingual-e5 (si scaricano dalla pagina Modelli). Cambiandolo, l'indice si rifà da capo.",
          )}
        </p>
      </div>

      <div className="field">
        <label>{t("Cartelle da indicizzare")}</label>
        {grantedFolders.length === 0 ? (
          <p className="dim note">{t("Concedi prima una cartella ai modelli, qui sopra.")}</p>
        ) : (
          <div className="tools-bar">
            {grantedFolders.map((folder) => (
              <label key={folder} className="mono">
                <input
                  type="checkbox"
                  checked={settings.folders.includes(folder)}
                  disabled={status.indexing}
                  onChange={(e) => toggleFolder(folder, e.target.checked)}
                />
                {folder}
              </label>
            ))}
          </div>
        )}
      </div>

      <div className="row">
        <button
          className="btn"
          disabled={status.indexing || !settings.embeddingModel || settings.folders.length === 0}
          onClick={index}
        >
          {status.files > 0 ? t("Aggiorna l'indice") : t("Crea l'indice")}
        </button>
        <span className="dim">
          {status.indexing
            ? t("Indicizzazione: {done} di {total} file…", { done: status.doneFiles, total: status.totalFiles })
            : t("{files} file, {chunks} brani nell'indice", { files: status.files, chunks: status.chunks })}
        </span>
      </div>
      {status.indexing && status.totalFiles > 0 && (
        <div className="meter-track spend-meter" aria-hidden>
          <div className="meter-fill" style={{ width: `${(status.doneFiles / status.totalFiles) * 100}%` }} />
        </div>
      )}
      {status.error && <p className="error pre" role="alert">{tc(status.error)}</p>}
      {status.skipped.length > 0 && (
        <p className="dim note">
          {t("File non leggibili, saltati: {list}", { list: status.skipped.slice(0, 5).join(", ") })}
          {status.skipped.length > 5 && " …"}
        </p>
      )}

      {status.chunks > 0 && (
        <div className="field rag-search">
          <label htmlFor="rag-query">{t("Prova una ricerca")}</label>
          <div className="row">
            <input
              id="rag-query"
              value={query}
              placeholder={t("Che cosa cerchi nei tuoi documenti?")}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && search()}
            />
            <button className="btn" disabled={!query.trim()} onClick={search}>
              {t("Cerca")}
            </button>
          </div>
          {results && results.length === 0 && <p className="dim note">{t("Nessun risultato")}</p>}
          {results?.map(([path, text, score], i) => (
            <div key={i} className="rag-hit">
              <div className="dim mono">
                {path} · {score.toFixed(2)}
              </div>
              <div className="rag-hit-text">{text.length > 300 ? `${text.slice(0, 300)}…` : text}</div>
            </div>
          ))}
        </div>
      )}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </section>
  );
}
