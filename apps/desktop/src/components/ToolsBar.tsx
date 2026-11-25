import { useEffect, useRef, useState } from "react";
import { api, type Approval, type McpServer, type PermissionLevel, type ToolRequest, type ToolSettings } from "../api";
import { msg, t, tc } from "../i18n";

const LEVELS: { value: PermissionLevel; label: string; hint: string }[] = [
  { value: "ask", label: msg("Chiedi sempre"), hint: msg("Ogni modifica ai file chiede la tua conferma") },
  {
    value: "edits",
    label: msg("Consenti modifiche ai file"),
    hint: msg("Il modello crea e modifica file nelle cartelle concesse senza chiedere"),
  },
  {
    value: "all",
    label: msg("Consenti tutto nelle cartelle"),
    hint: msg("Nessuna conferma, sempre e solo dentro le cartelle concesse"),
  },
];

type BarProps = {
  settings: ToolSettings;
  /** Cartelle concesse: senza, gli strumenti sui file non servono. */
  folderCount: number;
  disabled: boolean;
  onChange: (settings: ToolSettings) => void;
  onOpenSettings: () => void;
};

/** Strumenti e permessi della conversazione aperta. */
export function ToolsBar({ settings, folderCount, disabled, onChange, onOpenSettings }: BarProps) {
  const hint = LEVELS.find((l) => l.value === settings.permission)?.hint;
  return (
    <div className="tools-bar">
      <label title={folderCount === 0 ? t("Concedi prima una cartella nelle Impostazioni") : undefined}>
        <input
          type="checkbox"
          checked={settings.files}
          disabled={disabled}
          onChange={(e) => onChange({ ...settings, files: e.target.checked })}
        />
        {t("File")}
      </label>
      {settings.files && folderCount === 0 && (
        <button className="btn-link" onClick={onOpenSettings}>
          {t("Concedi una cartella")}
        </button>
      )}
      <label title={t("Le parole cercate vengono inviate a DuckDuckGo")}>
        <input
          type="checkbox"
          checked={settings.web}
          disabled={disabled}
          onChange={(e) => onChange({ ...settings, web: e.target.checked })}
        />
        {t("Ricerca web")}
      </label>
      <label title={t("Cerca nei documenti resi consultabili in Impostazioni")}>
        <input
          type="checkbox"
          checked={settings.documents ?? false}
          disabled={disabled}
          onChange={(e) => onChange({ ...settings, documents: e.target.checked })}
        />
        {t("Documenti")}
      </label>
      <label title={t("Istruzioni preparate per certi compiti, in Impostazioni → Skill")}>
        <input
          type="checkbox"
          checked={settings.skills ?? false}
          disabled={disabled}
          onChange={(e) => onChange({ ...settings, skills: e.target.checked })}
        />
        {t("Skill")}
      </label>
      <McpPicker settings={settings} disabled={disabled} onChange={onChange} />
      <select
        aria-label={t("Permessi")}
        value={settings.permission}
        disabled={disabled}
        title={hint && t(hint)}
        onChange={(e) => onChange({ ...settings, permission: e.target.value as PermissionLevel })}
      >
        {LEVELS.map((l) => (
          <option key={l.value} value={l.value} title={t(l.hint)}>
            {t(l.label)}
          </option>
        ))}
      </select>
    </div>
  );
}

type PickerProps = Pick<BarProps, "settings" | "disabled" | "onChange">;

/** I server MCP della conversazione (ADR 0022): un menu con i server configurati. */
function McpPicker({ settings, disabled, onChange }: PickerProps) {
  const [servers, setServers] = useState<McpServer[]>([]);
  const menu = useRef<HTMLDetailsElement>(null);
  const reload = () => api.listMcpServers().then(setServers).catch(() => {});
  // Si rilegge cambiando conversazione e quando la finestra torna in primo
  // piano: un server aggiunto nelle Impostazioni compare senza ricaricare.
  useEffect(() => {
    reload();
  }, [settings]);
  useEffect(() => {
    window.addEventListener("focus", reload);
    return () => window.removeEventListener("focus", reload);
  }, []);
  // Un clic fuori dal menu lo chiude.
  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (menu.current?.open && !menu.current.contains(e.target as Node)) menu.current.open = false;
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);
  if (servers.length === 0) return null;

  // Solo i server che esistono ancora: uno eliminato sparisce dalla scelta.
  const chosen = servers.filter((s) => (settings.mcpServers ?? []).includes(s.id)).map((s) => s.id);
  const toggle = (id: string, on: boolean) =>
    onChange({ ...settings, mcpServers: on ? [...chosen, id] : chosen.filter((c) => c !== id) });

  return (
    <details className="tools-mcp" ref={menu} onToggle={(e) => e.currentTarget.open && reload()}>
      <summary aria-disabled={disabled}>
        {chosen.length === 0 ? t("Server MCP") : t("Server MCP: {n}", { n: chosen.length })}
      </summary>
      <div className="tools-mcp-menu">
        {servers.map((s) => (
          <label key={s.id}>
            <input
              type="checkbox"
              checked={chosen.includes(s.id)}
              disabled={disabled}
              onChange={(e) => toggle(s.id, e.target.checked)}
            />
            {s.name}
          </label>
        ))}
        <p className="dim note">
          {t("Ogni strumento MCP chiede conferma finché non scegli «Consenti sempre» per quello strumento.")}
        </p>
      </div>
    </details>
  );
}

type RequestProps = {
  request: ToolRequest;
  onAnswer: (approval: Approval) => void;
  /** Chi chiede: il modello in chat o l'agente in una sessione. */
  kind?: "chat" | "agent";
};

/** Il modello chiede di fare qualcosa che richiede la conferma dell'utente. */
export function ToolRequestCard({ request, onAnswer, kind = "chat" }: RequestProps) {
  const args = request.arguments as Record<string, unknown>;
  // Per gli strumenti MCP si mostrano i parametri: è l'unico modo di sapere che cosa faranno.
  const preview = request.name.startsWith("mcp__")
    ? JSON.stringify(args, null, 2)
    : typeof args.content === "string"
      ? args.content
      : typeof args.new_text === "string"
        ? t("Sostituisce:\n{old}\n\nCon:\n{new}", { old: String(args.old_text ?? ""), new: args.new_text })
        : null;

  return (
    <div className="tool-request" role="alertdialog" aria-label={t("Richiesta di permesso")}>
      <div className="tool-request-title">
        {kind === "agent" ? t("L'agente chiede il permesso di:") : t("Il modello chiede il permesso di:")}
      </div>
      <div className="mono">{tc(request.summary)}</div>
      {preview !== null && <pre className="tool-preview">{preview}</pre>}
      <div className="row">
        <button className="btn primary" onClick={() => onAnswer("once")}>
          {t("Consenti")}
        </button>
        <button className="btn" onClick={() => onAnswer("always")}>
          {kind === "agent" ? t("Consenti sempre in questa sessione") : t("Consenti sempre in questa conversazione")}
        </button>
        <button className="btn" onClick={() => onAnswer("deny")}>
          {t("Nega")}
        </button>
      </div>
    </div>
  );
}
