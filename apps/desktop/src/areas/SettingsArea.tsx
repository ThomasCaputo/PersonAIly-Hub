import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type Preferences, type SecuritySettings, type Theme, type ToolSettings } from "../api";
import { ApiSection } from "../components/ApiSection";
import { McpSection } from "../components/McpSection";
import { ToolsBar } from "../components/ToolsBar";
import { LANGUAGES, msg, t, tc, type Lang } from "../i18n";
import { useLanguage } from "../LanguageProvider";
import { applyTheme } from "../theme";

const THEMES: { id: Theme; label: string }[] = [
  { id: "system", label: msg("Come il sistema") },
  { id: "light", label: msg("Chiaro") },
  { id: "dark", label: msg("Scuro") },
];

/** Lingua, aspetto, cartelle concesse, strumenti predefiniti, protezioni, MCP e API locale. */
export function SettingsArea() {
  const { language, chooseLanguage } = useLanguage();
  const [folders, setFolders] = useState<string[]>([]);
  const [defaults, setDefaults] = useState<ToolSettings | null>(null);
  const [security, setSecurity] = useState<SecuritySettings | null>(null);
  const [preferences, setPreferences] = useState<Preferences | null>(null);
  const [tray, setTray] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.grantedFolders().then(setFolders).catch((e) => setError(String(e)));
    api.toolDefaults().then(setDefaults).catch((e) => setError(String(e)));
    api.securitySettings().then(setSecurity).catch((e) => setError(String(e)));
    api.preferences().then(setPreferences).catch((e) => setError(String(e)));
    api.runsInTray().then(setTray).catch(() => {});
  }, []);

  const savePreferences = async (change: Partial<Preferences>) => {
    if (!preferences) return;
    const next = { ...preferences, ...change };
    setPreferences(next);
    if (change.theme) applyTheme(change.theme);
    await api.setPreferences(next).catch((e) => setError(String(e)));
  };

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
              "I modelli rispondono nella lingua in cui scrivi; quando non si capisce (per esempio un messaggio fatto solo di codice), in quella scelta qui.",
            )}
          </p>
        </div>
      </section>

      {preferences && (
        <section className="section">
          <h2>{t("Aspetto")}</h2>
          <div className="field">
            <label>{t("Tema")}</label>
            <div className="segmented">
              {THEMES.map((theme) => (
                <button
                  key={theme.id}
                  className={preferences.theme === theme.id ? "active" : ""}
                  aria-pressed={preferences.theme === theme.id}
                  onClick={() => savePreferences({ theme: theme.id })}
                >
                  {t(theme.label)}
                </button>
              ))}
            </div>
          </div>
        </section>
      )}

      {preferences && tray && (
        <section className="section">
          <h2>{t("Chiusura della finestra")}</h2>
          <label className="check">
            <input
              type="checkbox"
              checked={preferences.keepRunning}
              onChange={(e) => savePreferences({ keepRunning: e.target.checked })}
            />
            <span>
              <strong>{t("Resta attiva nell'area di notifica")}</strong>
              <span className="dim note block">
                {t(
                  "Chiudendo la finestra l'interfaccia si chiude e libera la sua memoria, ma il modello caricato, gli agenti al lavoro e l'estensione VS Code continuano. Riapri l'app con un clic sull'icona vicino all'orologio; per chiudere tutto scegli Esci dal suo menu.",
                )}
              </span>
            </span>
          </label>
        </section>
      )}

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
