# Roadmap

Ogni tappa finisce con qualcosa che si può usare davvero. I codici tra parentesi rimandano
a [requisiti.md](requisiti.md).

## Tappa 0 — Fondamenta ✅

- Repository con la struttura definitiva, build funzionante di app vuota e nucleo.
- Controlli automatici su GitHub (formattazione, lint, test) a ogni modifica.
- Archivio SQLite con le prime tabelle.

**Fatto quando:** l'app si apre su Windows e mostra una finestra vuota; i controlli passano.

## Tappa 1 — Chat con un modello locale ✅

Fatto:

- [x] Avvio di `llama-server`; caricamento e scaricamento di un GGUF (LO-1, LO-2).
- [x] Collocazione Solo VRAM / Solo RAM, cache KV `q8_0`, stima della memoria (LO-4, LO-5, LO-6).
- [x] Misure dell'ADR 0003 (modello da 0,8 GB e da 4,4 GB) e scelta dei valori predefiniti.
- [x] Chat in streaming con cronologia, interruzione, Markdown e codice evidenziato (CH-1, CH-2, CH-4).
- [x] Pannello parametri, prompt di sistema, preset salvato per modello (PA-1, PA-2, PA-3, PA-5).
- [x] Indicatori di VRAM, RAM, velocità e contesto occupato (LO-7).
- [x] Download e installazione di llama.cpp dall'app, con controllo dell'impronta dei file.
- [x] Rinomina e ricerca delle conversazioni, anche nel testo dei messaggi (CH-2).
- [x] Formule matematiche (CH-4).
- [x] Thread e dimensione del batch tra i parametri di caricamento (PA-2).
- [x] Avviso quando un modello in VRAM sconfina nella RAM (ADR 0003, punto a).

Rimandato (non blocca la Tappa 2):

- [x] Il contesto occupato si perdeva riavviando l'app: ora è salvato con ogni risposta
      (fatto nella Tappa 2). Resta noto solo dopo la prima risposta.
- [x] I file `.gguf` scaricati a metà comparivano in libreria come modelli normali: i
      download dell'app usano un file `.part` (fatto nella Tappa 2).
- [ ] Scelta manuale della variante di llama.cpp (oggi: CUDA con GPU NVIDIA, altrimenti Vulkan).

**Fatto quando:** si chatta con un modello locale, cambiandone i parametri, e a modello
caricato in VRAM la RAM usata resta entro la soglia fissata dalle misure.

## Tappa 2 — Modelli cloud ✅

Fatto:

- [x] Provider Anthropic e compatibile OpenAI, con elenco dei modelli scaricato dal servizio (CL-1, CL-2, CL-5).
- [x] Chiavi nel Gestore credenziali di Windows (CL-4).
- [x] Scelta del modello in chat e cambio a metà conversazione (CH-3).
- [x] Parametri filtrati per provider (PA-6).
- [x] Avvisi per risposte troncate o rifiutate.
- [x] Preset con nome: salva, applica, esporta e importa come file JSON (PA-4).
- [x] Ricerca e download dei modelli da Hugging Face, con ripresa dei download interrotti
      e controllo dell'impronta (LO-3, [ADR 0006](adr/0006-download-modelli.md)).
- [x] Conteggio dei token e costo stimato per risposta e per conversazione, salvati in
      archivio (CL-6).

Rimandato (non blocca la Tappa 3):

- [ ] **Prova con chiavi vere**: i provider cloud sono stati provati solo contro servizi
      finti su questo computer. Si aspetta la prima segnalazione di chi usa una chiave.
- [ ] Modelli GGUF divisi in più file (`-00001-of-00003`): non vengono proposti per il download.
- [ ] Un solo download per volta.

**Fatto quando:** nella stessa conversazione si passa da un modello locale a uno cloud.

## Tappa 3 — Strumenti

- Sistema di permessi (SI-1, SI-2, SI-3, SI-5).
- Allegati (CH-5), accesso alle cartelle concesse (CH-7), ricerca web con fonti (CH-6).

**Fatto quando:** il modello cerca sul web, legge un file concesso e ne crea uno nuovo
dopo la conferma dell'utente.

## Tappa 4 — Agenti

- Definizione degli agenti e sessioni (AG-1, AG-2, AG-3, AG-4).
- Terminale con permessi (AG-5); registro delle azioni (SI-6).
- MCP (AG-7); agenti in parallelo (AG-6).

**Fatto quando:** un agente porta a termine un compito in più passi su una cartella di
progetto, e l'utente può seguirlo e fermarlo.

## Tappa 5 — VS Code

- API locale completa e protetta, endpoint compatibile OpenAI (VS-1, SI-4).
- Eseguibile `server` senza interfaccia (VS-5).
- Estensione: pannello chat, contesto dell'editor, diff (VS-2, VS-3, VS-4).

**Fatto quando:** da VS Code si chiede una modifica al file aperto e la si accetta come diff.

## Tappa 6 — Rifinitura e pubblicazione

- Installer, aggiornamento automatico (GE-7), lingue e temi (GE-5, GE-6).
- Guida per l'utente; nome definitivo e licenza.
- Prima versione pubblica per Windows.

## Dopo

Linux e macOS (GE-2), Gemini (CL-3), modalità ibrida esplicita (LO-10), completamento del
codice in linea (VS-6), agenti che delegano ad altri agenti (AG-8).
