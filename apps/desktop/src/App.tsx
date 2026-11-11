import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type CoreInfo = { version: string; schemaVersion: number };

const AREAS = [
  { id: "chat", label: "Chat", hint: "Le conversazioni arrivano con la Tappa 1." },
  { id: "agents", label: "Agenti", hint: "Gli agenti arrivano con la Tappa 4." },
  { id: "models", label: "Modelli", hint: "La libreria dei modelli arriva con la Tappa 1." },
  { id: "settings", label: "Impostazioni", hint: "Le impostazioni arrivano con le prossime tappe." },
] as const;

type AreaId = (typeof AREAS)[number]["id"];

export default function App() {
  const [areaId, setAreaId] = useState<AreaId>("chat");
  const [core, setCore] = useState<CoreInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<CoreInfo>("core_info")
      .then(setCore)
      .catch((e) => setError(String(e)));
  }, []);

  const area = AREAS.find((a) => a.id === areaId)!;

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
        <div className="status">
          {error && <span className="status-error">Nucleo non raggiungibile: {error}</span>}
          {core && (
            <span>
              Nucleo {core.version} · archivio v{core.schemaVersion}
            </span>
          )}
        </div>
      </nav>

      <main className="content">
        <h1>{area.label}</h1>
        <p className="placeholder">{area.hint}</p>
      </main>
    </div>
  );
}
