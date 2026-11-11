import { useState } from "react";
import type { LoadedModel, Preset, SamplingParams } from "../api";

type Props = {
  preset: Preset;
  loaded: LoadedModel;
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
  { key: "temperature", label: "Temperatura", hint: "Più alta = risposte più varie", min: 0, max: 2, step: 0.05 },
  { key: "topP", label: "Top-p", hint: "Quota di probabilità considerata", min: 0, max: 1, step: 0.01 },
  { key: "topK", label: "Top-k", hint: "Numero di parole candidate (0 = tutte)", min: 0, max: 200, step: 1 },
  { key: "minP", label: "Min-p", hint: "Scarta le parole troppo improbabili", min: 0, max: 0.5, step: 0.01 },
  { key: "repeatPenalty", label: "Penalità di ripetizione", hint: "1 = nessuna penalità", min: 1, max: 2, step: 0.01 },
];

const KV_LABEL = { q8_0: "q8_0", f16: "f16", q4_0: "q4_0" } as const;

export function ParamsPanel({ preset, loaded, onChange, onOpenModels }: Props) {
  // Testo così come lo scrive l'utente: ricavarlo dall'elenco gli impedirebbe
  // di digitare la virgola.
  const [stopText, setStopText] = useState(() => preset.sampling.stop.join(", "));

  const setSampling = (patch: Partial<SamplingParams>) =>
    onChange({ ...preset, sampling: { ...preset.sampling, ...patch } });

  return (
    <div className="params-panel">
      <h2>Parametri</h2>
      <p className="dim note">Valgono dal prossimo messaggio.</p>

      <div className="field">
        <label htmlFor="system-prompt">Prompt di sistema</label>
        <textarea
          id="system-prompt"
          rows={4}
          value={preset.systemPrompt}
          placeholder="Istruzioni sempre valide per questo modello"
          onChange={(e) => onChange({ ...preset, systemPrompt: e.target.value })}
        />
      </div>

      {SLIDERS.map((s) => (
        <div className="field" key={s.key} title={s.hint}>
          <div className="slider-head">
            <label htmlFor={`param-${s.key}`}>{s.label}</label>
            <input
              className="slider-value"
              type="number"
              aria-label={s.label}
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

      <div className="field">
        <label htmlFor="param-max-tokens">Token massimi per risposta</label>
        <input
          id="param-max-tokens"
          type="number"
          min={1}
          placeholder="Nessun limite"
          value={preset.sampling.maxTokens ?? ""}
          onChange={(e) => setSampling({ maxTokens: optionalInt(e.target.value, 1) })}
        />
      </div>

      <div className="field">
        <label htmlFor="param-seed">Seed</label>
        <input
          id="param-seed"
          type="number"
          placeholder="Casuale"
          value={preset.sampling.seed ?? ""}
          onChange={(e) => setSampling({ seed: optionalInt(e.target.value, 0) })}
        />
      </div>

      <div className="field">
        <label htmlFor="param-stop">Sequenze di stop</label>
        <input
          id="param-stop"
          type="text"
          placeholder="Separate da virgola"
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

      <h2>Caricamento</h2>
      <dl className="facts">
        <dt>Collocazione</dt>
        <dd>{loaded.params.placement === "vram" ? "Solo VRAM" : "Solo RAM"}</dd>
        <dt>Contesto</dt>
        <dd>{loaded.params.contextLength.toLocaleString("it-IT")} token</dd>
        <dt>Cache KV</dt>
        <dd>{KV_LABEL[loaded.params.kvCacheType]}</dd>
      </dl>
      <p className="dim note">
        Per cambiarli il modello va ricaricato.{" "}
        <button className="btn-link" onClick={onOpenModels}>
          Apri Modelli
        </button>
      </p>
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
