import { useCallback, useEffect, useState } from "react";
import { api, size, type EngineStatus, type EngineVariant, type InstallProgress } from "../api";

const VARIANT_LABEL: Record<EngineVariant, string> = {
  cuda: "per GPU NVIDIA (CUDA)",
  vulkan: "per GPU AMD e Intel (Vulkan)",
  cpu: "solo processore",
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
      <h2>Motore</h2>
      {status.installed && !progress && (
        <p className="dim">
          llama.cpp installato: <span className="mono">{status.installed}</span>
        </p>
      )}
      {!status.installed && !progress && (
        <div className="estimate bad">
          <div>
            Per eseguire i modelli locali serve <strong>llama.cpp</strong>, che non è ancora installato.
          </div>
          <div className="dim">
            Versione {status.build} {VARIANT_LABEL[status.recommended]} — download di {size(status.downloadBytes)} da
            github.com/ggml-org/llama.cpp
          </div>
          <button className="btn primary engine-install" onClick={install}>
            Scarica e installa
          </button>
        </div>
      )}
      {progress && (
        <div className="estimate">
          <div>
            {progress.phase === "download"
              ? `Download di llama.cpp: ${percent}% (${size(progress.doneBytes)} di ${size(progress.totalBytes)})`
              : "Estrazione dei file…"}
          </div>
          <div className="meter-track engine-progress">
            <div className="meter-fill" style={{ width: `${percent}%` }} />
          </div>
        </div>
      )}
      {error && <p className="error pre">{error}</p>}
    </section>
  );
}
