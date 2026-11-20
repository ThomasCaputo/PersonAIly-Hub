import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  cloudPresetKey,
  gb,
  type CoreInfo,
  type ModelRef,
  type Preset,
  type ProviderInfo,
  type RuntimeStatus,
  type Theme,
  type UpdateInfo,
} from "./api";
import { ChatArea, type ChatCommand, type ChatRequest } from "./areas/ChatArea";
import { CommandPalette, type PaletteCommand } from "./components/CommandPalette";
import { applyTheme } from "./theme";
import { placementLabel } from "./placement";
import { msg, t } from "./i18n";
import { useLanguage } from "./LanguageProvider";
import { preloadMarkdown } from "./components/Markdown";
import { SpillWarning } from "./components/SpillWarning";
import { UpdateOffer } from "./components/UpdateSection";
import "./App.css";

// La chat è la pagina d'apertura; le altre si caricano quando si aprono.
const AgentsArea = lazy(() => import("./areas/AgentsArea").then((m) => ({ default: m.AgentsArea })));
const ModelsArea = lazy(() => import("./areas/ModelsArea").then((m) => ({ default: m.ModelsArea })));
const CompareArea = lazy(() => import("./areas/CompareArea").then((m) => ({ default: m.CompareArea })));
const SettingsArea = lazy(() => import("./areas/SettingsArea").then((m) => ({ default: m.SettingsArea })));

// Icone Material (24×24) disegnate inline: nessun font di icone da caricare.
const ICONS = {
  chat: "M20 2H4c-1.1 0-2 .9-2 2v18l4-4h14c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2zm0 14H5.17L4 17.17V4h16v12zM7 9h10v2H7zm0-3h10v2H7zm0 6h7v2H7z",
  compare: "M10 3H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h5v2h2V1h-2v2zm0 15H5l5-6v6zm9-15h-5v2h5v13l-5-6v9h5c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2z",
  agents:
    "M11 2h2v3h-2zM7 6h10a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3V9a3 3 0 0 1 3-3zm0 2a1 1 0 0 0-1 1v8a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1V9a1 1 0 0 0-1-1zm2.5 2.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3zm5 0a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3zM1 11h2v4H1zm20 0h2v4h-2zM9 15h6v1.5H9z",
  models:
    "M15 9H9v6h6V9zm-2 4h-2v-2h2v2zm8-2V9h-2V7c0-1.1-.9-2-2-2h-2V3h-2v2h-2V3H9v2H7c-1.1 0-2 .9-2 2v2H3v2h2v2H3v2h2v2c0 1.1.9 2 2 2h2v2h2v-2h2v2h2v-2h2c1.1 0 2-.9 2-2v-2h2v-2h-2v-2h2zm-4 6H7V7h10v10z",
  settings:
    "M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.49.49 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.48.48 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96a.49.49 0 0 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32a.49.49 0 0 0-.12-.61l-2.01-1.58zM12 15.6A3.6 3.6 0 1 1 12 8.4a3.6 3.6 0 0 1 0 7.2z",
} as const;

const AREAS = [
  { id: "chat", label: msg("Chat") },
  { id: "compare", label: msg("Confronto") },
  { id: "agents", label: msg("Agenti") },
  { id: "models", label: msg("Modelli") },
  { id: "settings", label: msg("Impostazioni") },
] as const;

function Icon({ path, size = 22 }: { path: string; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d={path} />
    </svg>
  );
}

type AreaId = (typeof AREAS)[number]["id"];

const STATUS_INTERVAL_MS = 2000;
/** Il controllo degli aggiornamenti aspetta che l'app sia partita. */
const UPDATE_CHECK_DELAY_MS = 5000;
const PRESET_SAVE_DELAY_MS = 500;
const ACTIVE_MODEL_KEY = "active-model";

/** Ultimo modello scelto in chat: una comodità, quindi basta la memoria del browser. */
function readStoredModel(): ModelRef {
  try {
    const stored = JSON.parse(localStorage.getItem(ACTIVE_MODEL_KEY) ?? "null");
    if (stored?.kind === "cloud" && stored.providerId && stored.model) return stored;
  } catch {
    // Memoria non disponibile o contenuto non valido: si parte dal modello locale.
  }
  return { kind: "local" };
}

