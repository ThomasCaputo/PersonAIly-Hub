import { useCallback, useEffect, useState } from "react";
import {
  api,
  size,
  type EngineStatus,
  type EngineVariant,
  type InstallProgress,
  type Preferences,
} from "../api";
import { msg, t, tc, tn } from "../i18n";

const VARIANT_LABEL: Record<EngineVariant, string> = {
  cuda: msg("per GPU NVIDIA (CUDA)"),
  vulkan: msg("per GPU AMD e Intel (Vulkan)"),
  cpu: msg("solo processore"),
};

/** Scelte proposte per lo scaricamento automatico, in minuti (0 = mai). */
const IDLE_CHOICES = [0, 5, 15, 30, 60, 120];

/**
 * llama.cpp, il motore che esegue i modelli locali: varianti installate,
 * quella in uso (LO-9), installazione delle altre e scaricamento automatico
 * del modello dopo un periodo di inattività (LO-8).
 */
export function EngineSection() {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [choice, setChoice] = useState<EngineVariant | null>(null);
  const [installing, setInstalling] = useState<EngineVariant | null>(null);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [preferences, setPreferences] = useState<Preferences | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api
      .engineStatus()
      .then((s) => {
        setStatus(s);
        setChoice((current) => current ?? s.selected ?? s.recommended);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    api.preferences().then(setPreferences).catch(() => {});
  }, [refresh]);

  if (!status) return null;

  const installedVariants = new Set(status.engines.map((e) => e.variant));
  const active = status.engines.find((e) => e.dirName === status.installed) ?? null;
  const picked = choice ?? status.recommended;
  const pickedSize = status.variants.find((v) => v.variant === picked)?.downloadBytes ?? status.downloadBytes;

  const install = async (variant: EngineVariant) => {
    setError(null);
    setInstalling(variant);
    setProgress({ phase: "download", doneBytes: 0, totalBytes: pickedSize });
    try {
      await api.installEngine(variant, setProgress);
    } catch (e) {
      setError(String(e));
    } finally {
      setInstalling(null);
      setProgress(null);
      refresh();
    }
  };

  const use = async (variant: EngineVariant) => {
    setError(null);
    await api.setEngineVariant(variant).catch((e) => setError(String(e)));
    refresh();
  };

  const remove = async (dirName: string) => {
    setError(null);
    await api.removeEngine(dirName).catch((e) => setError(String(e)));
    refresh();
  };

  const saveIdle = async (minutes: number) => {
    if (!preferences) return;
    const next = { ...preferences, idleUnloadMinutes: minutes };
    setPreferences(next);
    await api.setPreferences(next).catch((e) => setError(String(e)));
  };

  const percent = progress ? Math.round((progress.doneBytes / Math.max(1, progress.totalBytes)) * 100) : 0;

  return (
    <section className="section">
      <h2>{t("Motore")}</h2>

      {status.engines.length === 0 && !progress && (
        <div className="estimate bad">
          {tn("Per eseguire i modelli locali serve {engine}, che non è ancora installato.", {
            engine: <strong>llama.cpp</strong>,
          })}
        </div>
      )}

      {status.engines.length > 0 && (
        <div className="engine-list">
          {status.engines.map((engine) => {
            const inUse = engine.dirName === active?.dirName;
            return (
              <div key={engine.dirName} className="folder">
                <span>
                  <span className="mono">{engine.dirName}</span>
                  {engine.variant && <span className="dim"> · {t(VARIANT_LABEL[engine.variant])}</span>}
                  {inUse && <span className="badge">{t("In uso")}</span>}
                </span>
                <span className="row">
                  {!inUse && engine.variant && (
                    <button className="btn-link" onClick={() => use(engine.variant!)}>
                      {t("Usa questa")}
                    </button>
                  )}
                  <button className="btn-link" disabled={installing !== null} onClick={() => remove(engine.dirName)}>
                    {t("Elimina")}
                  </button>
                </span>
              </div>
            );
          })}
          <p className="dim note">{t("La variante scelta vale dal prossimo caricamento di un modello.")}</p>
        </div>
      )}

      {!progress && (
        <div className="field">
          <label htmlFor="engine-variant">
            {status.engines.length === 0 ? t("Variante da installare") : t("Installa un'altra variante")}
          </label>
          <div className="row">
            <select id="engine-variant" value={picked} onChange={(e) => setChoice(e.target.value as EngineVariant)}>
              {status.variants.map((v) => (
                <option key={v.variant} value={v.variant}>
                  {t(VARIANT_LABEL[v.variant])}
                  {v.variant === status.recommended ? ` — ${t("consigliata per questo PC")}` : ""}
                  {installedVariants.has(v.variant) ? ` — ${t("installata")}` : ""}
                </option>
              ))}
            </select>
            <button
              className={status.engines.length === 0 ? "btn primary" : "btn"}
              disabled={installing !== null}
              onClick={() => install(picked)}
            >
              {installedVariants.has(picked) ? t("Reinstalla") : t("Scarica e installa")}
            </button>
          </div>
          <span className="dim note">
            {t("Versione {build} {variant} — download di {size} da github.com/ggml-org/llama.cpp", {
              build: status.build,
              variant: t(VARIANT_LABEL[picked]),
              size: size(pickedSize),
            })}
          </span>
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

      {preferences && (
        <div className="field">
          <label htmlFor="idle-unload">{t("Scarica il modello dalla memoria")}</label>
          <select
            id="idle-unload"
            value={preferences.idleUnloadMinutes}
            onChange={(e) => saveIdle(Number(e.target.value))}
          >
            {(IDLE_CHOICES.includes(preferences.idleUnloadMinutes)
              ? IDLE_CHOICES
              : [...IDLE_CHOICES, preferences.idleUnloadMinutes].sort((a, b) => a - b)
            ).map((minutes) => (
              <option key={minutes} value={minutes}>
                {minutes === 0 ? t("Mai da solo") : t("Dopo {n} minuti senza richieste", { n: minutes })}
              </option>
            ))}
          </select>
          <span className="dim note">
            {t(
              "Libera VRAM e RAM quando il modello non serve, anche con la finestra chiusa. Lo ricarichi dalla scheda del modello.",
            )}
          </span>
        </div>
      )}

      {error && <p className="error pre">{tc(error)}</p>}
    </section>
  );
}
