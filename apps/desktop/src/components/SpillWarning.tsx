import { gb, type RuntimeStatus } from "../api";

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
      <strong>Il modello sta sconfinando nella RAM.</strong> La VRAM è piena e il driver della scheda video sta usando
      circa {gb(spill)} GB di RAM al suo posto: le risposte saranno molto più lente.
      <details>
        <summary>Come risolvere</summary>
        <ul>
          <li>Riduci il contesto o usa una cache KV più compatta, poi ricarica il modello.</li>
          <li>Chiudi i programmi che occupano la VRAM (giochi, browser con molte schede, altri modelli).</li>
          <li>
            Per impedirlo del tutto: Pannello di controllo NVIDIA → Gestisci le impostazioni 3D → voce{" "}
            <em>CUDA - Sysmem Fallback Policy</em> → <em>Prefer No Sysmem Fallback</em>. Da quel momento un modello
            troppo grande darà errore invece di rallentare.
          </li>
        </ul>
      </details>
    </div>
  );
}
