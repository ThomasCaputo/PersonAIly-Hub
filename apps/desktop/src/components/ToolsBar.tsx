import type { Approval, PermissionLevel, ToolRequest, ToolSettings } from "../api";

const LEVELS: { value: PermissionLevel; label: string; hint: string }[] = [
  { value: "ask", label: "Chiedi sempre", hint: "Ogni modifica ai file chiede la tua conferma" },
  {
    value: "edits",
    label: "Consenti modifiche ai file",
    hint: "Il modello crea e modifica file nelle cartelle concesse senza chiedere",
  },
  {
    value: "all",
    label: "Consenti tutto nelle cartelle",
    hint: "Nessuna conferma, sempre e solo dentro le cartelle concesse",
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
  return (
    <div className="tools-bar">
      <label title={folderCount === 0 ? "Concedi prima una cartella nelle Impostazioni" : undefined}>
        <input
          type="checkbox"
          checked={settings.files}
          disabled={disabled}
          onChange={(e) => onChange({ ...settings, files: e.target.checked })}
        />
        File
      </label>
      {settings.files && folderCount === 0 && (
        <button className="btn-link" onClick={onOpenSettings}>
          Concedi una cartella
        </button>
      )}
      <label title="Le parole cercate vengono inviate a DuckDuckGo">
        <input
          type="checkbox"
          checked={settings.web}
          disabled={disabled}
          onChange={(e) => onChange({ ...settings, web: e.target.checked })}
        />
        Ricerca web
      </label>
      <select
        aria-label="Permessi"
        value={settings.permission}
        disabled={disabled}
        title={LEVELS.find((l) => l.value === settings.permission)?.hint}
        onChange={(e) => onChange({ ...settings, permission: e.target.value as PermissionLevel })}
      >
        {LEVELS.map((l) => (
          <option key={l.value} value={l.value} title={l.hint}>
            {l.label}
          </option>
        ))}
      </select>
    </div>
  );
}

type RequestProps = {
  request: ToolRequest;
  onAnswer: (approval: Approval) => void;
};

/** Il modello chiede di fare qualcosa che richiede la conferma dell'utente. */
export function ToolRequestCard({ request, onAnswer }: RequestProps) {
  const args = request.arguments as Record<string, unknown>;
  const preview =
    typeof args.content === "string"
      ? args.content
      : typeof args.new_text === "string"
        ? `Sostituisce:\n${String(args.old_text ?? "")}\n\nCon:\n${args.new_text}`
        : null;

  return (
    <div className="tool-request" role="alertdialog" aria-label="Richiesta di permesso">
      <div className="tool-request-title">Il modello chiede il permesso di:</div>
      <div className="mono">{request.summary}</div>
      {preview !== null && <pre className="tool-preview">{preview}</pre>}
      <div className="row">
        <button className="btn primary" onClick={() => onAnswer("once")}>
          Consenti
        </button>
        <button className="btn" onClick={() => onAnswer("always")}>
          Consenti sempre in questa conversazione
        </button>
        <button className="btn" onClick={() => onAnswer("deny")}>
          Nega
        </button>
      </div>
    </div>
  );
}
