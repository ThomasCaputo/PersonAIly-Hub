# 0017 — Documenti consultabili (RAG locale)

**Stato:** accettata e realizzata (20-11-2026). Provata nell'app con Gemma 3 1B al posto
di un vero modello di embedding: indice di 3 file in 15 s, aggiornamento senza
cambiamenti in 0,3 s, ricerche corrette; in chat Qwen2.5 7B usa lo strumento e cita il
file. Manca una prova con un modello di embedding vero.

## Contesto

L'utente vuole che i modelli rispondano usando i propri documenti, senza mandarli a
nessuno. Gli strumenti sui file leggono un file alla volta e solo se il modello sa quale:
con molti documenti serve una ricerca per significato.

## Decisione

- **Embedding con un secondo `llama-server`** (`--embeddings`), avviato solo quando serve
  (indicizzazione o ricerca) e chiuso dopo 5 minuti senza uso. Gira **sulla CPU**: la
  VRAM resta al modello di chat e alle regole dell'ADR 0003. Il modello è un GGUF della
  libreria scelto in Impostazioni → Documenti consultabili.
- Se il modello non dichiara il "pooling" (un modello di chat), si riavvia con
  `--pooling mean`. Per e5 e nomic si aggiungono i prefissi che vogliono
  (`query:`/`passage:`, `search_query:`/`search_document:`).
- **Indice in SQLite** (`rag_files`, `rag_chunks`): pezzi di circa 1200 caratteri che si
  sovrappongono di 200, tagliati di preferenza tra paragrafi; vettori `f32`. Si
  rielaborano solo i file con data o dimensione cambiate; quelli spariti escono
  dall'indice. Cambiando modello l'indice si svuota.
- **Ricerca**: somiglianza del coseno su tutti i pezzi, i 6 migliori. Con qualche decina
  di migliaia di pezzi non serve un indice vettoriale.
- **Solo cartelle concesse**: si indicizzano file di testo, Markdown, PDF e codice
  (fino a 20 MB), saltando `.git`, `node_modules`, `target` e simili, e senza seguire i
  collegamenti.
- Lo strumento `search_documents` (opzione **Documenti** di conversazione e agente) è in
  classe "lettura". I brani sono dati, non istruzioni (SI-5), e con lo strumento acceso
  vale la protezione dalla fuga di dati (SI-7) come per i file.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Database vettoriale esterno (Qdrant, LanceDB) | Un servizio o una dipendenza in più per volumi che SQLite regge |
| Embedding con il modello di chat già caricato | Cambia a ogni modello, occupa il suo unico slot e la sua VRAM |
| Embedding sulla GPU | Romperebbe la collocazione unica dell'ADR 0003 |

## Conseguenze

- La qualità dipende dal modello di embedding: con un modello di chat le ricerche
  funzionano ma peggio.
- Il secondo `llama-server` occupa RAM finché resta acceso (qualche centinaio di MB per
  un modello di embedding tipico).
