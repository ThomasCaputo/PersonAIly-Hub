import { useCallback, useEffect, useRef, useState } from "react";
import { api, gb, type CoreInfo, type Preset, type RuntimeStatus } from "./api";
import { ChatArea } from "./areas/ChatArea";
import { ModelsArea } from "./areas/ModelsArea";
import { SpillWarning } from "./components/SpillWarning";
import "./App.css";

const AREAS = [
  { id: "chat", label: "Chat" },
  { id: "agents", label: "Agenti" },
  { id: "models", label: "Modelli" },
  { id: "settings", label: "Impostazioni" },
] as const;

type AreaId = (typeof AREAS)[number]["id"];

const STATUS_INTERVAL_MS = 2000;
const PRESET_SAVE_DELAY_MS = 500;

export default function App() {
  const [areaId, setAreaId] = useState<AreaId>("chat");
  const [core, setCore] = useState<CoreInfo | null>(null);
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [preset, setPreset] = useState<Preset | null>(null);
  const saveTimer = useRef<number | undefined>(undefined);

  const refreshStatus = useCallback(() => {
    api.runtimeStatus().then(setStatus).catch(() => {});
  }, []);

  useEffect(() => {
    api.coreInfo().then(setCore).catch(() => {});
    refreshStatus();
    const timer = window.setInterval(refreshStatus, STATUS_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [refreshStatus]);

  // Il preset in uso è quello del modello caricato.
  const loadedPath = status?.loaded?.modelPath ?? null;
  useEffect(() => {
    if (!loadedPath) {
      setPreset(null);
      return;
    }
    api.getPreset(loadedPath).then(setPreset).catch(() => setPreset(null));
  }, [loadedPath]);

  // Le modifiche ai parametri valgono subito; il salvataggio su disco aspetta
  // che l'utente smetta di muovere i controlli.
  const updatePreset = useCallback(
    (next: Preset) => {
      setPreset(next);
      if (!loadedPath) return;
      window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => {
        api.savePreset(loadedPath, next).catch(() => {});
      }, PRESET_SAVE_DELAY_MS);
    },
    [loadedPath],
  );

  return (
    <div className="shell">
      <nav className="sidebar">
        <div className="brand">Progetto AI</div>
        {AREAS.map((a) => (
          <button
            key={a.id}
            className={a.id === areaId ? "nav-item active" : "nav-item"}
            onClick={() => setAreaId(a.id)}
          >
            {a.label}
          </button>
        ))}
        <StatusPanel status={status} core={core} />
      </nav>

      <main className="content">
        <SpillWarning status={status} />
        <div className="area">
        {areaId === "chat" && (
          <ChatArea
            status={status}
            preset={preset}
            onPresetChange={updatePreset}
            onOpenModels={() => setAreaId("models")}
          />
        )}
        {areaId === "models" && <ModelsArea status={status} onStatusChange={refreshStatus} />}
        {areaId === "agents" && <Placeholder title="Agenti" hint="Gli agenti arrivano con la Tappa 4." />}
        {areaId === "settings" && (
          <Placeholder title="Impostazioni" hint="Le impostazioni arrivano con le prossime tappe." />
        )}
        </div>
      </main>
    </div>
  );
}

function Placeholder({ title, hint }: { title: string; hint: string }) {
  return (
    <div className="page">
      <h1>{title}</h1>
      <p className="dim">{hint}</p>
    </div>
  );
}

function StatusPanel({ status, core }: { status: RuntimeStatus | null; core: CoreInfo | null }) {
  const gpu = status?.memory.gpu ?? null;
  const ramUsed = status ? status.memory.ramTotalBytes - status.memory.ramAvailableBytes : 0;

  return (
    <div className="status">
      <div className="status-model">
        {status?.loading
          ? "Caricamento del modello…"
          : (status?.loaded?.modelName ?? "Nessun modello caricato")}
      </div>
      {status?.loaded && (
        <div className="dim">
          {status.loaded.params.placement === "vram" ? "Solo VRAM" : "Solo RAM"}
          {status.processRamBytes != null && ` · processo ${gb(status.processRamBytes)} GB di RAM`}
        </div>
      )}
      {gpu && <Meter label="VRAM" used={gpu.usedBytes} total={gpu.totalBytes} />}
      {status && <Meter label="RAM" used={ramUsed} total={status.memory.ramTotalBytes} />}
      {core && <div className="dim status-version">Nucleo {core.version}</div>}
    </div>
  );
}

function Meter({ label, used, total }: { label: string; used: number; total: number }) {
  const percent = total > 0 ? Math.min(100, (used / total) * 100) : 0;
  return (
    <div className="meter">
      <div className="meter-label">
        <span>{label}</span>
        <span>
          {gb(used)} / {gb(total)} GB
        </span>
      </div>
      <div className="meter-track">
        <div className={percent > 90 ? "meter-fill high" : "meter-fill"} style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}
