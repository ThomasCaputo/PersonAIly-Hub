import { useEffect, useId, useRef, useState } from "react";
import { api, type ModelRef, type ProviderInfo } from "../api";
import { t, tc } from "../i18n";

type CloudRef = Extract<ModelRef, { kind: "cloud" }>;

/** Modelli al massimo in una coda (lo stesso limite del nucleo). */
const MAX_MODELS = 10;

const keyOf = (m: CloudRef) => JSON.stringify([m.providerId, m.model]);
const cloudOnly = (list: ModelRef[] | null | undefined): CloudRef[] =>
  (list ?? []).filter((m): m is CloudRef => m.kind === "cloud");

/** Nome leggibile di un modello della coda e, se c'è, perché verrà saltato. */
function describe(m: CloudRef, providers: ProviderInfo[]) {
  const provider = providers.find((p) => p.id === m.providerId);
  const model = provider?.models.find((x) => x.id === m.model);
  const name = model && model.name !== model.id ? `${model.name} (${model.id})` : m.model;
  let problem: string | null = null;
  if (!provider) problem = t("provider eliminato: verrà saltato");
  else if (!provider.hasKey) problem = t("manca la chiave API: verrà saltato");
  return { name, provider: provider?.name ?? m.providerId, problem };
}

type EditorProps = {
  queue: ModelRef[];
  providers: ProviderInfo[];
  disabled?: boolean;
  onChange: (queue: ModelRef[]) => void;
};

