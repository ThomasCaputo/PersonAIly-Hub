# 0003 — Modello e cache KV in una sola memoria; KV a `q8_0`

**Stato:** accettata e misurata; resta da realizzare il rilevamento del punto (a)

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
| `--load-mode` | `dio` | `dio` | Lettura diretta dal disco: con la mappatura in memoria i pesi finiscono due volte in RAM (vedi misure) |
| `--fit` | `off` | `off` | Di default llama.cpp *adatta da solo* i parametri per far entrare il modello: è la divisione automatica che non vogliamo |
| `--cache-ram` | `0` | `0` | Di default tiene fino a 8 GB di cache dei prompt in RAM |
| `--ctx-checkpoints` | `0` | `0` | Di default salva in RAM fino a 32 copie dello stato del contesto |
| `--parallel` | `1` | `1` | Tutto il contesto alla conversazione in corso |

Nota: le vecchie opzioni `--no-mmap` e `--mlock` in questa versione sono sostituite da
`--load-mode`.

## Misure

Macchina: RTX 3060 12 GB, 16 GB di RAM, Windows 11. Contesto 8192, cache KV `q8_0`.
Si rifanno con
`cargo run -p app-core --example misura -- <cartella dati> <modello> <vram|ram> [load-mode]`.

**Qwen2.5 7B Instruct Q4_K_M** (file da 4466 MiB; stima dell'app: 5238 MiB)

| Collocazione | Lettura file | VRAM usata | RAM del processo | Velocità |
|---|---|---|---|---|
| Solo VRAM | `mmap` | +4675 MiB | **4626 MiB** | 63 token/s |
| Solo VRAM | `dio` | +4679 MiB | **752 MiB** | 61 token/s |
| Solo RAM | `mmap` | +0 MiB | **7650 MiB** | 7,8 token/s |
| Solo RAM | `none` | +0 MiB | 4966 MiB | 8,2 token/s |
| Solo RAM | `dio` | +0 MiB | **4966 MiB** | 7,1 token/s |

**Gemma 3 1B Q4** (file da 770 MiB; stima dell'app: 1317 MiB)

| Collocazione | Lettura file | VRAM usata | RAM del processo | Velocità |
|---|---|---|---|---|
| Solo VRAM | `mmap` | +977 MiB | 1204 MiB | — |
| Solo VRAM | `none` | +981 MiB | 746 MiB | — |
| Solo VRAM | `dio` | +977 MiB | 746 MiB | 161 token/s |
| Solo RAM | `mmap` | +0 MiB | 1158 MiB | 40 token/s |

Cosa dicono:

- **In VRAM, con `mmap` l'intero modello resta anche in RAM**: 4626 MiB di RAM per un
  modello da 4466 MiB già copiato sulla GPU. Con `dio` la RAM scende a 752 MiB.
- **Quei 750 MiB circa non sono una copia del modello**: sono uguali per il modello da
  0,8 GB e per quello da 4,4 GB. Sono librerie CUDA e buffer del server.
- **In RAM, con `mmap` i pesi ci sono due volte**: 7650 MiB per un modello da 4466 MiB,
  perché llama.cpp riorganizza i pesi in un secondo buffer per velocizzare i calcoli e
  il file mappato resta in memoria. Con `none` o `dio` la RAM scende a 4966 MiB, cioè
  pesi + cache KV + buffer, senza doppioni.
- Tra `none` e `dio` la RAM del processo è identica. Si sceglie **`dio`** perché legge
  il file saltando la cache dei file di Windows, che altrimenti si riempirebbe con una
  copia del modello (memoria che Windows rilascia a richiesta, ma che compare come usata).
- La velocità non dipende dalla modalità di lettura (le differenze sono oscillazioni
  tra una prova e l'altra).
- In modalità RAM la GPU non viene usata affatto (+0 MiB).
- Dopo lo scaricamento VRAM e RAM tornano ai valori di partenza.
- La stima dell'app è prudente: 5238 MiB stimati contro 4679 (VRAM) e 4966 (RAM) reali.

## Punti ancora aperti

**a) Riversamento della VRAM nella RAM da parte del driver.** Su Windows il driver NVIDIA,
quando la VRAM finisce, può spostare dati nella "memoria condivisa" (RAM) invece di dare
errore: è proprio il doppione che vogliamo evitare, ed è molto lento. L'impostazione sta
nel pannello di controllo NVIDIA ("CUDA - Sysmem Fallback Policy") e un'app non può
cambiarla. Oggi l'app si difende solo con la stima prima del caricamento. Da fare:

- controllo, dopo il caricamento, della memoria GPU condivisa usata dal processo, con
  avviso all'utente e spiegazione di come disattivare il riversamento;
- una pagina di guida sull'impostazione.

**b) Taratura della stima.** Pesi = dimensione del file; cache KV = layer × contesto ×
2 × (dimensione di una testa × teste KV) × byte per valore; margine = 400 MiB + 3 % dei
pesi. Va confrontata con l'uso reale su più modelli, soprattutto con contesti lunghi.

**c) Effetto di `--ctx-checkpoints 0`.** Con alcuni modelli, senza punti di ripristino,
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
