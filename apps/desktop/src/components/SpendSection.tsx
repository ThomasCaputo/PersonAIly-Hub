import { useEffect, useState } from "react";
import { api, usd, type SpendLimits, type Spending } from "../api";
import { t, tc } from "../i18n";

/** Impostazioni → Spesa cloud: quanto si è speso nel mese e i limiti. */
export function SpendSection() {
  const [spending, setSpending] = useState<Spending | null>(null);
  const [monthly, setMonthly] = useState("");
  const [perConversation, setPerConversation] = useState("");
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .spending()
      .then((s) => {
        setSpending(s);
        setMonthly(s.limits.monthlyUsd?.toString() ?? "");
        setPerConversation(s.limits.conversationUsd?.toString() ?? "");
      })
      .catch((e) => setError(String(e)));
  }, []);

  /** Un campo vuoto vuol dire "nessun limite"; la virgola vale come punto. */
  const parse = (text: string): number | null | undefined => {
    const clean = text.trim().replace(",", ".");
    if (clean === "") return null;
    const value = Number(clean);
    return Number.isFinite(value) && value >= 0 ? value : undefined;
  };

  const save = async () => {
    const monthlyUsd = parse(monthly);
    const conversationUsd = parse(perConversation);
    setSaved(false);
    if (monthlyUsd === undefined || conversationUsd === undefined) {
      setError(t("Il limite di spesa deve essere un numero positivo."));
      return;
    }
    const limits: SpendLimits = { monthlyUsd, conversationUsd };
    setError(null);
    try {
      await api.setSpendLimits(limits);
      setSpending((s) => (s ? { ...s, limits } : s));
      setSaved(true);
    } catch (e) {
      setError(String(e));
    }
  };

  const limit = spending?.limits.monthlyUsd ?? null;
  const share = limit && limit > 0 && spending ? Math.min(1, spending.monthUsd / limit) : null;

  return (
    <section className="section">
      <h2>{t("Spesa cloud")}</h2>
      <p className="dim note">
        {t(
          "Quando un limite è raggiunto i modelli cloud non ricevono altre richieste, in chat e negli agenti. Conta il costo stimato delle risposte: i modelli di cui non si conosce il prezzo non vengono conteggiati.",
        )}
      </p>
      {spending && (
        <p>
          {t("Questo mese: circa {cost}", { cost: usd(spending.monthUsd) })}
          {limit != null && ` / ${usd(limit)}`}
        </p>
      )}
      {share != null && (
        <div className="meter-track spend-meter" aria-hidden>
          <div className={`meter-fill${share >= 0.9 ? " high" : ""}`} style={{ width: `${share * 100}%` }} />
        </div>
      )}
      <div className="row spend-row">
        <div className="field">
          <label htmlFor="spend-monthly">{t("Limite al mese ($)")}</label>
          <input
            id="spend-monthly"
            inputMode="decimal"
            placeholder={t("nessuno")}
            value={monthly}
            onChange={(e) => {
              setMonthly(e.target.value);
              setSaved(false);
            }}
          />
        </div>
        <div className="field">
          <label htmlFor="spend-conversation">{t("Limite per conversazione ($)")}</label>
          <input
            id="spend-conversation"
            inputMode="decimal"
            placeholder={t("nessuno")}
            value={perConversation}
            onChange={(e) => {
              setPerConversation(e.target.value);
              setSaved(false);
            }}
          />
        </div>
        <button className="btn" onClick={save}>
          {t("Salva")}
        </button>
      </div>
      {saved && <p className="dim note">{t("Limiti salvati.")}</p>}
      {error && <p className="error pre" role="alert">{tc(error)}</p>}
    </section>
  );
}
