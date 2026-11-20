import { useCallback, useEffect, useRef, useState } from "react";
import { documentDir, join } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  gb,
  size,
  type DownloadState,
  type EngineStatus,
  type HubFile,
  type InstallProgress,
  type ProviderInfo,
  type RuntimeStatus,
} from "../api";
import { msg, t, tc } from "../i18n";
import { ProvidersSection } from "./ProvidersSection";

const GIB = 1024 ** 3;

/** Un modello iniziale da proporre, scelto in base alla memoria disponibile. */
export type Suggestion = {
  repo: string;
  label: string;
  /** Memoria (VRAM o RAM) che serve davvero, modello e contesto insieme. */
  needBytes: number;
  why: string;
};

/**
 * Modelli di partenza, dal più grande: tutti Qwen2.5 Instruct in Q4_K_M, che
 * scrivono bene in italiano e in inglese e sanno usare gli strumenti.
 */
const SUGGESTIONS: Suggestion[] = [
  {
    repo: "bartowski/Qwen2.5-14B-Instruct-GGUF",
    label: "Qwen2.5 14B Instruct",
    needBytes: 11 * GIB,
    why: msg("Il più capace che entra nella tua memoria: buono anche per gli agenti."),
  },
  {
    repo: "bartowski/Qwen2.5-7B-Instruct-GGUF",
    label: "Qwen2.5 7B Instruct",
    needBytes: 6 * GIB,
    why: msg("Un buon equilibrio tra qualità e velocità, anche con gli strumenti."),
  },
  {
    repo: "bartowski/Qwen2.5-3B-Instruct-GGUF",
    label: "Qwen2.5 3B Instruct",
    needBytes: 3 * GIB,
    why: msg("Leggero e veloce, per chat e domande semplici."),
  },
  {
    repo: "bartowski/Qwen2.5-1.5B-Instruct-GGUF",
    label: "Qwen2.5 1.5B Instruct",
    needBytes: 0,
    why: msg("Il più piccolo: gira anche su computer modesti, ma sa fare meno."),
  },
];

/** Il modello consigliato: con una GPU NVIDIA conta la VRAM, altrimenti la RAM (con margine per il sistema). */
export function suggestModel(memory: RuntimeStatus["memory"]): { suggestion: Suggestion; inVram: boolean } {
  const vram = memory.gpu?.totalBytes ?? 0;
  const inVram = vram >= 3 * GIB;
  const budget = inVram ? vram : Math.max(0, memory.ramTotalBytes - 6 * GIB) / 2;
  const suggestion = SUGGESTIONS.find((s) => s.needBytes <= budget) ?? SUGGESTIONS[SUGGESTIONS.length - 1];
  return { suggestion, inVram };
}

/** Il file Q4_K_M di un repository, se c'è. */
export const pickQ4 = (files: HubFile[]) =>
  files.find((f) => f.kind === "model" && /q4_k_m/i.test(f.path)) ?? null;

type Step = "start" | "local" | "cloud" | "done";

type Props = {
  status: RuntimeStatus | null;
  providers: ProviderInfo[];
  onProvidersChange: () => void;
  onFinish: () => void;
};

/**
 * Il primo avvio guidato: riconosce la GPU, installa la variante giusta di
 * llama.cpp e propone un modello adatto alla memoria, oppure collega un
 * provider cloud. Si può saltare in ogni momento e ripetere dalla tavolozza
 * dei comandi.
 */
