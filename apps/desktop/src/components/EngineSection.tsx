import { useCallback, useEffect, useState } from "react";
import { api, size, type EngineStatus, type EngineVariant, type InstallProgress } from "../api";
import { msg, t, tc, tn } from "../i18n";

const VARIANT_LABEL: Record<EngineVariant, string> = {
  cuda: msg("per GPU NVIDIA (CUDA)"),
  vulkan: msg("per GPU AMD e Intel (Vulkan)"),
  cpu: msg("solo processore"),
};

/** Stato di llama.cpp, il motore che esegue i modelli locali, e sua installazione. */
export function EngineSection() {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api.engineStatus().then(setStatus).catch((e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  if (!status) return null;

  const install = async () => {
    setError(null);
    setProgress({ phase: "download", doneBytes: 0, totalBytes: status.downloadBytes });
    try {
      await api.installEngine(status.recommended, setProgress);
    } catch (e) {
      setError(String(e));
    } finally {
      setProgress(null);
      refresh();
    }
  };

  const percent = progress ? Math.round((progress.doneBytes / Math.max(1, progress.totalBytes)) * 100) : 0;

  return (
    <section className="section">
      <h2>{t("Motore")}</h2>
      {status.installed && !progress && (
        <p className="dim">{tn("llama.cpp installato: {version}", { version: <span className="mono">{status.installed}</span> })}</p>
      )}
      {!status.installed && !progress && (
        <div className="estimate bad">
          <div>
            {tn("Per eseguire i modelli locali serve {engine}, che non è ancora installato.", {
              engine: <strong>llama.cpp</strong>,
            })}
          </div>
          <div className="dim">
            {t("Versione {build} {variant} — download di {size} da github.com/ggml-org/llama.cpp", {
              build: status.build,
              variant: t(VARIANT_LABEL[status.recommended]),
              size: size(status.downloadBytes),
            })}
          </div>
          <button className="btn primary engine-install" onClick={install}>
            {t("Scarica e installa")}
          </button>
        </div>
      )}
      {progress && (
        <div className="estimate">
          <div>
            {progress.phase === "download"
              ? t("Download di llama.cpp: {percent}% ({done} di {total})", {
                  percent,
                  done: size(progress.doneBytes),
                  total: size(progress.totalBytes),
                })
              : t("Estrazione dei file…")}
          </div>
          <div className="meter-track engine-progress">
            <div className="meter-fill" style={{ width: `${percent}%` }} />
          </div>
        </div>
      )}
      {error && <p className="error pre">{tc(error)}</p>}
    </section>
  );
}
