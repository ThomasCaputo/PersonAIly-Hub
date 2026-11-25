import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, type Skill, type SkillDraft } from "../api";
import { t, tc } from "../i18n";

const EMPTY: SkillDraft = { name: "", description: "", instructions: "" };

/** Skill (ADR 0023): istruzioni per certi compiti, caricate dal modello quando servono. */
export function SkillsSection() {
  const [skills, setSkills] = useState<Skill[]>([]);
  /** La skill in modifica: `original` è il suo nome di prima, assente per una nuova. */
  const [draft, setDraft] = useState<{ skill: SkillDraft; original?: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** Le skill dei plugin: nome della skill → nome del plugin. */
  const [owners, setOwners] = useState<Record<string, string>>({});

  const refresh = () => {
    api
      .listPlugins()
      .then((list) => setOwners(Object.fromEntries(list.flatMap((p) => p.skills.map((s) => [s, p.name])))))
      .catch(() => {});
    return api.listSkills().then(setSkills).catch((e) => setError(String(e)));
  };
  useEffect(() => {
    refresh();
  }, []);

  const run = async (action: () => Promise<unknown>) => {
    setError(null);
    setNotice(null);
    try {
      await action();
    } catch (e) {
      setError(String(e));
    }
    refresh();
  };

  const importFrom = (directory: boolean) =>
    run(async () => {
      const picked = await open(
        directory
          ? { directory: true, title: t("Cartella della skill (con SKILL.md)") }
          : { title: t("Skill in un file .zip"), filters: [{ name: "Zip", extensions: ["zip"] }] },
      );
      if (typeof picked !== "string") return;
      let outcome = await api.importSkill(picked, false);
      if (outcome.existing) {
        const question = t("Esiste già la skill «{name}»: sostituirla con quella scelta?", { name: outcome.existing });
        if (!window.confirm(question)) return;
        outcome = await api.importSkill(picked, true);
      }
      if (outcome.installed) setNotice(t("Skill «{name}» importata.", { name: outcome.installed.name }));
    });

  const save = () =>
    draft &&
    run(async () => {
      await api.saveSkill(draft.skill, draft.original ?? null);
      setDraft(null);
    });

  const remove = (skill: Skill) => {
    if (!window.confirm(t("Eliminare la skill «{name}» con tutti i suoi file?", { name: skill.name }))) return;
    run(() => api.deleteSkill(skill.name));
  };

  const setField = (change: Partial<SkillDraft>) => draft && setDraft({ ...draft, skill: { ...draft.skill, ...change } });

  return (
    <section className="section" aria-labelledby="skills-title">
      <div className="section-head">
        <h2 id="skills-title">{t("Skill")}</h2>
        {!draft && (
          <span className="row">
            <button className="btn" onClick={() => setDraft({ skill: EMPTY })}>
              {t("Nuova skill")}
            </button>
            <button className="btn" onClick={() => importFrom(true)}>
              {t("Importa una cartella")}
            </button>
            <button className="btn" onClick={() => importFrom(false)}>
              {t("Importa un .zip")}
            </button>
          </span>
        )}
      </div>
      <p className="dim note">
        {t(
          "Istruzioni pronte per certi compiti, nel formato aperto SKILL.md. Con lo strumento «Skill» acceso il modello ne vede solo nome e descrizione, e carica le istruzioni quando servono. Importa solo skill di cui ti fidi: il modello le segue come se le avessi scritte tu.",
        )}
      </p>

      {skills.length === 0 && !draft && <p className="dim">{t("Nessuna skill.")}</p>}
      {skills.map((s) => (
        <div key={s.name} className="folder skill-row">
          <label className="check">
            <input
              type="checkbox"
              checked={s.enabled && !s.error}
              disabled={!!s.error}
              aria-label={t("Accesa: {name}", { name: s.name })}
              onChange={(e) => run(() => api.setSkillEnabled(s.name, e.target.checked))}
            />
            <span>
              <strong className="mono">{s.name}</strong>{" "}
              {s.error ? (
                <span className="error">{tc(s.error)}</span>
              ) : (
                <span className="dim">
                  — {s.description}
                  {s.files.length > 0 &&
                    (s.files.length === 1 ? t(" (1 file in più)") : t(" ({n} file in più)", { n: s.files.length }))}
                </span>
              )}
              {owners[s.name] && <span className="dim"> · {t("plugin {name}", { name: owners[s.name] })}</span>}
            </span>
          </label>
          <span className="row">
            {!s.error && (
              <button
                className="btn-link"
                aria-label={t("Modifica {name}", { name: s.name })}
                onClick={() =>
                  setDraft({
                    skill: { name: s.name, description: s.description, instructions: s.instructions },
                    original: s.name,
                  })
                }
              >
                {t("Modifica")}
              </button>
            )}
            {!owners[s.name] && (
              <button className="btn-link" aria-label={t("Elimina {name}", { name: s.name })} onClick={() => remove(s)}>
                {t("Elimina")}
              </button>
            )}
          </span>
        </div>
      ))}

      {draft && (
        <div className="card mcp-form">
          <div className="field">
            <label htmlFor="skill-name">{t("Nome")}</label>
            <input
              id="skill-name"
              type="text"
              className="mono"
              value={draft.skill.name}
              placeholder="verbale-riunione"
              aria-describedby="skill-name-note"
              onChange={(e) => setField({ name: e.target.value })}
            />
            <p className="dim note" id="skill-name-note">
              {t("Solo lettere minuscole, cifre e trattini.")}
            </p>
          </div>
          <div className="field">
            <label htmlFor="skill-description">{t("Quando usarla")}</label>
            <textarea
              id="skill-description"
              rows={2}
              value={draft.skill.description}
              placeholder={t("Per esempio: Scrive il verbale di una riunione a partire dagli appunti.")}
              onChange={(e) => setField({ description: e.target.value })}
            />
          </div>
          <div className="field">
            <label htmlFor="skill-instructions">{t("Istruzioni (Markdown)")}</label>
            <textarea
              id="skill-instructions"
              className="mono"
              rows={10}
              value={draft.skill.instructions}
              onChange={(e) => setField({ instructions: e.target.value })}
            />
          </div>
          <div className="row">
            <button className="btn primary" onClick={save}>
              {t("Salva")}
            </button>
            <button className="btn" onClick={() => setDraft(null)}>
              {t("Annulla")}
            </button>
          </div>
        </div>
      )}
      <div aria-live="polite">
        {notice && <p className="note">{notice}</p>}
        {error && (
          <p className="error pre" role="alert">
            {tc(error)}
          </p>
        )}
      </div>
    </section>
  );
}