export default function App() {
  // Cambiando lingua l'interfaccia intera si ridisegna da qui.
  useLanguage();
  const [areaId, setAreaId] = useState<AreaId>("chat");
  const [core, setCore] = useState<CoreInfo | null>(null);
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [activeModel, setActiveModel] = useState<ModelRef>(readStoredModel);
  const [preset, setPreset] = useState<Preset | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [chatCommand, setChatCommand] = useState<ChatCommand | null>(null);

  // Ctrl+K (o Cmd+K) apre e chiude la tavolozza dei comandi, da qualunque pagina.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  /** Un comando per la chat: si apre la pagina e glielo si passa. */
  const toChat = useCallback((request: ChatRequest) => {
    setAreaId("chat");
    setChatCommand({ ...request, nonce: Date.now() });
  }, []);

  const saveTimer = useRef<number | undefined>(undefined);

  const refreshStatus = useCallback(() => {
    api.runtimeStatus().then(setStatus).catch(() => {});
  }, []);

  const refreshProviders = useCallback(() => {
    api.listProviders().then(setProviders).catch(() => {});
  }, []);

  const paletteCommands = useMemo<PaletteCommand[]>(() => {
    const goTo = t("Vai a");
    const setTheme = (theme: Theme) => {
      applyTheme(theme);
      api
        .preferences()
        .then((p) => api.setPreferences({ ...p, theme }))
        .catch(() => {});
    };
    return [
      ...AREAS.map((a) => ({ id: `area-${a.id}`, label: t(a.label), group: goTo, run: () => setAreaId(a.id) })),
      { id: "chat-new", label: t("Nuova conversazione"), group: t("Chat"), run: () => toChat({ type: "new" }) },
      {
        id: "chat-export",
        label: t("Esporta la conversazione"),
        group: t("Chat"),
        run: () => toChat({ type: "export" }),
      },
      {
        id: "model-unload",
        label: t("Scarica il modello locale"),
        group: t("Modelli"),
        run: () => {
          api.unloadModel().then(refreshStatus).catch(() => {});
        },
      },
      { id: "theme-system", label: t("Tema: come il sistema"), group: t("Aspetto"), run: () => setTheme("system") },
      { id: "theme-light", label: t("Tema: chiaro"), group: t("Aspetto"), run: () => setTheme("light") },
      { id: "theme-dark", label: t("Tema: scuro"), group: t("Aspetto"), run: () => setTheme("dark") },
    ];
    // Ricalcolate a ogni apertura: i testi seguono la lingua scelta.
  }, [toChat, refreshStatus, paletteOpen]);

  useEffect(() => {
    api.coreInfo().then(setCore).catch(() => {});
    refreshStatus();
    refreshProviders();
    const timer = window.setInterval(refreshStatus, STATUS_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [refreshStatus, refreshProviders]);

  // Una volta per apertura della finestra, se l'utente non l'ha spento. Un
  // errore (niente rete, GitHub irraggiungibile) non merita un avviso.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  useEffect(() => {
    const timer = window.setTimeout(async () => {
      try {
        if ((await api.preferences()).checkUpdates) setUpdate(await api.checkUpdate());
      } catch {
        // Riprova l'utente da Impostazioni → Aggiornamenti.
      }
    }, UPDATE_CHECK_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);

  // Il Markdown è la parte più pesante: si carica appena la finestra è pronta,
  // invece che prima di mostrarla.
  useEffect(() => {
    const idle = window.setTimeout(preloadMarkdown, 300);
    return () => window.clearTimeout(idle);
  }, []);

  // A ogni cambio di pagina l'elenco dei provider viene riletto, così la
  // scelta del modello in chat è sempre aggiornata.
  useEffect(refreshProviders, [areaId, refreshProviders]);

  const chooseModel = useCallback((model: ModelRef) => {
    setActiveModel(model);
    try {
      localStorage.setItem(ACTIVE_MODEL_KEY, JSON.stringify(model));
    } catch {
      // Senza memoria del browser la scelta vale solo per questa sessione.
    }
  }, []);

  // Il preset in uso è quello del modello scelto: per un modello locale la
  // chiave è il percorso del file, per uno cloud provider + modello.
  const loadedPath = status?.loaded?.modelPath ?? null;
  const presetKey = useMemo(
    () => (activeModel.kind === "cloud" ? cloudPresetKey(activeModel.providerId, activeModel.model) : loadedPath),
    [activeModel, loadedPath],
  );

  useEffect(() => {
    setPreset(null);
    if (!presetKey) return;
    let current = true;
    api
      .getPreset(presetKey)
      .then((p) => current && setPreset(p))
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [presetKey]);

  // Le modifiche ai parametri valgono subito; il salvataggio su disco aspetta
  // che l'utente smetta di muovere i controlli.
  const updatePreset = useCallback(
    (next: Preset) => {
      setPreset(next);
      if (!presetKey) return;
      window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => {
        api.savePreset(presetKey, next).catch(() => {});
      }, PRESET_SAVE_DELAY_MS);
    },
    [presetKey],
  );

  return (
    <div className="shell">
      <nav className="sidebar">
        <div className="brand">
          <span className="brand-mark">
            <Icon path="M12 2l2.4 6.6L21 11l-6.6 2.4L12 20l-2.4-6.6L3 11l6.6-2.4z" size={18} />
          </span>
          PersonAIly Hub
        </div>
        {AREAS.map((a) => (
          <button
            key={a.id}
            className={a.id === areaId ? "nav-item active" : "nav-item"}
            aria-current={a.id === areaId ? "page" : undefined}
            onClick={() => setAreaId(a.id)}
          >
            <Icon path={ICONS[a.id]} />
            {t(a.label)}
          </button>
        ))}
        <StatusPanel status={status} core={core} />
      </nav>

      <main className="content">
        <SpillWarning status={status} />
        {update && (
          <div className="update-banner" role="status">
            <UpdateOffer update={update} />
            <button className="btn" onClick={() => setUpdate(null)}>
              {t("Più tardi")}
            </button>
          </div>
        )}
        <div className="area">
          <Suspense fallback={<p className="dim page">{t("Caricamento…")}</p>}>
            {areaId === "chat" && (
              <ChatArea
                command={chatCommand}
                status={status}
                providers={providers}
                activeModel={activeModel}
                onModelChange={chooseModel}
                preset={preset}
                onPresetChange={updatePreset}
                onOpenModels={() => setAreaId("models")}
                onOpenSettings={() => setAreaId("settings")}
              />
            )}
            {areaId === "models" && (
              <ModelsArea
                status={status}
                onStatusChange={refreshStatus}
                providers={providers}
                onProvidersChange={refreshProviders}
              />
            )}
            {areaId === "agents" && (
              <AgentsArea status={status} providers={providers} onOpenModels={() => setAreaId("models")} />
            )}
            {areaId === "compare" && <CompareArea status={status} providers={providers} />}
            {areaId === "settings" && <SettingsArea />}
          </Suspense>
        </div>
      </main>
      {paletteOpen && (
        <CommandPalette
          commands={paletteCommands}
          onOpenConversation={(id) => toChat({ type: "open", id })}
          onClose={() => setPaletteOpen(false)}
        />
      )}
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
          ? t("Caricamento del modello…")
          : (status?.loaded?.modelName ?? t("Nessun modello locale caricato"))}
      </div>
      {status?.loaded && (
        <div className="dim">
          {placementLabel(status.loaded.params)}
          {status.processRamBytes != null && ` · ${t("processo {gb} GB di RAM", { gb: gb(status.processRamBytes) })}`}
        </div>
      )}
      {gpu && <Meter label="VRAM" used={gpu.usedBytes} total={gpu.totalBytes} />}
      {status && <Meter label="RAM" used={ramUsed} total={status.memory.ramTotalBytes} />}
      {core && <div className="dim status-version">{t("Nucleo {version}", { version: core.version })}</div>}
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
