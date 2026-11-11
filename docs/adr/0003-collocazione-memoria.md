# 0003 — Modello e cache KV in una sola memoria; KV a `q8_0`

**Stato:** accettata — i dettagli segnati con ⚠ vanno confermati con misure nella Tappa 1

## Contesto

Con 12 GB di VRAM e 16 GB di RAM, un modello di 8–10 GB che occupa *sia* la VRAM *sia*
la RAM lascia il PC quasi senza memoria. Vogliamo che modello e cache KV stiano **in un
solo posto**, scelto dall'utente, e che la cache KV occupi meno spazio possibile senza
perdita visibile di qualità.

## Decisione

1. L'utente sceglie per ogni modello una **collocazione**: *Solo VRAM* oppure *Solo RAM*.
2. Non esiste divisione automatica tra le due. Se il modello non entra nella memoria
   scelta, l'app avvisa e non lo carica.
3. La cache KV è quantizzata a **`q8_0`** di default (chiavi e valori), modificabile nel
   preset (`f16` per la massima fedeltà, `q4_0` per risparmiare ancora).

## Traduzione in argomenti di `llama-server`

| Impostazione | Solo VRAM | Solo RAM |
|---|---|---|
| Layer su GPU (`--n-gpu-layers`) | tutti | `0` |
| Cache KV sulla GPU | sì (predefinito) | no (`--no-kv-offload`) |
| Tipo cache KV (`--cache-type-k`, `--cache-type-v`) | `q8_0` | `q8_0` |
| Flash attention (`--flash-attn`) | attiva | attiva |
| Blocco in RAM (`--mlock`) | mai | facoltativo |
| Mappatura del file (`--no-mmap`) | ⚠ da misurare | mappatura attiva |

La quantizzazione dei valori della cache KV richiede la flash attention: per questo è
sempre attiva quando il tipo non è `f16`.

## Punti da verificare con misure ⚠

**a) Residuo in RAM nella modalità Solo VRAM.** Mentre carica i pesi sulla GPU,
llama.cpp legge il file dal disco. Con la mappatura in memoria (mmap) le pagine lette
possono restare conteggiate nella RAM del processo anche a caricamento finito; senza
mappatura, la lettura passa da un'area temporanea. Va misurato quale delle due lascia
meno RAM occupata a regime, e se serve chiedere a Windows di ridurre la memoria del
processo dopo il caricamento. La scelta predefinita di `--no-mmap` dipenderà dal risultato.

**b) Riversamento della VRAM nella RAM da parte del driver.** Su Windows il driver NVIDIA,
quando la VRAM finisce, può spostare dati nella "memoria condivisa" (RAM) invece di dare
errore: è proprio il doppione che vogliamo evitare, ed è molto lento. L'impostazione sta
nel pannello di controllo NVIDIA ("CUDA - Sysmem Fallback Policy") e un'app non può
cambiarla. Contromisure previste:

- stima prudente della memoria prima del caricamento, con margine;
- controllo, dopo il caricamento, della memoria condivisa usata dal processo: se cresce,
  avviso all'utente con la spiegazione di come disattivare il riversamento;
- la guida dell'app spiega l'impostazione.

**c) Precisione della stima.** Pesi = dimensione del file; cache KV = funzione di
contesto, numero di layer e tipo di quantizzazione; più un margine per i buffer di
calcolo. La formula va tarata confrontandola con l'uso reale su alcuni modelli.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Divisione automatica dei layer tra GPU e CPU (come Ollama e LM Studio di default) | È esattamente il comportamento che non vogliamo: imprevedibile e lento quando sconfina |
| Cache KV a `f16` di default | Circa il doppio della memoria per un guadagno di qualità in genere non percepibile |
| Cache KV a `q4_0` di default | Risparmia di più, ma il calo di qualità può notarsi, soprattutto con contesti lunghi |

## Conseguenze

- Comportamento prevedibile: l'utente sa sempre dove sta il modello.
- Alcuni modelli che "girerebbero comunque" in modalità mista vengono rifiutati. La
  modalità ibrida resta possibile in futuro solo come scelta esplicita (requisito LO-10).
- Serve un monitoraggio affidabile di VRAM e RAM (NVML per NVIDIA; da definire per le
  altre GPU).
