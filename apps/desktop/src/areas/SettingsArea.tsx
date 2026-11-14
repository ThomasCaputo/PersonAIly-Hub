import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type SecuritySettings, type ToolSettings } from "../api";
import { ApiSection } from "../components/ApiSection";
import { McpSection } from "../components/McpSection";
import { ToolsBar } from "../components/ToolsBar";
import { LANGUAGES, t, tc, type Lang } from "../i18n";
import { useLanguage } from "../LanguageProvider";

/** Lingua, cartelle concesse, strumenti predefiniti, protezioni, MCP e API locale. */
export function SettingsArea() {
  const { language, chooseLanguage } = useLanguage();
  const [folders, setFolders] = useState<string[]>([]);
  const [defaults, setDefaults] = useState<ToolSettings | null>(null);
  const [security, setSecurity] = useState<SecuritySettings | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.grantedFolders().then(setFolders).catch((e) => setError(String(e)));
    api.toolDefaults().then(setDefaults).catch((e) => setError(String(e)));
    api.securitySettings().then(setSecurity).catch((e) => setError(String(e)));
  }, []);

  const saveFolders = async (next: string[]) => {
    setFolders(next);
    await api.setGrantedFolders(next).catch((e) => setError(String(e)));
  };

  const addFolder = async () => {
    const picked = await open({ directory: true, title: t("Scegli una cartella da concedere ai modelli") });
    if (typeof picked === "string" && !folders.includes(picked)) await saveFolders([...folders, picked]);
  };

  const saveDefaults = async (next: ToolSettings) => {
    setDefaults(next);
    await api.setToolDefaults(next).catch((e) => setError(String(e)));
  };

  const saveSecurity = async (change: Partial<SecuritySettings>) => {
    if (!security) return;
    const next = { ...security, ...change };
    setSecurity(next);
    await api.setSecuritySettings(next).catch((e) => setError(String(e)));
  };

  return (
    <div className="page">
      <h1>{t("Impostazioni")}</h1>

      <section className="section">
        <h2>{t("Lingua")}</h2>
        <div className="field">
          <label htmlFor="language">{t("Lingua dell'interfaccia")}</label>
          <select id="language" value={language} onChange={(e) => chooseLanguage(e.target.value as Lang)}>
            {LANGUAGES.map((l) => (
              <option key={l.id} value={l.id}>
                {l.label}
              </option>
            ))}
          </select>
          <p className="dim note">
            {t(
              "I modelli rispondono nella lingua in cui scrivi. Alcuni messaggi di errore meno comuni restano in italiano.",
            )}
          </p>
        </div>
      </section>

      <section className="section">
        <div className="section-head">
          <h2>{t("Cartelle concesse ai modelli")}</h2>
          <button className="btn" onClick={addFolder}>
            {t("Aggiungi cartella")}
          </button>
        </div>
        <p className="dim note">
          {t(
            "I modelli possono leggere, creare e modificare file solo qui dentro, sottocartelle comprese. Ogni modifica chiede la tua conferma, salvo che tu scelga altrimenti.",
          )}
        </p>
        {folders.length === 0 && (
          <p className="dim">{t("Nessuna cartella concessa: i modelli non vedono i tuoi file.")}</p>
        )}
        {folders.map((folder) => (
          <div key={folder} className="folder">
            <span className="mono">{folder}</span>
            <button className="btn-link" onClick={() => saveFolders(folders.filter((f) => f !== folder))}>
              {t("Revoca")}
            </button>
          </div>
        ))}
      </section>

      <section className="section">
        <h2>{t("Strumenti delle nuove conversazioni")}</h2>
        <p className="dim note">{t("Ogni conversazione può poi cambiarli dalla barra sopra i messaggi.")}</p>
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

      <section className="section">
        <h2>{t("Sicurezza")}</h2>
        <p className="dim note">
          {t(
            "Protezioni accese di base. Chiedono qualche conferma in più: spegnile solo se sai a cosa rinunci.",
          )}
        </p>
        {security && (
          <>
            <label className="check">
              <input
                type="checkbox"
                checked={security.exfiltrationGuard}
                onChange={(e) => saveSecurity({ exfiltrationGuard: e.target.checked })}
              />
              <span>
                <strong>{t("Protezione dalla fuga di dati")}</strong>
                <span className="dim note block">
                  {t(
                    "Quando un modello vede i tuoi file e può usare il web, aprire una pagina con un indirizzo che non viene da te né da una ricerca chiede conferma. Impedisce che un testo malevolo (una pagina, un documento) faccia spedire il contenuto dei tuoi file a un sito altrui.",
                  )}
                </span>
              </span>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={security.confirmLinks}
                onChange={(e) => saveSecurity({ confirmLinks: e.target.checked })}
              />
              <span>
                <strong>{t("Conferma prima di aprire i collegamenti")}</strong>
                <span className="dim note block">
                  {t(
                    "Un collegamento in una risposta mostra l'indirizzo vero prima di aprirsi nel browser: il testo del collegamento può dire tutt'altro.",
                  )}
                </span>
              </span>
            </label>
          </>
        )}
      </section>

      <McpSection />

      <ApiSection />

      {error && <p className="error pre">{tc(error)}</p>}
    </div>
  );
}
