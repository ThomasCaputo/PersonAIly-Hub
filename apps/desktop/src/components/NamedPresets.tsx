import { useCallback, useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, type NamedPreset, type Preset } from "../api";
import { t, tc } from "../i18n";

type Props = {
  preset: Preset;
  /** Il prompt di sistema della conversazione, salvato insieme ai parametri. */
  systemPrompt: string;
  /** Applica parametri e prompt di sistema di un preset con nome al modello in uso. */
  onApply: (named: NamedPreset) => void;
};

const JSON_FILTER = [{ name: "Preset", extensions: ["json"] }];

/** Preset con nome: salvare i parametri attuali, riapplicarli, esportarli e importarli. */
export function NamedPresets({ preset, systemPrompt, onApply }: Props) {
  const [presets, setPresets] = useState<NamedPreset[]>([]);
  const [selected, setSelected] = useState("");
  const [naming, setNaming] = useState<string | null>(null);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  const refresh = useCallback(async () => {
    const list = await api.listNamedPresets();
    setPresets(list);
    return list;
  }, []);

  useEffect(() => {
    refresh().catch(() => {});
  }, [refresh]);

  const run = async (action: () => Promise<string | void>) => {
    setMessage(null);
    try {
      const done = await action();
      if (done) setMessage({ text: done, error: false });
    } catch (e) {
      setMessage({ text: tc(String(e)), error: true });
    }
  };

  const current = presets.find((p) => p.name === selected);

  const saveAs = () =>
    run(async () => {
      const saved = await api.saveNamedPreset({
        name: naming ?? "",
        sampling: preset.sampling,
        systemPrompt,
      });
      setNaming(null);
      await refresh();
      setSelected(saved.name);
      return t('Salvato come "{name}".', { name: saved.name });
    });

  const remove = () =>
    run(async () => {
      if (!current || !window.confirm(t('Eliminare il preset "{name}"?', { name: current.name }))) return;
      await api.deleteNamedPreset(current.name);
      setSelected("");
      await refresh();
    });

  const exportIt = () =>
    run(async () => {
      if (!current) return;
      const path = await save({ defaultPath: `${current.name}.json`, filters: JSON_FILTER });
      if (!path) return;
      await api.exportNamedPreset(current.name, path);
      return t("Preset esportato.");
    });

  const importIt = () =>
    run(async () => {
      const path = await open({ filters: JSON_FILTER, title: t("Importa un preset") });
      if (typeof path !== "string") return;
      const imported = await api.importNamedPreset(path);
      await refresh();
      setSelected(imported.name);
      return t('Importato come "{name}".', { name: imported.name });
    });

  return (
    <div className="named-presets">
      <label htmlFor="named-preset">{t("Preset")}</label>
      <select id="named-preset" value={selected} onChange={(e) => setSelected(e.target.value)}>
        <option value="">{presets.length === 0 ? t("Nessun preset salvato") : t("Scegli un preset…")}</option>
        {presets.map((p) => (
          <option key={p.name} value={p.name}>
            {p.name}
          </option>
        ))}
      </select>

      <div className="row">
        <button className="btn" disabled={!current} onClick={() => current && onApply(current)}>
          {t("Applica")}
        </button>
        <button className="btn" onClick={() => setNaming(current?.name ?? "")}>
          {t("Salva come…")}
        </button>
        <button className="btn-link" onClick={importIt}>
          {t("Importa")}
        </button>
        {current && (
          <>
            <button className="btn-link" onClick={exportIt}>
              {t("Esporta")}
            </button>
            <button className="btn-link" onClick={remove}>
              {t("Elimina")}
            </button>
          </>
        )}
      </div>

      {naming !== null && (
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            saveAs();
          }}
        >
          <input
            type="text"
            aria-label={t("Nome del preset")}
            placeholder={t("Nome del preset")}
            autoFocus
            value={naming}
            onChange={(e) => setNaming(e.target.value)}
          />
          <button className="btn primary" type="submit" disabled={!naming.trim()}>
            {t("Salva")}
          </button>
          <button className="btn-link" type="button" onClick={() => setNaming(null)}>
            {t("Annulla")}
          </button>

        </form>
      )}

      {message && <p className={message.error ? "error pre" : "dim note"}>{message.text}</p>}
    </div>
  );
}
