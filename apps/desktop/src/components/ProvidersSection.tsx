import { useState } from "react";
import { api, type Provider, type ProviderInfo, type ProviderKind } from "../api";
import { msg, t, tc } from "../i18n";

type Props = { providers: ProviderInfo[]; onChange: () => void };

/** Servizi già pronti: l'utente sceglie e deve solo incollare la chiave. */
const TEMPLATES: { label: string; name: string; kind: ProviderKind; baseUrl: string }[] = [
  { label: "Anthropic (Claude)", name: "Anthropic", kind: "anthropic", baseUrl: "https://api.anthropic.com" },
  { label: "OpenAI", name: "OpenAI", kind: "openai_compatible", baseUrl: "https://api.openai.com/v1" },
  { label: "OpenRouter", name: "OpenRouter", kind: "openai_compatible", baseUrl: "https://openrouter.ai/api/v1" },
  { label: msg("Altro servizio compatibile con OpenAI"), name: "", kind: "openai_compatible", baseUrl: "https://" },
];

type Draft = { provider: Provider; apiKey: string; hasKey: boolean };

const emptyDraft = (templateIndex: number): Draft => {
  const template = TEMPLATES[templateIndex];
  return {
    provider: { id: "", name: template.name, kind: template.kind, baseUrl: template.baseUrl, models: [] },
    apiKey: "",
    hasKey: false,
  };
};

/** Provider cloud: elenco, aggiunta, modifica della chiave, aggiornamento dei modelli. */
export function ProvidersSection({ providers, onChange }: Props) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [templateIndex, setTemplateIndex] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async (label: string, action: () => Promise<void>) => {
    setBusy(label);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
      onChange();
    }
  };

  const save = () =>
    run("save", async () => {
      if (!draft) return;
      const saved = await api.saveProvider(draft.provider, draft.apiKey);
      setDraft(null);
      // Con una chiave disponibile si scarica subito l'elenco dei modelli.
      if (saved.hasKey) await api.refreshProviderModels(saved.id);
    });

  const remove = (provider: ProviderInfo) => {
    if (!window.confirm(t("Eliminare {name} e la sua chiave API?", { name: provider.name }))) return;
    run(provider.id, () => api.deleteProvider(provider.id));
  };

  return (
    <section className="section">
      <div className="section-head">
        <h2>{t("Provider cloud")}</h2>
        {!draft && (
          <button className="btn" onClick={() => setDraft(emptyDraft(templateIndex))}>
            {t("Aggiungi provider")}
          </button>
        )}
      </div>

      {providers.length === 0 && !draft && (
        <p className="dim">
          {t("Collega un servizio come Anthropic o OpenAI con la tua chiave API per usarne i modelli in chat.")}
        </p>
      )}

      {providers.map((p) => (
        <div key={p.id} className="card provider">
          <div className="card-lead">
            <span className="card-icon cloud" aria-hidden="true">
              <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor">
                <path d="M19.35 10.04A7.49 7.49 0 0 0 12 4C9.11 4 6.6 5.64 5.35 8.04A5.994 5.994 0 0 0 0 14c0 3.31 2.69 6 6 6h13c2.76 0 5-2.24 5-5 0-2.64-2.05-4.78-4.65-4.96zM19 18H6c-2.21 0-4-1.79-4-4 0-2.05 1.53-3.76 3.56-3.97l1.07-.11.5-.95A5.469 5.469 0 0 1 12 6c2.62 0 4.88 1.86 5.39 4.43l.3 1.5 1.53.11A2.98 2.98 0 0 1 22 15c0 1.65-1.35 3-3 3z" />
              </svg>
            </span>
            <div>
              <div className="card-title">{p.name}</div>
              <div className="dim">
                {p.hasKey ? t("{n} modelli", { n: p.models.length }) : t("Chiave API mancante")} ·{" "}
                <span className="mono">{p.baseUrl}</span>
              </div>
            </div>
          </div>
          <div className="row">
            <button
              className="btn"
              disabled={busy !== null || !p.hasKey}
              onClick={() =>
                run(p.id, async () => {
                  await api.refreshProviderModels(p.id);
                })
              }
            >
              {busy === p.id ? t("Aggiornamento…") : t("Aggiorna modelli")}
            </button>
            <button
              className="btn"
              disabled={busy !== null}
              onClick={() => setDraft({ provider: { ...p }, apiKey: "", hasKey: p.hasKey })}
            >
              {t("Modifica")}
            </button>
            <button className="btn-link" disabled={busy !== null} onClick={() => remove(p)}>
              {t("Elimina")}
            </button>
          </div>
        </div>
      ))}

      {draft && (
        <div className="card card-body provider-form">
          {draft.provider.id === "" && (
            <div className="field">
              <label htmlFor="provider-template">{t("Servizio")}</label>
              <select
                id="provider-template"
                value={templateIndex}
                onChange={(e) => {
                  const index = Number(e.target.value);
                  setTemplateIndex(index);
                  setDraft({ ...emptyDraft(index), apiKey: draft.apiKey });
                }}
              >
                {TEMPLATES.map((template, i) => (
                  <option key={template.label} value={i}>
                    {t(template.label)}
                  </option>
                ))}
              </select>
            </div>
          )}
          <div className="field">
            <label htmlFor="provider-name">{t("Nome")}</label>
            <input
              id="provider-name"
              type="text"
              value={draft.provider.name}
              onChange={(e) => setDraft({ ...draft, provider: { ...draft.provider, name: e.target.value } })}
            />
          </div>
          <div className="field">
            <label htmlFor="provider-url">{t("Indirizzo dell'API")}</label>
            <input
              id="provider-url"
              type="text"
              className="mono"
              value={draft.provider.baseUrl}
              onChange={(e) => setDraft({ ...draft, provider: { ...draft.provider, baseUrl: e.target.value } })}
            />
          </div>
          <div className="field">
            <label htmlFor="provider-key">{t("Chiave API")}</label>
            <input
              id="provider-key"
              type="password"
              autoComplete="off"
              placeholder={
                draft.hasKey ? t("Già salvata — lascia vuoto per non cambiarla") : t("Incolla qui la chiave")
              }
              value={draft.apiKey}
              onChange={(e) => setDraft({ ...draft, apiKey: e.target.value })}
            />
            <span className="dim note">
              {t(
                "Viene salvata nel Gestore credenziali di Windows, non nei file dell'app, e inviata solo all'indirizzo qui sopra.",
              )}
            </span>
          </div>
          <div className="row">
            <button className="btn primary" disabled={busy !== null} onClick={save}>
              {busy === "save" ? t("Salvataggio…") : t("Salva")}
            </button>
            <button className="btn" disabled={busy !== null} onClick={() => setDraft(null)}>
              {t("Annulla")}
            </button>
          </div>
        </div>
      )}

      {error && <p className="error pre" role="alert">{tc(error)}</p>}

    </section>
  );
}
