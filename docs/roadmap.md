# Roadmap

Ogni tappa finisce con qualcosa che si può usare davvero. I codici tra parentesi rimandano
a [requisiti.md](requisiti.md).

## Tappa 0 — Fondamenta ✅

- Repository con la struttura definitiva, build funzionante di app vuota e nucleo.
- Controlli automatici su GitHub (formattazione, lint, test) a ogni modifica.
- Archivio SQLite con le prime tabelle.

**Fatto quando:** l'app si apre su Windows e mostra una finestra vuota; i controlli passano.

## Tappa 1 — Chat con un modello locale

- Download e avvio di `llama-server`; caricamento e scaricamento di un GGUF (LO-1, LO-2).
- Collocazione Solo VRAM / Solo RAM, cache KV `q8_0`, stima della memoria (LO-4, LO-5, LO-6).
- **Misure previste dall'ADR 0003** e scelta dei valori predefiniti.
- Chat in streaming con cronologia (CH-1, CH-2, CH-4).
- Pannello parametri e indicatori di memoria e velocità (PA-1, PA-2, PA-3, PA-5, LO-7).

**Fatto quando:** si chatta con un modello locale, cambiandone i parametri, e a modello
caricato in VRAM la RAM usata resta entro la soglia fissata dalle misure.

## Tappa 2 — Modelli cloud

- Provider Anthropic e compatibile OpenAI; chiavi nel Gestore credenziali (CL-1, CL-2, CL-4, CL-5).
- Cambio di modello a metà conversazione (CH-3); parametri filtrati per provider (PA-6).
- Preset salvabili (PA-4); download da Hugging Face (LO-3).

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
