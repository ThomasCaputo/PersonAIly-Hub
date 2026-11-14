import { gb, type RuntimeStatus } from "../api";
import { t, tn } from "../i18n";

/**
 * Avviso mostrato quando un modello caricato in "Solo VRAM" sta in realtà
 * usando anche la RAM: il driver NVIDIA, a VRAM piena, sposta i dati nella
 * memoria di sistema invece di dare errore (vedi ADR 0003).
 */
export function SpillWarning({ status }: { status: RuntimeStatus | null }) {
  const spill = status?.vramSpillBytes;
  if (spill == null) return null;

  return (
    <div className="spill" role="alert">
      <strong>{t("Il modello sta sconfinando nella RAM.")}</strong>{" "}
      {t(
        "La VRAM è piena e il driver della scheda video sta usando circa {gb} GB di RAM al suo posto: le risposte saranno molto più lente.",
        { gb: gb(spill) },
      )}
      <details>
        <summary>{t("Come risolvere")}</summary>
        <ul>
          <li>{t("Riduci il contesto o usa una cache KV più compatta, poi ricarica il modello.")}</li>
          <li>{t("Chiudi i programmi che occupano la VRAM (giochi, browser con molte schede, altri modelli).")}</li>
          <li>
            {tn(
              "Per impedirlo del tutto: Pannello di controllo NVIDIA → Gestisci le impostazioni 3D → voce {policy} → {value}. Da quel momento un modello troppo grande darà errore invece di rallentare.",
              {
                policy: <em>CUDA - Sysmem Fallback Policy</em>,
                value: <em>Prefer No Sysmem Fallback</em>,
              },
            )}
          </li>
        </ul>
      </details>
    </div>
  );
}