export function Welcome({ status, providers, onProvidersChange, onFinish }: Props) {
  const [step, setStep] = useState<Step>("start");
  const dialog = useRef<HTMLDivElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);

  // Il fuoco va al titolo di ogni passo, così un lettore di schermo lo annuncia.
  useEffect(() => {
    heading.current?.focus();
  }, [step]);

  // Esc chiude la guida, come il pulsante "Salta".
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onFinish();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onFinish]);

  const gpu = status?.memory.gpu ?? null;

  return (
    <div className="welcome-backdrop">
      <div className="welcome card" role="dialog" aria-modal="true" aria-labelledby="welcome-title" ref={dialog}>
        {step === "start" && (
          <>
            <h1 id="welcome-title" tabIndex={-1} ref={heading}>
              {t("Benvenuto in PersonAIly Hub")}
            </h1>
            <p>{t("Due minuti per preparare il primo modello. Puoi saltare e farlo più tardi dalla pagina Modelli.")}</p>
            {status && (
              <dl className="welcome-hw">
                <dt>{t("Scheda video")}</dt>
                <dd>{gpu ? `${gpu.name}, ${gb(gpu.totalBytes)} GB` : t("nessuna GPU NVIDIA rilevata")}</dd>
                <dt>RAM</dt>
                <dd>{gb(status.memory.ramTotalBytes)} GB</dd>
              </dl>
            )}
            <div className="welcome-choices">
              <button className="choice" onClick={() => setStep("local")}>
                <strong>{t("Un modello sul tuo computer")}</strong>
                <span className="dim">{t("Gratuito e privato: niente esce dal PC. Serve scaricare qualche GB.")}</span>
              </button>
              <button className="choice" onClick={() => setStep("cloud")}>
                <strong>{t("Un servizio cloud")}</strong>
                <span className="dim">
                  {t("Anthropic, OpenAI, OpenRouter e altri, con la tua chiave API. Si paga a consumo.")}
                </span>
              </button>
            </div>
          </>
        )}

        {step === "local" && status && (
          <LocalSetup status={status} heading={heading} onDone={() => setStep("done")} />
        )}

        {step === "cloud" && (
          <>
            <h1 id="welcome-title" tabIndex={-1} ref={heading}>
              {t("Collega un servizio cloud")}
            </h1>
            <ProvidersSection providers={providers} onChange={onProvidersChange} />
            <div className="row">
              <button className="btn primary" disabled={providers.length === 0} onClick={() => setStep("done")}>
                {t("Avanti")}
              </button>
              <button className="btn" onClick={() => setStep("start")}>
                {t("Indietro")}
              </button>
            </div>
          </>
        )}

        {step === "done" && (
          <>
            <h1 id="welcome-title" tabIndex={-1} ref={heading}>
              {t("Tutto pronto")}
            </h1>
            <ul className="welcome-tips">
              <li>{t("Scrivi nella Chat: il modello si sceglie in alto.")}</li>
              <li>{t("Nella pagina Agenti crei assistenti che lavorano da soli su una cartella, con la tua conferma.")}</li>
              <li>{t("Ctrl+K apre la tavolozza dei comandi da qualunque pagina.")}</li>
              <li>{t("Impostazioni → Backup dei dati: una copia di conversazioni, agenti e impostazioni.")}</li>
            </ul>
            <div className="row">
              <button className="btn primary" onClick={onFinish}>
                {t("Inizia")}
              </button>
            </div>
          </>
        )}

        {step !== "done" && (
          <div className="welcome-foot">
            <button className="btn-link" onClick={onFinish}>
              {t("Salta la configurazione")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

/** Motore, cartella e modello: i tre passi per un modello locale. */
function LocalSetup({
  status,
  heading,
  onDone,
}: {
  status: RuntimeStatus;
  heading: React.RefObject<HTMLHeadingElement | null>;
  onDone: () => void;
}) {
  const { suggestion, inVram } = suggestModel(status.memory);
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [install, setInstall] = useState<InstallProgress | null>(null);
  const [folder, setFolder] = useState<string | null>(null);
  const [file, setFile] = useState<HubFile | null>(null);
  const [download, setDownload] = useState<DownloadState | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refreshEngine = useCallback(() => {
    api.engineStatus().then(setEngine).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refreshEngine();
    api
      .modelFolders()
      .then(async (folders) => {
        if (folders.length > 0) return setFolder(folders[0]);
        // Proposta: Documenti\PersonAIly Hub\Modelli; l'utente può sceglierne un'altra.
        try {
          setFolder(await join(await documentDir(), "PersonAIly Hub", "Modelli"));
        } catch {
          setFolder(null);
        }
      })
      .catch(() => {});
    api
      .hubFiles(suggestion.repo)
      .then((files) => setFile(pickQ4(files)))
      .catch((e) => setError(String(e)));
  }, [refreshEngine, suggestion.repo]);

  // Avanzamento del download, finché dura.
  useEffect(() => {
    if (!download || download.phase !== "downloading") return;
    const timer = window.setInterval(() => {
      api
        .listDownloads()
        .then((all) => setDownload(all.find((d) => d.id === download.id) ?? null))
        .catch(() => {});
    }, 1000);
    return () => window.clearInterval(timer);
  }, [download]);

  const installEngine = async () => {
    if (!engine) return;
    setError(null);
    setInstall({ phase: "download", doneBytes: 0, totalBytes: engine.downloadBytes });
    try {
      await api.installEngine(engine.recommended, setInstall);
    } catch (e) {
      setError(String(e));
    } finally {
      setInstall(null);
      refreshEngine();
    }
  };

  const chooseFolder = async () => {
    const picked = await open({ directory: true, title: t("Scegli una cartella di modelli") });
    if (typeof picked === "string") setFolder(picked);
  };

  const startDownload = async () => {
    if (!folder || !file) return;
    setError(null);
    try {
      const folders = await api.modelFolders();
      if (!folders.includes(folder)) await api.setModelFolders([...folders, folder]);
      // Il download parte in sottofondo: continua anche chiudendo la guida.
      await api.startModelDownload(suggestion.repo, file, folder);
      setDownload({
        id: `${suggestion.repo}/${file.path}`,
        repo: suggestion.repo,
        file: file.path,
        doneBytes: 0,
        totalBytes: file.sizeBytes,
        phase: "downloading",
        error: null,
        unverified: false,
      });
    } catch (e) {
      setError(String(e));
    }
  };

  const engineReady = (engine?.engines.length ?? 0) > 0;
  const percent = (done: number, total: number) => Math.round((done / Math.max(1, total)) * 100);

  return (
    <>
      <h1 id="welcome-title" tabIndex={-1} ref={heading}>
        {t("Un modello sul tuo computer")}
      </h1>

      <ol className="welcome-steps">
        <li>
          <h2>{t("1. Il motore (llama.cpp)")}</h2>
          {!engine && <p className="dim">{t("Caricamento…")}</p>}
          {engine && engineReady && <p className="ok">{t("Installato: {name}.", { name: engine.installed ?? "" })}</p>}
          {engine && !engineReady && (
            <>
              <p>
                {engine.recommended === "cuda"
                  ? t("Hai una GPU NVIDIA: installo la versione CUDA ({size}).", { size: size(engine.downloadBytes) })
                  : t("Installo la versione Vulkan, adatta alle GPU AMD e Intel ({size}).", {
                      size: size(engine.downloadBytes),
                    })}
              </p>
              {install ? (
                <Progress
                  label={install.phase === "extract" ? t("Estrazione…") : t("Download di llama.cpp")}
                  value={percent(install.doneBytes, install.totalBytes)}
                />
              ) : (
                <button className="btn primary" onClick={installEngine}>
                  {t("Installa llama.cpp")}
                </button>
              )}
            </>
          )}
        </li>

        <li>
          <h2>{t("2. Il modello")}</h2>
          <p>
            {inVram
              ? t("Con {gb} GB di VRAM ti consiglio:", { gb: gb(status.memory.gpu?.totalBytes ?? 0) })
              : t("Senza una GPU NVIDIA il modello lavora nella RAM ({gb} GB). Ti consiglio:", {
                  gb: gb(status.memory.ramTotalBytes),
                })}
          </p>
          <p>
            <strong>{suggestion.label}</strong>
            {file && <span className="dim"> · {size(file.sizeBytes)}</span>}
            <br />
            <span className="dim">{t(suggestion.why)}</span>
          </p>
          <div className="field">
            <span className="label" id="welcome-folder-label">
              {t("Cartella dei modelli")}
            </span>
            <div className="row">
              <span className="mono" aria-labelledby="welcome-folder-label">
                {folder ?? t("nessuna cartella scelta")}
              </span>
              <button className="btn-link" onClick={chooseFolder}>
                {t("Cambia")}
              </button>
            </div>
          </div>
          {download ? (
            <>
              {download.phase === "downloading" && (
                <Progress
                  label={t("Download di {name}", { name: suggestion.label })}
                  value={percent(download.doneBytes, download.totalBytes)}
                />
              )}
              {download.phase === "done" && (
                <p className="ok">{t("Scaricato. Caricalo dalla pagina Modelli, nella sezione Modelli locali.")}</p>
              )}
              {download.phase === "failed" && <p className="error" role="alert">{tc(download.error ?? "")}</p>}
            </>
          ) : (
            <button className="btn primary" disabled={!file || !folder} onClick={startDownload}>
              {t("Scarica il modello")}
            </button>
          )}
          {!file && !error && <p className="dim">{t("Cerco il file su Hugging Face…")}</p>}
        </li>
      </ol>

      {error && (
        <p className="error pre" role="alert">
          {tc(error)}
        </p>
      )}
      <div className="row">
        <button className="btn primary" disabled={!engineReady} onClick={onDone}>
          {download?.phase === "downloading" ? t("Continua mentre scarica") : t("Avanti")}
        </button>
      </div>
    </>
  );
}

function Progress({ label, value }: { label: string; value: number }) {
  return (
    <div className="welcome-progress">
      <span>
        {label} — {value}%
      </span>
      <div
        className="meter-track"
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={value}
      >
        <div className="meter-fill" style={{ width: `${value}%` }} />
      </div>
    </div>
  );
}
