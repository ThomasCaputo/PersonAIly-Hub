# 0003 — Modello e cache KV in una sola memoria; KV a `q8_0`

**Stato:** accettata — misurata su un modello piccolo; resta da confermare su modelli
grandi e da realizzare il rilevamento del punto (b)

## Contesto

Con 12 GB di VRAM e 16 GB di RAM, un modello di 8–10 GB che occupa *sia* la VRAM *sia*
la RAM lascia il PC quasi senza memoria. Vogliamo che modello e cache KV stiano **in un
solo posto**, scelto dall'utente, e che la cache KV occupi meno spazio possibile senza
perdita visibile di qualità.

## Decisione

1. L'utente sceglie per ogni modello una **collocazione**: *Solo VRAM* oppure *Solo RAM*.
2. Non esiste divisione automatica tra le due. Se il modello non entra nella memoria
   scelta, l'app avvisa e non lo carica (l'utente può forzare il caricamento).
3. La cache KV è quantizzata a **`q8_0`** di default (chiavi e valori), modificabile nel
   preset (`f16` per la massima fedeltà, `q4_0` per risparmiare ancora).
4. `llama-server` non tiene **nessuna copia della cache in RAM**: né la cache dei prompt,
   né i punti di ripristino del contesto.

## Traduzione in argomenti di `llama-server`

Verificati con `llama-server --help` della build **b12110** (versione stabile v0.9.0).
Il codice sta in `crates/core/src/local/mod.rs`, funzione `build_args`.

| Argomento | Solo VRAM | Solo RAM | Perché |
|---|---|---|---|
| `--gpu-layers` | `all` | `0` | Dove stanno i pesi |
| `--device` | — | `none` | In RAM la GPU non viene toccata affatto |
| `--kv-offload` / `--no-kv-offload` | `--kv-offload` | `--no-kv-offload` | Dove sta la cache KV |
| `--cache-type-k`, `--cache-type-v` | `q8_0` | `q8_0` | Quantizzazione della cache KV |
| `--flash-attn` | `on` | `on` | Necessaria per quantizzare la cache KV |
| `--load-mode` | `dio` | `mmap` | Come viene letto il file (vedi misure) |
| `--fit` | `off` | `off` | Di default llama.cpp *adatta da solo* i parametri per far entrare il modello: è la divisione automatica che non vogliamo |
| `--cache-ram` | `0` | `0` | Di default tiene fino a 8 GB di cache dei prompt in RAM |
| `--ctx-checkpoints` | `0` | `0` | Di default salva in RAM fino a 32 copie dello stato del contesto |
| `--parallel` | `1` | `1` | Tutto il contesto alla conversazione in corso |

Nota: le vecchie opzioni `--no-mmap` e `--mlock` in questa versione sono sostituite da
`--load-mode`.

## Misure

Macchina: RTX 3060 12 GB, 16 GB di RAM, Windows 11. Modello: Gemma 3 1B Q4 (0,75 GB),
contesto 8192, cache KV `q8_0`. Si rifanno con
`cargo run -p app-core --example misura -- <cartella dati> <modello> <vram|ram> [load-mode]`.

| Collocazione | Lettura file | VRAM usata | RAM del processo | Velocità |
|---|---|---|---|---|
| Solo VRAM | `mmap` | +977 MiB | **1204 MiB** | — |
| Solo VRAM | `none` | +981 MiB | 746 MiB | — |
| Solo VRAM | `dio` | +977 MiB | **746 MiB** | 161 token/s |
| Solo RAM | `mmap` | +0 MiB | 1158 MiB | 40 token/s |

Cosa dicono:

- **Con `mmap` in modalità VRAM i pesi restano anche in RAM**: il processo occupa circa
  460 MiB in più, cioè quasi tutto il file. È il doppione da evitare. Con `none` e `dio`
  non succede.
- Tra `none` e `dio` la RAM del processo è identica. Si sceglie **`dio`** perché legge
  il file saltando la cache dei file di Windows, che altrimenti si riempirebbe con una
  copia del modello (memoria che Windows rilascia a richiesta, ma che compare come usata).
- I circa 750 MiB che restano in RAM non sono una copia del modello: sono le librerie
  CUDA, i buffer del server e la tabella del vocabolario, che llama.cpp tiene sempre in
  RAM. Su un modello da 4–8 GB ci si aspetta che questa cifra cresca poco; **va
  confermato** con la misura su un modello grande.
- In modalità RAM la GPU non viene usata affatto (+0 MiB).
- Dopo lo scaricamento VRAM e RAM tornano ai valori di partenza.
- La stima dell'app (1317 MiB) è prudente rispetto all'uso reale (977 MiB).

## Punti ancora aperti

**a) Conferma su un modello grande.** Rifare la tabella con un modello da 4 GB o più.

**b) Riversamento della VRAM nella RAM da parte del driver.** Su Windows il driver NVIDIA,
quando la VRAM finisce, può spostare dati nella "memoria condivisa" (RAM) invece di dare
errore: è proprio il doppione che vogliamo evitare, ed è molto lento. L'impostazione sta
nel pannello di controllo NVIDIA ("CUDA - Sysmem Fallback Policy") e un'app non può
cambiarla. Oggi l'app si difende solo con la stima prima del caricamento. Da fare:

- controllo, dopo il caricamento, della memoria GPU condivisa usata dal processo, con
  avviso all'utente e spiegazione di come disattivare il riversamento;
- una pagina di guida sull'impostazione.

**c) Taratura della stima.** Pesi = dimensione del file; cache KV = layer × contesto ×
2 × (dimensione di una testa × teste KV) × byte per valore; margine = 400 MiB + 3 % dei
pesi. Va confrontata con l'uso reale su più modelli, soprattutto con contesti lunghi.

**d) Effetto di `--ctx-checkpoints 0`.** Con alcuni modelli, senza punti di ripristino,
modificare un messaggio vecchio costringe a rielaborare tutta la conversazione. Da
valutare se offrire l'opzione nel preset.

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
- La VRAM si legge con `nvidia-smi`: per ora funziona solo con GPU NVIDIA. Sulle altre
  l'app non può fare il controllo preventivo e lo dice.
