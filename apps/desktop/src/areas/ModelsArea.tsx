import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  gb,
  type KvCacheType,
  type LoadCheck,
  type LoadParams,
  type ModelInfo,
  type Preset,
  type ProviderInfo,
  type RuntimeStatus,
} from "../api";

import { EngineSection } from "../components/EngineSection";

import { HubSection } from "../components/HubSection";
import { ProvidersSection } from "../components/ProvidersSection";

type Props = {
  status: RuntimeStatus | null;
  onStatusChange: () => void;
  providers: ProviderInfo[];
  onProvidersChange: () => void;
};

export function ModelsArea({ status, onStatusChange, providers, onProvidersChange }: Props) {
  const [folders, setFolders] = useState<string[]>([]);
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const rescan = useCallback(() => {
    setModels(null);
    api.listModels().then(setModels).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    api.modelFolders().then(setFolders).catch((e) => setError(String(e)));
    rescan();
  }, [rescan]);

  const saveFolders = async (next: string[]) => {
    setFolders(next);
    await api.setModelFolders(next);
    rescan();
  };

  const addFolder = async () => {
    const picked = await open({ directory: true, title: "Scegli una cartella di modelli" });
    if (typeof picked === "string" && !folders.includes(picked)) {
      await saveFolders([...folders, picked]);
    }
  };

  const loadedPath = status?.loaded?.modelPath ?? null;

  return (
    <div className="page">
      <h1>Modelli</h1>

      <ProvidersSection providers={providers} onChange={onProvidersChange} />

      <EngineSection />

      <section className="section">
        <div className="section-head">
          <h2>Cartelle</h2>
          <div className="row">
            <button className="btn" onClick={rescan}>
              Aggiorna
            </button>
            <button className="btn" onClick={addFolder}>
              Aggiungi cartella
            </button>
          </div>
        </div>
        {folders.length === 0 && (
          <p className="dim">Aggiungi una cartella che contiene file .gguf per iniziare.</p>
        )}
        {folders.map((folder) => (
          <div key={folder} className="folder">
            <span className="mono">{folder}</span>
            <button className="btn-link" onClick={() => saveFolders(folders.filter((f) => f !== folder))}>
              Rimuovi
            </button>
          </div>
        ))}
      </section>

      {error && <p className="error">{error}</p>}

      <HubSection folders={folders} status={status} onDownloaded={rescan} />

      <section className="section">
        <h2>Modelli locali</h2>
        {models === null && <p className="dim">Ricerca dei modelli…</p>}
        {models?.length === 0 && folders.length > 0 && (
          <p className="dim">Nessun file .gguf trovato nelle cartelle indicate.</p>
        )}
        {models?.map((model) => (
          <ModelCard
            key={model.path}
            model={model}
            open={selected === model.path}
            onToggle={() => setSelected(selected === model.path ? null : model.path)}
            isLoaded={loadedPath === model.path}
            busy={status?.loading ?? false}
            onStatusChange={onStatusChange}
          />
        ))}
      </section>
    </div>
  );
}

type CardProps = {
  model: ModelInfo;
  open: boolean;
  onToggle: () => void;
  isLoaded: boolean;
  busy: boolean;
  onStatusChange: () => void;
};

