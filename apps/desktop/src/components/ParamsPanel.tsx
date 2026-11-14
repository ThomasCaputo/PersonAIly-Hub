import { useState } from "react";
import type { LoadedModel, Preset, SamplingParams } from "../api";
import { NamedPresets } from "./NamedPresets";
import { msg, t } from "../i18n";

type Props = {
  preset: Preset;
  /** Il modello locale caricato; `null` per un modello cloud. */
  loaded: LoadedModel | null;
  /** Parametri accettati dal modello; `null` = tutti (modello locale). */
  supported: string[] | null;
  onChange: (preset: Preset) => void;
  onOpenModels: () => void;
};

type SliderDef = {
  key: "temperature" | "topP" | "topK" | "minP" | "repeatPenalty";
  label: string;
  hint: string;
  min: number;
  max: number;
  step: number;
};

const SLIDERS: SliderDef[] = [
  {
    key: "temperature",
    label: msg("Temperatura"),
    hint: msg("Più alta = risposte più varie"),
    min: 0,
    max: 2,
    step: 0.05,
  },
  { key: "topP", label: "Top-p", hint: msg("Quota di probabilità considerata"), min: 0, max: 1, step: 0.01 },
  { key: "topK", label: "Top-k", hint: msg("Numero di parole candidate (0 = tutte)"), min: 0, max: 200, step: 1 },
  { key: "minP", label: "Min-p", hint: msg("Scarta le parole troppo improbabili"), min: 0, max: 0.5, step: 0.01 },
  {
    key: "repeatPenalty",
    label: msg("Penalità di ripetizione"),
    hint: msg("1 = nessuna penalità"),
    min: 1,
    max: 2,
    step: 0.01,
  },
];

const KV_LABEL = { q8_0: "q8_0", f16: "f16", q4_0: "q4_0" } as const;

export function ParamsPanel({ preset, loaded, supported, onChange, onOpenModels }: Props) {
  const shows = (param: string) => supported === null || supported.includes(param);

  // Testo così come lo scrive l'utente: ricavarlo dall'elenco gli impedirebbe
  // di digitare la virgola.
  const [stopText, setStopText] = useState(() => preset.sampling.stop.join(", "));

  const setSampling = (patch: Partial<SamplingParams>) =>
    onChange({ ...preset, sampling: { ...preset.sampling, ...patch } });

  return (
    <div className="params-panel">
      <h2>{t("Parametri")}</h2>
      <p className="dim note">{t("Valgono dal prossimo messaggio.")}</p>

      <NamedPresets
        preset={preset}
        onApply={(named) => {
          setStopText(named.sampling.stop.join(", "));
          onChange({ ...preset, sampling: named.sampling, systemPrompt: named.systemPrompt });
        }}
      />

      <div className="field">
        <label htmlFor="system-prompt">{t("Prompt di sistema")}</label>
        <textarea
          id="system-prompt"
          rows={4}
          value={preset.systemPrompt}
          placeholder={t("Istruzioni sempre valide per questo modello")}
          onChange={(e) => onChange({ ...preset, systemPrompt: e.target.value })}
        />
      </div>

      {SLIDERS.filter((s) => shows(s.key)).map((s) => (
        <div className="field" key={s.key} title={t(s.hint)}>
          <div className="slider-head">
            <label htmlFor={`param-${s.key}`}>{t(s.label)}</label>
            <input
              className="slider-value"
              type="number"
              aria-label={t(s.label)}
              min={s.min}
              max={s.max}
              step={s.step}
              value={preset.sampling[s.key]}
              onChange={(e) => setSampling({ [s.key]: toNumber(e.target.value, s) })}
            />
          </div>
          <input
            id={`param-${s.key}`}
            type="range"
            min={s.min}
            max={s.max}
            step={s.step}
            value={preset.sampling[s.key]}
            onChange={(e) => setSampling({ [s.key]: Number(e.target.value) })}
          />
        </div>
      ))}

      {shows("maxTokens") && (
        <div className="field">
          <label htmlFor="param-max-tokens">{t("Token massimi per risposta")}</label>
          <input
            id="param-max-tokens"
            type="number"
            min={1}
            placeholder={loaded ? t("Nessun limite") : t("Predefinito")}
            value={preset.sampling.maxTokens ?? ""}
            onChange={(e) => setSampling({ maxTokens: optionalInt(e.target.value, 1) })}
          />
        </div>
      )}

      {shows("seed") && (
        <div className="field">
          <label htmlFor="param-seed">{t("Seed")}</label>
          <input
            id="param-seed"
            type="number"
            placeholder={t("Casuale")}
            value={preset.sampling.seed ?? ""}
            onChange={(e) => setSampling({ seed: optionalInt(e.target.value, 0) })}
          />
        </div>
      )}

      <div className="field">
        <label htmlFor="param-stop">{t("Sequenze di stop")}</label>
        <input
          id="param-stop"
          type="text"
          placeholder={t("Separate da virgola")}
          value={stopText}
          onChange={(e) => {
            setStopText(e.target.value);
            setSampling({
              stop: e.target.value
                .split(",")
                .map((s) => s.trim())
                .filter(Boolean),
            });
          }}
        />
      </div>

      {supported !== null && (
        <p className="dim note">
          {t("Sono mostrati solo i parametri che questo servizio accetta: gli altri verrebbero rifiutati.")}
        </p>
      )}

      {loaded && (
        <>
          <h2>{t("Caricamento")}</h2>
          <dl className="facts">
            <dt>{t("Collocazione")}</dt>
            <dd>{loaded.params.placement === "vram" ? t("Solo VRAM") : t("Solo RAM")}</dd>
            <dt>{t("Contesto")}</dt>
            <dd>{t("{n} token", { n: loaded.params.contextLength })}</dd>
            <dt>{t("Cache KV")}</dt>
            <dd>{KV_LABEL[loaded.params.kvCacheType]}</dd>
          </dl>
          <p className="dim note">
            {t("Per cambiarli il modello va ricaricato.")}{" "}
            <button className="btn-link" onClick={onOpenModels}>
              {t("Apri Modelli")}
            </button>

          </p>
        </>
      )}
    </div>
  );
}

function toNumber(text: string, def: SliderDef): number {
  const n = Number(text);
  if (!Number.isFinite(n)) return def.min;
  return Math.max(def.min, Math.min(def.max, n));
}

function optionalInt(text: string, min: number): number | null {
  if (text.trim() === "") return null;
  const n = Math.round(Number(text));
  return Number.isFinite(n) && n >= min ? n : null;
}
