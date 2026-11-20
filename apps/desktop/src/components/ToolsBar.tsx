import type { Approval, PermissionLevel, ToolRequest, ToolSettings } from "../api";
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
