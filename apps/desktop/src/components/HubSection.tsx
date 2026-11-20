import { useCallback, useEffect, useRef, useState } from "react";
import { api, gb, size, type DownloadState, type HubFile, type HubRepo, type RuntimeStatus } from "../api";
import { t, tc } from "../i18n";

type Props = {
  folders: string[];
  status: RuntimeStatus | null;
  /** Chiamata quando un download finisce, per rileggere la libreria. */
  onDownloaded: () => void;
};

const POLL_MS = 1000;

type Kept = { query: string; repos: HubRepo[] | null; openRepo: string | null; files: HubFile[] | null };

/** L'ultima ricerca: come il download, non si perde passando a un'altra pagina. */
let kept: Kept = { query: "", repos: null, openRepo: null, files: null };

/** Ricerca e download di modelli GGUF da Hugging Face. */
export function HubSection({ folders, status, onDownloaded }: Props) {
  const [query, setQuery] = useState(kept.query);
  const [repos, setRepos] = useState(kept.repos);
  const [openRepo, setOpenRepo] = useState(kept.openRepo);
  const [files, setFiles] = useState(kept.files);
  const [folder, setFolder] = useState("");
  const [downloads, setDownloads] = useState<DownloadState[]>([]);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lastPhases = useRef<Map<string, string>>(new Map());

  useEffect(() => {
    // Una scheda lasciata a metà lettura dei file si riapre chiusa, non bloccata.
    kept = { query, repos, openRepo: files ? openRepo : null, files };
  }, [query, repos, openRepo, files]);

  const destination = folders.includes(folder) ? folder : (folders[0] ?? "");
  const vramTotal = status?.memory.gpu?.totalBytes ?? null;

  // L'avanzamento si legge dal nucleo: i download proseguono anche cambiando
  // pagina o chiudendo la finestra.
  const poll = useCallback(() => {
    api
      .listDownloads()
      .then((list) => {
        setDownloads(list);
        const finished = list.some((d) => d.phase === "done" && lastPhases.current.get(d.id) === "downloading");
        lastPhases.current = new Map(list.map((d) => [d.id, d.phase]));
        if (finished) onDownloaded();
      })
      .catch(() => {});
  }, [onDownloaded]);

  useEffect(() => {
    poll();
    const timer = window.setInterval(poll, POLL_MS);
    return () => window.clearInterval(timer);
  }, [poll]);

  const search = async () => {
    if (!query.trim()) return;
    setSearching(true);
    setError(null);
    setOpenRepo(null);
    try {
      setRepos(await api.searchHub(query));
    } catch (e) {
      setError(String(e));
    } finally {
      setSearching(false);
    }
  };

  const toggleRepo = async (id: string) => {
    if (openRepo === id) return setOpenRepo(null);
    setOpenRepo(id);
    setFiles(null);
    setError(null);
    try {
      setFiles(await api.hubFiles(id));
    } catch (e) {
      setError(String(e));
    }
  };

  const start = async (repo: string, file: HubFile) => {
    setError(null);
    await api.startModelDownload(repo, file, destination).catch((e) => setError(String(e)));
    poll();
  };

  const running = (repo: string, file: HubFile) =>
    downloads.some((d) => d.id === `${repo}/${file.path}` && d.phase === "downloading");

  return (
    <section className="section">
      <h2>{t("Scarica un modello")}</h2>

      {downloads.map((download) => (
        <DownloadBar key={download.id} download={download} onDismiss={() => api.dismissDownload(download.id).then(poll)} />
      ))}

      {folders.length === 0 ? (
        <p className="dim">{t("Aggiungi prima una cartella qui sopra: è lì che verranno salvati i modelli.")}</p>
      ) : (
        <>
          <form
            className="row hub-search"
            onSubmit={(e) => {
              e.preventDefault();
              search();
            }}
          >
            <input
              type="search"
              aria-label={t("Cerca modelli su Hugging Face")}
              placeholder={t("Cerca su Hugging Face, per esempio: qwen 7b instruct")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <button className="btn" type="submit" disabled={searching || !query.trim()}>
              {searching ? t("Ricerca…") : t("Cerca")}
            </button>
          </form>

          <p className="dim note">
            {t("Ogni repository si salva in una sua sottocartella (autore/nome), così i componenti visivi restano accanto al loro modello.")}
          </p>
          {folders.length > 1 && (
            <div className="field">
              <label htmlFor="hub-folder">{t("Salva in")}</label>
              <select id="hub-folder" value={destination} onChange={(e) => setFolder(e.target.value)}>
                {folders.map((f) => (
                  <option key={f} value={f}>
                    {f}
                  </option>
                ))}
              </select>
            </div>
          )}

          {repos?.length === 0 && <p className="dim">{t("Nessun modello GGUF trovato.")}</p>}
          {repos?.map((repo) => (
            <div key={repo.id} className="card">
              <button className="card-head" onClick={() => toggleRepo(repo.id)}>
                <div>
                  <div className="card-title">{repo.id}</div>
                  <div className="dim">{t("{n} download", { n: repo.downloads })}</div>
                </div>
                <span className="dim">{openRepo === repo.id ? "▾" : "▸"}</span>
              </button>
              {openRepo === repo.id && (
                <div className="card-body">
                  {files === null && <p className="dim note">{t("Lettura dei file…")}</p>}
                  {files?.length === 0 && <p className="dim note">{t("Nessun file GGUF completo in questo repository.")}</p>}
                  {files?.some((f) => f.kind === "vision") && (
                    <p className="dim note">
                      {t(
                        "I componenti visivi (mmproj) fanno leggere le immagini ai modelli dello stesso repository: scaricane uno insieme al modello.",
                      )}
                    </p>
                  )}
                  {files?.map((file) => (
                    <div key={file.path} className="hub-file">
                      <div>
                        <span className="mono">{file.path}</span>
                        {file.kind === "vision" && <span className="badge">{t("Componente visivo")}</span>}
                        <div className="dim">
                          {size(file.sizeBytes)}
                          {file.parts.length > 1 && ` · ${t("diviso in {n} file", { n: file.parts.length })}`}
                          {file.kind === "model" && vramTotal !== null && ` · ${fitHint(file.sizeBytes, vramTotal)}`}
                        </div>
                      </div>
                      <button className="btn" disabled={running(repo.id, file)} onClick={() => start(repo.id, file)}>
                        {t("Scarica")}
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          ))}
        </>
      )}

      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </section>
  );
}

function DownloadBar({ download, onDismiss }: { download: DownloadState; onDismiss: () => void }) {
  const percent = download.totalBytes > 0 ? Math.round((download.doneBytes / download.totalBytes) * 100) : 0;
  const file = download.file;
  const label = {
    downloading: t("Download di {file}: {percent}% ({done} di {total})", {
      file,
      percent,
      done: size(download.doneBytes),
      total: size(download.totalBytes),
    }),
    done: download.unverified
      ? t(
          "{file} scaricato. Hugging Face non ne pubblica l'impronta: è stata controllata solo la dimensione, quindi scaricalo solo da autori di cui ti fidi.",
          { file },
        )
      : t("{file} scaricato e verificato.", { file }),
    cancelled: t('Download di {file} interrotto al {percent}%. Premi di nuovo "Scarica" per riprenderlo.', {
      file,
      percent,
    }),
    failed: t("Download di {file} non riuscito.", { file }),
  }[download.phase];

  return (
    <div className={download.phase === "failed" ? "estimate bad" : "estimate"}>
      <div className="hub-progress-head">
        <span>{label}</span>
        {download.phase === "downloading" ? (
          <button className="btn" onClick={() => api.cancelDownload(download.id)}>
            {t("Interrompi")}
          </button>
        ) : (
          <button className="conversation-delete" aria-label={t("Togli dall'elenco")} title={t("Togli dall'elenco")} onClick={onDismiss}>
            ×
          </button>
        )}
      </div>
      {download.error && <div className="pre">{tc(download.error)}</div>}
      {download.phase !== "done" && download.phase !== "failed" && (
        <div className="meter-track engine-progress">
          <div className="meter-fill" style={{ width: `${percent}%` }} />
        </div>
      )}
    </div>
  );
}

/** Indicazione di massima: il file più circa il 15 % per cache e buffer. */
function fitHint(fileBytes: number, vramTotal: number): string {
  return fileBytes * 1.15 <= vramTotal
    ? t("entra nei {gb} GB di VRAM", { gb: gb(vramTotal) })
    : t("troppo grande per i {gb} GB di VRAM", { gb: gb(vramTotal) });

}