function ModelCard({ model, open, onToggle, isLoaded, busy, onStatusChange }: CardProps) {
  const [preset, setPreset] = useState<Preset | null>(null);
  const [check, setCheck] = useState<LoadCheck | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open && !preset) {
      api.getPreset(model.path).then(setPreset).catch((e) => setError(String(e)));
    }
  }, [open, preset, model.path]);

  // Ricalcola la stima a ogni modifica dei parametri di caricamento.
  const load = preset?.load;
  useEffect(() => {
    if (!open || !load) return;
    let current = true;
    api.checkLoad(model.path, load).then((c) => current && setCheck(c)).catch(() => {});
    return () => {
      current = false;
    };
  }, [open, load, model.path, isLoaded]);

  const setLoad = (patch: Partial<LoadParams>) => {
    if (preset) setPreset({ ...preset, load: { ...preset.load, ...patch } });
  };

  const doLoad = async (force: boolean) => {
    if (!preset) return;
    setWorking(true);
    setError(null);
    try {
      await api.savePreset(model.path, preset);
      onStatusChange();
      await api.loadModel(model.path, preset.load, force);
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
      onStatusChange();
    }
  };

  const doUnload = async () => {
    setWorking(true);
    await api.unloadModel().catch((e) => setError(String(e)));
    setWorking(false);
    onStatusChange();
  };

  const details = [model.sizeLabel, model.quantization, `${gb(model.sizeBytes)} GB`, model.architecture]
    .filter(Boolean)
    .join(" · ");
  const maxContext = model.contextLength ?? 131072;

  return (
    <div className={isLoaded ? "card loaded" : "card"}>
      <button className="card-head" onClick={onToggle}>
        <div>
          <div className="card-title">
            {model.name}
            {isLoaded && <span className="badge">Caricato</span>}
          </div>
          <div className="dim">{model.error ? `Non leggibile: ${model.error}` : details}</div>
        </div>
        <span className="dim">{open ? "▾" : "▸"}</span>
      </button>

      {open && preset && (
        <div className="card-body">
          <p className="dim note">Parametri di caricamento: per cambiarli il modello va ricaricato.</p>

          <div className="field">
            <label>Collocazione</label>
            <div className="segmented">
              {(["vram", "ram"] as const).map((p) => (
                <button
                  key={p}
                  className={preset.load.placement === p ? "active" : ""}
                  onClick={() => setLoad({ placement: p })}
                >
                  {p === "vram" ? "Solo VRAM" : "Solo RAM"}
                </button>
              ))}
            </div>
          </div>

          <div className="field">
            <label htmlFor={`ctx-${model.path}`}>Contesto (token)</label>
            <div className="row">
              <input
                id={`ctx-${model.path}`}
                type="number"
                min={512}
                max={maxContext}
                step={1024}
                value={preset.load.contextLength}
                onChange={(e) => setLoad({ contextLength: clamp(Number(e.target.value), 512, maxContext) })}
              />
              {model.contextLength && (
                <span className="dim">massimo del modello: {model.contextLength.toLocaleString("it-IT")}</span>
              )}
            </div>
          </div>

          <div className="field">
            <label htmlFor={`kv-${model.path}`}>Cache KV</label>
            <select
              id={`kv-${model.path}`}
              value={preset.load.kvCacheType}
              onChange={(e) => setLoad({ kvCacheType: e.target.value as KvCacheType })}
            >
              <option value="q8_0">q8_0 — consigliata</option>
              <option value="f16">f16 — massima fedeltà, il doppio della memoria</option>
              <option value="q4_0">q4_0 — minima memoria, qualità inferiore</option>
            </select>
          </div>

          <details className="advanced">
            <summary>Avanzate</summary>
            <div className="field">
              <label htmlFor={`threads-${model.path}`}>Thread del processore</label>
              <input
                id={`threads-${model.path}`}
                type="number"
                min={1}
                max={256}
                placeholder="Automatico"
                value={preset.load.threads ?? ""}
                onChange={(e) => setLoad({ threads: optionalInt(e.target.value, 1, 256) })}
              />
            </div>
            <div className="field">
              <label htmlFor={`batch-${model.path}`}>Dimensione del batch (token)</label>
              <input
                id={`batch-${model.path}`}
                type="number"
                min={32}
                max={8192}
                placeholder="Predefinita (2048)"
                value={preset.load.batchSize ?? ""}
                onChange={(e) => setLoad({ batchSize: optionalInt(e.target.value, 32, 8192) })}
              />
              <span className="dim note">
                Quanti token del messaggio vengono letti per volta. Più alto = lettura più rapida, un po' più di memoria.
              </span>
            </div>
          </details>

          {check && <Estimate check={check} placement={preset.load.placement} />}
          {error && <p className="error pre">{error}</p>}

          <div className="row">
            {isLoaded ? (
              <>
                <button className="btn" disabled={working} onClick={doUnload}>
                  Scarica dalla memoria
                </button>
                <button className="btn" disabled={working || busy} onClick={() => doLoad(false)}>
                  Ricarica con questi parametri
                </button>
              </>
            ) : (
              <button className="btn primary" disabled={working || busy} onClick={() => doLoad(false)}>
                {working ? "Caricamento…" : "Carica"}
              </button>
            )}
            {check && !check.fits && !working && (
              <button className="btn-link" onClick={() => doLoad(true)}>
                Carica comunque
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function Estimate({ check, placement }: { check: LoadCheck; placement: "vram" | "ram" }) {
  const { estimate, availableBytes, fits } = check;
  const place = placement === "vram" ? "VRAM" : "RAM";
  return (
    <div className={fits ? "estimate" : "estimate bad"}>
      <div>
        Memoria necessaria: <strong>circa {gb(estimate.totalBytes)} GB</strong>
        {availableBytes != null && ` — ${place} libera: ${gb(availableBytes)} GB`}
      </div>
      <div className="dim">
        pesi {gb(estimate.weightsBytes)} GB
        {estimate.kvCacheKnown ? ` + cache KV ${gb(estimate.kvCacheBytes)} GB` : " + cache KV non calcolabile"}
        {` + margine ${gb(estimate.overheadBytes)} GB`}
      </div>
      {!fits && <div>Il modello non entra nella {place} libera: riduci il contesto o libera memoria.</div>}
      {availableBytes == null && <div className="dim">Memoria libera non misurabile su questa GPU.</div>}
    </div>
  );
}

/** Numero intero facoltativo: campo vuoto o valore fuori dai limiti = automatico. */
function optionalInt(text: string, min: number, max: number): number | null {
  if (text.trim() === "") return null;
  const n = Math.round(Number(text));
  return Number.isFinite(n) && n >= min && n <= max ? n : null;
}

function clamp(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) return min;
  return Math.max(min, Math.min(max, Math.round(value)));
}
