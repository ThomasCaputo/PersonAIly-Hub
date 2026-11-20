# 0016 — Collocazione ibrida GPU + CPU (LO-10)

**Stato:** accettata e realizzata (20-11-2026), misurata con Qwen2.5 7B. Integra
l'[ADR 0003](0003-collocazione-memoria.md), che resta valido per le collocazioni di base.

## Contesto

L'ADR 0003 tiene modello e cache in una sola memoria, per evitare divisioni silenziose e
lente. Un modello che non entra nella VRAM oggi va tutto in RAM, molto più lento anche
quando la GPU potrebbe portarne una buona parte. Il requisito LO-10 chiede la divisione
come **scelta esplicita e avanzata**.

## Decisione

- Terza collocazione **Ibrida (avanzata)**: l'utente sceglie quanti layer vanno sulla GPU
  (`--gpu-layers N --kv-offload`); la cache KV di ogni layer sta dove sta il layer.
  Niente valori automatici: il numero lo decide l'utente, con un cursore e la stima.
- La stima divide pesi e cache in proporzione ai layer; i buffer di calcolo stanno sulla
  GPU; alla RAM si aggiungono 750 MiB per le librerie CUDA e i buffer di scambio. Prima
  del caricamento si controllano **entrambe** le memorie.
- Il controllo dello sconfinamento della VRAM (ADR 0003, punto a) **non** vale in
  collocazione ibrida: lì i pesi dei layer in RAM compaiono come memoria GPU "condivisa"
  e il contatore non distingue uno sconfinamento.

## Misure

Qwen2.5 7B Q4_K_M (28 layer), contesto 32256, cache `q8_0`, RTX 3060 12 GB:

| Layer sulla GPU | VRAM stimata / reale | RAM stimata / reale | Velocità |
|---|---|---|---|
| 7 | 1,74 / 1,78 GB | 4,06 / 4,31 GB | 8,8 token/s |
| 14 | 3,09 / 2,93 GB | 2,70 / 3,20 GB | 11,9 token/s |
| 21 | 4,45 / 4,04 GB | 1,35 / 2,08 GB | 21,5 token/s |
| tutti (solo VRAM) | — | — | 63 token/s |

La memoria condivisa del processo è 3,0 / 2,0 / 1,1 GB: ecco perché il controllo dello
sconfinamento non si usa. Le stime RAM qui sopra sono quelle prima della quota di 750 MiB,
aggiunta dopo queste misure.

## Conseguenze

- È lenta: anche con tre quarti dei layer sulla GPU va a un terzo della sola VRAM.
  L'interfaccia lo dice accanto al cursore.
- Serve sapere quanti layer ha il modello (dal file GGUF); se non si sa, l'opzione è spenta.
