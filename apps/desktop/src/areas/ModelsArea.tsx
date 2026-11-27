import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  gb,
  type KvCacheType,
  type LoadCheck,
  type LoadParams,
  type ModelInfo,
  type Placement,
  type Preset,
  type ProviderInfo,
  type RuntimeStatus,
} from "../api";

import { EngineSection } from "../components/EngineSection";
import { HubSection } from "../components/HubSection";
import { ProvidersSection } from "../components/ProvidersSection";
import { t, tc, tn } from "../i18n";

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
    const picked = await open({ directory: true, title: t("Scegli una cartella di modelli") });
    if (typeof picked === "string" && !folders.includes(picked)) {
      await saveFolders([...folders, picked]);
    }
  };

  const loadedPaths = (status?.models ?? []).map((m) => m.modelPath);

  return (
    <div className="page">
      <h1>{t("Modelli")}</h1>

      <ProvidersSection providers={providers} onChange={onProvidersChange} />

      <EngineSection />

      <section className="section">
        <div className="section-head">
          <h2>{t("Cartelle")}</h2>
          <div className="row">
            <button className="btn" onClick={rescan}>
              {t("Aggiorna")}
            </button>
            <button className="btn" onClick={addFolder}>
              {t("Aggiungi cartella")}
            </button>
          </div>
        </div>
        {folders.length === 0 && <p className="dim">{t("Aggiungi una cartella che contiene file .gguf per iniziare.")}</p>}
        {folders.map((folder) => (
          <div key={folder} className="folder">
            <span className="mono">{folder}</span>
            <button className="btn-link" onClick={() => saveFolders(folders.filter((f) => f !== folder))}>
              {t("Rimuovi")}
            </button>
          </div>
        ))}
      </section>

      {error && <p className="error" role="alert">{tc(error)}</p>}

      <HubSection folders={folders} status={status} onDownloaded={rescan} />

      {status && status.models.length > 0 && <LoadedModels status={status} onStatusChange={onStatusChange} />}

      <section className="section">
        <h2>{t("Modelli locali")}</h2>
        {models === null && <p className="dim">{t("Ricerca dei modelli…")}</p>}
        {models?.length === 0 && folders.length > 0 && (
          <p className="dim">{t("Nessun file .gguf trovato nelle cartelle indicate.")}</p>
        )}
        {models?.map((model) => (
          <ModelCard
            key={model.path}
            model={model}
            open={selected === model.path}
            onToggle={() => setSelected(selected === model.path ? null : model.path)}
            isLoaded={loadedPaths.includes(model.path)}
            othersLoaded={loadedPaths.some((p) => p !== model.path)}
            multiModel={status?.multiModel ?? false}
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
  /** Ci sono altri modelli caricati (con "Più modelli insieme" acceso, restano). */
  othersLoaded: boolean;
  multiModel: boolean;
  busy: boolean;
  onStatusChange: () => void;
};

function ModelCard({ model, open, onToggle, isLoaded, othersLoaded, multiModel, busy, onStatusChange }: CardProps) {
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

  const doLoad = async (force: boolean, replace = false) => {
    if (!preset) return;
    setWorking(true);
    setError(null);
    try {
      await api.savePreset(model.path, preset);
      onStatusChange();
      await api.loadModel(model.path, preset.load, force, replace);
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
      onStatusChange();
    }
  };

  const doUnload = async () => {
    setWorking(true);
    await api.unloadModel(model.path).catch((e) => setError(String(e)));
    setWorking(false);
    onStatusChange();
  };

  const details = [
    model.sizeLabel,
    model.quantization,
    `${gb(model.sizeBytes)} GB`,
    model.architecture,
    model.parts > 1 && t("diviso in {n} file", { n: model.parts }),
    model.mmproj && t("legge le immagini"),
  ]
    .filter(Boolean)
    .join(" · ");
  const maxContext = model.contextLength ?? 131072;
  const layers = model.blockCount && model.blockCount > 1 ? model.blockCount : null;

  return (
    <div className={isLoaded ? "card loaded" : "card"}>
      <button className="card-head" aria-expanded={open} onClick={onToggle}>
        <div>
          <div className="card-title">
            {model.name}
            {isLoaded && <span className="badge">{t("Caricato")}</span>}
          </div>
          <div className="dim">{model.error ? t("Non leggibile: {error}", { error: tc(model.error) }) : details}</div>
        </div>
        <span className="dim" aria-hidden="true">
          {open ? "▾" : "▸"}
        </span>
      </button>

      {open && preset && (
        <div className="card-body">
          <p className="dim note">{t("Parametri di caricamento: per cambiarli il modello va ricaricato.")}</p>

          <div className="field">
            <label>{t("Collocazione")}</label>
            <div className="segmented">
              {(["vram", "ram", "hybrid"] as const).map((p) => (
                <button
                  key={p}
                  className={preset.load.placement === p ? "active" : ""}
                  disabled={p === "hybrid" && !layers}
                  title={p === "hybrid" && !layers ? t("Il file non dice quanti layer ha il modello.") : undefined}
                  onClick={() =>
                    setLoad(
                      p === "hybrid"
                        ? { placement: p, gpuLayers: preset.load.gpuLayers ?? Math.max(1, Math.floor((layers ?? 2) / 2)) }
                        : { placement: p },
                    )
                  }
                >
                  {p === "vram" ? t("Solo VRAM") : p === "ram" ? t("Solo RAM") : t("Ibrida (avanzata)")}
                </button>
              ))}
            </div>
          </div>

          {preset.load.placement === "hybrid" && layers && (
            <div className="field">
              <label htmlFor={`layers-${model.path}`}>
                {t("Layer sulla GPU: {n} di {total}", { n: preset.load.gpuLayers ?? 0, total: layers })}
              </label>
              <input
                id={`layers-${model.path}`}
                type="range"
                min={1}
                max={layers - 1}
                value={preset.load.gpuLayers ?? 1}
                onChange={(e) => setLoad({ gpuLayers: clamp(Number(e.target.value), 1, layers - 1) })}
              />
              <p className="dim note">
                {t(
                  "Per i modelli che non entrano nella VRAM: una parte dei layer sulla GPU, il resto in RAM. È molto più lento della sola VRAM, perché a ogni token lavora anche il processore. Più layer sulla GPU, più è veloce.",
                )}
              </p>
            </div>
          )}

          <div className="field">
            <label htmlFor={`ctx-${model.path}`}>{t("Contesto (token)")}</label>
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
                <span className="dim">{t("massimo del modello: {n}", { n: model.contextLength })}</span>
              )}
            </div>
          </div>

          <div className="field">
            <label htmlFor={`kv-${model.path}`}>{t("Cache KV")}</label>
            <select
              id={`kv-${model.path}`}
              value={preset.load.kvCacheType}
              onChange={(e) => setLoad({ kvCacheType: e.target.value as KvCacheType })}
            >
              <option value="q8_0">{t("q8_0 — consigliata")}</option>
              <option value="f16">{t("f16 — massima fedeltà, il doppio della memoria")}</option>
              <option value="q4_0">{t("q4_0 — minima memoria, qualità inferiore")}</option>
            </select>
          </div>

          {model.mmproj && (
            <label className="check">
              <input
                type="checkbox"
                checked={preset.load.vision}
                onChange={(e) => setLoad({ vision: e.target.checked })}
              />
              <span>
                <strong>{t("Lettura delle immagini")}</strong>
                <span className="dim note block">
                  {tn("Carica anche il componente visivo {file} ({size} GB in più).", {
                    file: <span className="mono">{model.mmproj.split(/[\\/]/).pop()}</span>,
                    size: gb(model.mmprojBytes ?? 0),
                  })}
                </span>
              </span>
            </label>
          )}

          {model.thinking !== "none" && (
            <label className="check">
              <input
                type="checkbox"
                checked={preset.load.thinking}
                onChange={(e) => setLoad({ thinking: e.target.checked })}
              />
              <span>
                <strong>{t("Ragionamento (thinking)")}</strong>
                <span className="dim note block">
                  {model.thinking === "effort"
                    ? t("Il modello ragiona prima di rispondere e non sa farne a meno: spento, ragiona il meno possibile.")
                    : t("Il modello ragiona prima di rispondere: risposte più accurate, ma più lente. Spento, risponde subito.")}
                </span>
              </span>
            </label>
          )}

          <details className="advanced">
            <summary>{t("Avanzate")}</summary>
            <div className="field">
              <label htmlFor={`threads-${model.path}`}>{t("Thread del processore")}</label>
              <input
                id={`threads-${model.path}`}
                type="number"
                min={1}
                max={256}
                placeholder={t("Automatico")}
                value={preset.load.threads ?? ""}
                onChange={(e) => setLoad({ threads: optionalInt(e.target.value, 1, 256) })}
              />
            </div>
            <div className="field">
              <label htmlFor={`batch-${model.path}`}>{t("Dimensione del batch (token)")}</label>
              <input
                id={`batch-${model.path}`}
                type="number"
                min={32}
                max={8192}
                placeholder={t("Predefinita (2048)")}
                value={preset.load.batchSize ?? ""}
                onChange={(e) => setLoad({ batchSize: optionalInt(e.target.value, 32, 8192) })}
              />
              <span className="dim note">
                {t(
                  "Quanti token del messaggio vengono letti per volta. Più alto = lettura più rapida, un po' più di memoria.",
                )}
              </span>
            </div>
          </details>

          {check && <Estimate check={check} placement={preset.load.placement} />}
          {error && <p className="error pre" role="alert">{tc(error)}</p>}

          <div className="row">
            {isLoaded ? (
              <>
                <button className="btn" disabled={working} onClick={doUnload}>
                  {t("Scarica dalla memoria")}
                </button>
                <button className="btn" disabled={working || busy} onClick={() => doLoad(false)}>
                  {t("Ricarica con questi parametri")}
                </button>
              </>
            ) : (
              <button className="btn primary" disabled={working || busy} onClick={() => doLoad(false)}>
                {working ? t("Caricamento…") : t("Carica")}
              </button>
            )}
            {check && !check.fits && !working && (
              <button className="btn-link" onClick={() => doLoad(true)}>
                {t("Carica comunque")}
              </button>
            )}
            {multiModel && othersLoaded && !working && (check?.fits === false || error !== null) && (
              <button className="btn" onClick={() => doLoad(false, true)}>
                {t("Scarica gli altri e carica")}
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/** I modelli caricati, uno per riga: memoria, principale, scarica (ADR 0029). */
function LoadedModels({ status, onStatusChange }: { status: RuntimeStatus; onStatusChange: () => void }) {
  const [error, setError] = useState<string | null>(null);
  const act = (action: () => Promise<unknown>) => {
    setError(null);
    action()
      .catch((e) => setError(String(e)))
      .finally(onStatusChange);
  };
  const several = status.models.length > 1;
  return (
    <section className="section">
      <h2>{t("Modelli caricati")}</h2>
      {status.models.map((m) => (
        <div key={m.modelPath} className="folder loaded-model">
          <span>
            <strong>{m.modelName}</strong>
            {several && m.primary && <span className="badge">{t("Principale")}</span>}
            <span className="dim">
              {" · "}
              {m.params.placement === "vram" ? t("Solo VRAM") : m.params.placement === "ram" ? t("Solo RAM") : t("Ibrida")}
              {m.processGpu != null && m.processGpu.dedicatedBytes > 0 && ` · ${t("VRAM {gb} GB", { gb: gb(m.processGpu.dedicatedBytes) })}`}
              {m.processRamBytes != null && ` · ${t("RAM {gb} GB", { gb: gb(m.processRamBytes) })}`}
            </span>
            {m.vramSpillBytes != null && (
              <span className="error"> {t("sconfina in RAM per {gb} GB", { gb: gb(m.vramSpillBytes) })}</span>
            )}
          </span>
          <span className="row">
            {several && !m.primary && (
              <button className="btn-link" onClick={() => act(() => api.setPrimaryModel(m.modelPath))}>
                {t("Rendi principale")}
              </button>
            )}
            <button className="btn-link" onClick={() => act(() => api.unloadModel(m.modelPath))}>
              {t("Scarica")}
            </button>
          </span>
        </div>
      ))}
      {status.multiModel && (status.roomVramBytes != null || status.roomRamBytes != null) && (
        <p className="dim note">
          {t("Posto per un altro modello, tenendo un margine per il sistema: {vram}RAM {ram} GB.", {
            vram: status.roomVramBytes != null ? `VRAM ${gb(status.roomVramBytes)} GB, ` : "",
            ram: gb(status.roomRamBytes ?? 0),
          })}
        </p>
      )}
      {several && (
        <p className="dim note">
          {t("Il principale è quello usato dove non se ne sceglie uno: chat con «Il principale», agenti già creati, «local» nell'API, VS Code.")}
        </p>
      )}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </section>
  );
}

function Estimate({ check, placement }: { check: LoadCheck; placement: Placement }) {
  const { estimate, availableBytes, ramAvailableBytes, fits } = check;
  if (placement === "hybrid") {
    return (
      <div className={fits ? "estimate" : "estimate bad"}>
        <div>
          {t("VRAM: circa {need} GB, libera {free} GB", {
            need: gb(estimate.vramBytes),
            free: availableBytes != null ? gb(availableBytes) : "?",
          })}
        </div>
        <div>
          {t("RAM: circa {need} GB, disponibile {free} GB", {
            need: gb(estimate.ramBytes),
            free: ramAvailableBytes != null ? gb(ramAvailableBytes) : "?",
          })}
        </div>
        {!fits && <div>{t("Non entra: sposta qualche layer o riduci il contesto.")}</div>}
      </div>
    );
  }
  const place = placement === "vram" ? "VRAM" : "RAM";
  const need = placement === "vram" ? estimate.vramBytes : estimate.totalBytes;
  const host = placement === "vram" ? estimate.hostWeightsBytes : 0;
  return (
    <div className={fits ? "estimate" : "estimate bad"}>
      <div>
        {tn("Memoria necessaria: {amount}", {
          amount: <strong>{t("circa {gb} GB", { gb: gb(need) })}</strong>,
        })}
        {availableBytes != null && ` — ${t("{place} libera: {gb} GB", { place, gb: gb(availableBytes) })}`}
      </div>
      <div className="dim">
        {t("pesi {gb} GB", { gb: gb(estimate.weightsBytes) })}
        {estimate.kvCacheKnown
          ? ` + ${t("cache KV {gb} GB", { gb: gb(estimate.kvCacheBytes) })}`
          : ` + ${t("cache KV non calcolabile")}`}
        {` + ${t("margine {gb} GB", { gb: gb(estimate.overheadBytes) })}`}
      </div>
      {host > 0 && (
        <div className="dim">
          {t("In più {gb} GB di embedding restano in RAM: llama.cpp li tiene sempre lì, non è uno sconfinamento.", {
            gb: gb(host),
          })}
          {ramAvailableBytes != null && ` ${t("RAM disponibile: {gb} GB", { gb: gb(ramAvailableBytes) })}`}
        </div>
      )}
      {!fits && <div>{t("Il modello non entra nella {place} libera: riduci il contesto o libera memoria.", { place })}</div>}
      {availableBytes == null && <div className="dim">{t("Memoria libera non misurabile su questa GPU.")}</div>}

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
