import { useCallback, useEffect, useRef, useState } from "react";
import { api, gb, size, type DownloadState, type HubFile, type HubRepo, type RuntimeStatus } from "../api";

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
  const [download, setDownload] = useState<DownloadState | null>(null);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lastPhase = useRef<string | null>(null);

  useEffect(() => {
    // Una scheda lasciata a metà lettura dei file si riapre chiusa, non bloccata.
    kept = { query, repos, openRepo: files ? openRepo : null, files };
  }, [query, repos, openRepo, files]);

  const destination = folders.includes(folder) ? folder : (folders[0] ?? "");
  const vramTotal = status?.memory.gpu?.totalBytes ?? null;

  // L'avanzamento si legge dal nucleo: il download prosegue anche cambiando pagina.
  const poll = useCallback(() => {
    api
      .downloadStatus()
      .then((state) => {
        setDownload(state);
        if (state && lastPhase.current === "downloading" && state.phase === "done") onDownloaded();
        lastPhase.current = state?.phase ?? null;
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

  const active = download?.phase === "downloading";

  return (
    <section className="section">
      <h2>Scarica un modello</h2>

      {download && <DownloadBar download={download} />}

      {folders.length === 0 ? (
        <p className="dim">Aggiungi prima una cartella qui sopra: è lì che verranno salvati i modelli.</p>
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
              aria-label="Cerca modelli su Hugging Face"
              placeholder="Cerca su Hugging Face, per esempio: qwen 7b instruct"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <button className="btn" type="submit" disabled={searching || !query.trim()}>
              {searching ? "Ricerca…" : "Cerca"}
            </button>
          </form>

          {folders.length > 1 && (
            <div className="field">
              <label htmlFor="hub-folder">Salva in</label>
              <select id="hub-folder" value={destination} onChange={(e) => setFolder(e.target.value)}>
                {folders.map((f) => (
                  <option key={f} value={f}>
                    {f}
                  </option>
                ))}
              </select>
            </div>
          )}

          {repos?.length === 0 && <p className="dim">Nessun modello GGUF trovato.</p>}
          {repos?.map((repo) => (
            <div key={repo.id} className="card">
              <button className="card-head" onClick={() => toggleRepo(repo.id)}>
                <div>
                  <div className="card-title">{repo.id}</div>
                  <div className="dim">{repo.downloads.toLocaleString("it-IT")} download</div>
                </div>
                <span className="dim">{openRepo === repo.id ? "▾" : "▸"}</span>
              </button>
              {openRepo === repo.id && (
                <div className="card-body">
                  {files === null && <p className="dim note">Lettura dei file…</p>}
                  {files?.length === 0 && (
                    <p className="dim note">
                      Nessun modello scaricabile: questo repository contiene solo modelli divisi in più file, non
                      ancora supportati.
                    </p>
                  )}
                  {files?.map((file) => (
                    <div key={file.path} className="hub-file">
                      <div>
                        <span className="mono">{file.path}</span>
                        <div className="dim">
                          {size(file.sizeBytes)}
                          {vramTotal !== null && ` · ${fitHint(file.sizeBytes, vramTotal)}`}
                        </div>
                      </div>
                      <button className="btn" disabled={active} onClick={() => start(repo.id, file)}>
                        Scarica
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          ))}
        </>
      )}

      {error && <p className="error pre">{error}</p>}
    </section>
  );
}

function DownloadBar({ download }: { download: DownloadState }) {
  const percent = download.totalBytes > 0 ? Math.round((download.doneBytes / download.totalBytes) * 100) : 0;
  const label = {
    downloading: `Download di ${download.file}: ${percent}% (${size(download.doneBytes)} di ${size(download.totalBytes)})`,
    done: `${download.file} scaricato e verificato.`,
    cancelled: `Download di ${download.file} interrotto al ${percent}%. Premi di nuovo "Scarica" per riprenderlo.`,
    failed: `Download di ${download.file} non riuscito.`,
  }[download.phase];

  return (
    <div className={download.phase === "failed" ? "estimate bad" : "estimate"}>
      <div className="hub-progress-head">
        <span>{label}</span>
        {download.phase === "downloading" && (
          <button className="btn" onClick={() => api.cancelDownload()}>
            Interrompi
          </button>
        )}
      </div>
      {download.error && <div className="pre">{download.error}</div>}
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
    ? `entra nei ${gb(vramTotal)} GB di VRAM`
    : `troppo grande per i ${gb(vramTotal)} GB di VRAM`;
}
