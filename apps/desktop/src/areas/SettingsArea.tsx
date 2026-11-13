import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type ToolSettings } from "../api";
import { McpSection } from "../components/McpSection";
import { ToolsBar } from "../components/ToolsBar";

/** Cartelle concesse ai modelli e strumenti predefiniti delle nuove conversazioni. */
export function SettingsArea() {
  const [folders, setFolders] = useState<string[]>([]);
  const [defaults, setDefaults] = useState<ToolSettings | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.grantedFolders().then(setFolders).catch((e) => setError(String(e)));
    api.toolDefaults().then(setDefaults).catch((e) => setError(String(e)));
  }, []);

  const saveFolders = async (next: string[]) => {
    setFolders(next);
    await api.setGrantedFolders(next).catch((e) => setError(String(e)));
  };

  const addFolder = async () => {
    const picked = await open({ directory: true, title: "Scegli una cartella da concedere ai modelli" });
    if (typeof picked === "string" && !folders.includes(picked)) await saveFolders([...folders, picked]);
  };

  const saveDefaults = async (next: ToolSettings) => {
    setDefaults(next);
    await api.setToolDefaults(next).catch((e) => setError(String(e)));
  };

  return (
    <div className="page">
      <h1>Impostazioni</h1>

      <section className="section">
        <div className="section-head">
          <h2>Cartelle concesse ai modelli</h2>
          <button className="btn" onClick={addFolder}>
            Aggiungi cartella
          </button>
        </div>
        <p className="dim note">
          I modelli possono leggere, creare e modificare file solo qui dentro, sottocartelle comprese. Ogni modifica
          chiede la tua conferma, salvo che tu scelga altrimenti.
        </p>
        {folders.length === 0 && <p className="dim">Nessuna cartella concessa: i modelli non vedono i tuoi file.</p>}
        {folders.map((folder) => (
          <div key={folder} className="folder">
            <span className="mono">{folder}</span>
            <button className="btn-link" onClick={() => saveFolders(folders.filter((f) => f !== folder))}>
              Revoca
            </button>
          </div>
        ))}
      </section>

      <section className="section">
        <h2>Strumenti delle nuove conversazioni</h2>
        <p className="dim note">Ogni conversazione può poi cambiarli dalla barra sopra i messaggi.</p>
        {defaults && (
          <ToolsBar
            settings={defaults}
            folderCount={folders.length}
            disabled={false}
            onChange={saveDefaults}
            onOpenSettings={addFolder}
          />
        )}
      </section>

      <McpSection />

      {error && <p className="error pre">{error}</p>}
    </div>
  );
}