/** Una coda di modelli cloud: si aggiungono, si ordinano, si tolgono. */
export function FallbackEditor({ queue: raw, providers, disabled = false, onChange }: EditorProps) {
  const queue = cloudOnly(raw);
  const [choice, setChoice] = useState("");

  const move = (index: number, step: number) => {
    const next = [...queue];
    const [item] = next.splice(index, 1);
    next.splice(index + step, 0, item);
    onChange(next);
  };

  const taken = new Set(queue.map(keyOf));
  const choices = providers
    .filter((p) => p.hasKey && p.models.length > 0)
    .map((p) => ({
      provider: p,
      models: p.models.filter((m) => !taken.has(keyOf({ kind: "cloud", providerId: p.id, model: m.id }))),
    }))
    .filter((group) => group.models.length > 0);

  const add = () => {
    if (!choice) return;
    const [providerId, model] = JSON.parse(choice) as [string, string];
    setChoice("");
    onChange([...queue, { kind: "cloud", providerId, model }]);
  };

  return (
    <>
      {queue.length === 0 && <p className="dim">{t("Coda vuota: se il modello scelto non risponde, compare l'errore.")}</p>}
      <ol className="fallback-list">
        {queue.map((m, i) => {
          const info = describe(m, providers);
          return (
            <li key={keyOf(m)} className="folder">
              <span>
                <span className="mono">{info.name}</span> <span className="dim">— {info.provider}</span>
                {info.problem && <span className="dim note block">{info.problem}</span>}
              </span>
              <span className="row fallback-actions">
                <button
                  className="btn-link"
                  disabled={disabled || i === 0}
                  aria-label={t("Sposta su {name}", { name: info.name })}
                  onClick={() => move(i, -1)}
                >
                  ↑
                </button>
                <button
                  className="btn-link"
                  disabled={disabled || i === queue.length - 1}
                  aria-label={t("Sposta giù {name}", { name: info.name })}
                  onClick={() => move(i, 1)}
                >
                  ↓
                </button>
                <button
                  className="btn-link"
                  disabled={disabled}
                  aria-label={t("Togli {name} dalla coda", { name: info.name })}
                  onClick={() => onChange(queue.filter((_, k) => k !== i))}
                >
                  {t("Togli")}
                </button>
              </span>
            </li>
          );
        })}
      </ol>
      {queue.length < MAX_MODELS && (
        <div className="row">
          <select
            aria-label={t("Modello da aggiungere alla coda di riserva")}
            value={choice}
            onChange={(e) => setChoice(e.target.value)}
            disabled={disabled || choices.length === 0}
          >
            <option value="">
              {choices.length === 0 ? t("Nessun altro modello cloud con chiave API") : t("Scegli un modello…")}
            </option>
            {choices.map(({ provider, models }) => (
              <optgroup key={provider.id} label={provider.name}>
                {models.map((m) => (
                  <option key={m.id} value={JSON.stringify([provider.id, m.id])}>
                    {m.name !== m.id ? `${m.name} (${m.id})` : m.id}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
          <button className="btn" disabled={disabled || !choice} onClick={add}>
            {t("Aggiungi")}
          </button>
        </div>
      )}
    </>
  );
}

/** I provider e la coda delle Impostazioni, letti all'apertura. */
function useFallbackData() {
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [global, setGlobal] = useState<ModelRef[]>([]);
  const [error, setError] = useState<string | null>(null);
  const reload = () =>
    Promise.all([api.fallbackModels(), api.listProviders()])
      .then(([models, list]) => {
        setGlobal(cloudOnly(models));
        setProviders(list);
      })
      .catch((e) => setError(String(e)));
  useEffect(() => {
    reload();
  }, []);
  return { providers, global, setGlobal, error, setError, reload };
}

/**
 * Impostazioni → Modelli di riserva: la coda predefinita, seguita dalle chat e
 * dagli agenti che non ne hanno una propria.
 */
export function FallbackSection() {
  const { providers, global, setGlobal, error, setError } = useFallbackData();

  const save = async (next: ModelRef[]) => {
    const before = global;
    setGlobal(next);
    setError(null);
    try {
      await api.setFallbackModels(next);
    } catch (e) {
      setGlobal(before);
      setError(String(e));
    }
  };

  return (
    <section className="section">
      <h2>{t("Modelli di riserva")}</h2>
      <p className="dim note">
        {t(
          "Se il modello cloud scelto dà un errore (occupato, troppe richieste, credito o chiamate finite…), la risposta riparte dal primo modello della coda, poi dal secondo, e così via. Questa è la coda predefinita: ogni chat e ogni agente la segue finché non gliene dai una propria. Non vale nel confronto tra modelli. Ogni risposta mostra il nome del modello che l'ha scritta.",
        )}
      </p>
      <FallbackEditor queue={global} providers={providers} onChange={save} />
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </section>
  );
}

type ChoiceProps = {
  /** La coda propria; `null`/assente segue quella delle Impostazioni. */
  own: ModelRef[] | null | undefined;
  /** Di chi è la coda propria: questa chat o questo agente. */
  scope: "chat" | "agent";
  disabled?: boolean;
  onChange: (own: ModelRef[] | null) => void;
};

/**
 * La coda di una chat o di un agente: quella delle Impostazioni oppure una
 * propria, che parte come copia di quella delle Impostazioni.
 */
export function FallbackChoice({ own, scope, disabled = false, onChange }: ChoiceProps) {
  const { providers, global } = useFallbackData();
  const name = useId();
  const usesOwn = own != null;
  const shown = (list: ModelRef[]) =>
    cloudOnly(list)
      .map((m) => describe(m, providers).name)
      .join(" → ");

  return (
    <div className="fallback-choice">
      <label className="check">
        <input type="radio" name={name} checked={!usesOwn} disabled={disabled} onChange={() => onChange(null)} />
        <span>
          {t("Quella delle Impostazioni")}
          <span className="dim note block">{global.length === 0 ? t("vuota") : shown(global)}</span>
        </span>
      </label>
      <label className="check">
        <input
          type="radio"
          name={name}
          checked={usesOwn}
          disabled={disabled}
          onChange={() => onChange([...global])}
        />
        <span>{scope === "chat" ? t("Una propria per questa chat") : t("Una propria per questo agente")}</span>
      </label>
      {usesOwn && <FallbackEditor queue={own ?? []} providers={providers} disabled={disabled} onChange={onChange} />}
    </div>
  );
}

type PickerProps = {
  own: ModelRef[] | null | undefined;
  disabled: boolean;
  onChange: (own: ModelRef[] | null) => void;
};

/** Nella barra sopra i messaggi: la coda di riserva della chat, in un menu. */
export function FallbackPicker({ own, disabled, onChange }: PickerProps) {
  const menu = useRef<HTMLDetailsElement>(null);
  // Il contenuto si monta all'apertura: provider e coda delle Impostazioni si
  // rileggono ogni volta, così si vedono le modifiche fatte altrove.
  const [open, setOpen] = useState(false);
  // Un clic fuori dal menu lo chiude.
  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (menu.current?.open && !menu.current.contains(e.target as Node)) menu.current.open = false;
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);
  const label =
    own == null
      ? t("Riserva: Impostazioni")
      : own.length === 0
        ? t("Riserva: nessuna")
        : t("Riserva: propria ({n})", { n: own.length });
  return (
    <details className="tools-mcp" ref={menu} onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary aria-disabled={disabled}>{label}</summary>
      {open && (
        <div className="tools-mcp-menu fallback-menu">
          <p className="dim note">{t("Modelli cloud da provare, in ordine, se quello scelto dà un errore.")}</p>
          <FallbackChoice own={own} scope="chat" disabled={disabled} onChange={onChange} />
        </div>
      )}
    </details>
  );
}
