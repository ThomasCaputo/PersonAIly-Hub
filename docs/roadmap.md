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

## Tappa 3 — Strumenti ✅

Fatto ([ADR 0007](adr/0007-strumenti-e-permessi.md)):

- [x] Ciclo degli strumenti nel nucleo, per modelli locali, compatibili OpenAI e Anthropic.
- [x] Permessi a tre livelli per conversazione, con conferma "una volta" o "sempre" (SI-1, SI-2).
- [x] Accesso ai file solo nelle cartelle concesse, a prova di `..` e giunzioni (CH-7, SI-3).
- [x] Ricerca web (DuckDuckGo, senza chiave) e lettura di pagine pubbliche, con fonti (CH-6).
- [x] Il contenuto di file e pagine non può concedere permessi (SI-5).
- [x] Allegati: testo, PDF e immagini (queste ultime solo con modelli cloud) (CH-5).
- [x] Pagina Impostazioni: cartelle concesse e strumenti predefiniti.

Rimandato:

- [ ] Strumenti con Anthropic provati solo dai test automatici (manca la chiave vera, come in Tappa 2).
- [ ] Immagini con i modelli locali (richiede i file `mmproj` di llama.cpp).
- [x] Registro delle azioni consultabile a parte (SI-6): fatto nella Tappa 4.

**Fatto quando:** il modello cerca sul web, legge un file concesso e ne crea uno nuovo
dopo la conferma dell'utente.

## Tappa 4 — Agenti ✅

Fatto ([ADR 0008](adr/0008-agenti-e-terminale.md)):

- [x] Agenti: crea, modifica, duplica, elimina (AG-1).
- [x] Sessioni su un compito, seguite passo per passo con messaggi, strumenti e file
      toccati (AG-2); pausa, arresto e ripresa scrivendo all'agente (AG-3).
- [x] Vista d'insieme delle sessioni in corso e concluse (AG-4).
- [x] Terminale (PowerShell) con permessi, tempo massimo e chiusura dei processi figli (AG-5).
- [x] Registro delle azioni consultabile, per chat e agenti (SI-6).
- [x] Più sessioni insieme (AG-6); con il modello locale le richieste fanno la coda.
- [x] Prova con Qwen 2.5 7B: compito in più passi su una cartella, pausa, arresto, ripresa.
- [x] MCP con server stdio scelti per agente, strumenti in classe "esecuzione", "Consenti
      sempre" per singolo strumento, pulsante Prova (AG-7, [ADR 0009](adr/0009-mcp.md)).

Rimandato (non blocca la Tappa 5):

- [ ] Agenti con modelli cloud provati con una chiave vera (come in Tappa 2 e 3).
- [ ] MCP provato con server pubblicati (npm, PyPI); trasporto HTTP e risorse MCP.

**Fatto quando:** un agente porta a termine un compito in più passi su una cartella di
progetto, e l'utente può seguirlo e fermarlo.

## Tappa 5 — VS Code ✅

Fatto ([ADR 0010](adr/0010-api-locale-e-vscode.md)):

- [x] API locale su `127.0.0.1` con token, rifiuto delle pagine web e del DNS rebinding (SI-4).
- [x] Endpoint compatibile OpenAI: `/v1/models`, `/v1/chat/completions` con e senza
      streaming, strumenti del client, immagini (VS-1).
- [x] API propria: conversazioni con strumenti e conferme, modelli (anche caricamento),
      agenti e sessioni, registro delle azioni; streaming con Server-Sent Events.
- [x] Eseguibile `app-server` senza interfaccia, con chiusura dopo inattività (VS-5).
- [x] Un solo nucleo attivo: file di blocco `core.lock`; l'app desktop prende il posto di
      `app-server` all'apertura.
- [x] Indirizzo e token visibili nelle Impostazioni dell'app, con "Rigenera".
- [x] Più risposte in chat insieme, una per conversazione (app e VS Code).
- [x] Estensione: pannello chat con le conversazioni dell'app, scelta del modello (anche
      locale da caricare), avvio di un agente (VS-2).
- [x] File aperto o selezione passati al modello (VS-3).
- [x] Modifiche proposte come diff, da accettare o rifiutare (VS-4); `Ctrl+Alt+I` per
      chiedere una modifica alla selezione o al file.
- [x] Prova in VS Code vero con Qwen 2.5 7B: modifica a una selezione accettata come diff
      e salvata.

Rimandato (non blocca la Tappa 6):

- [ ] Percorso di `app-server.exe` da indicare a mano nell'estensione (ad app chiusa):
      l'installer dovrà fornirlo.
- [ ] Client TypeScript generato dai tipi Rust (`packages/api-client`): oggi i tipi usati
      dall'estensione sono scritti a mano.
- [ ] Le conferme degli agenti avviati da VS Code si danno solo nell'app.
- [ ] API ed estensione con modelli cloud provate solo dai test automatici (manca la chiave vera).

**Fatto quando:** da VS Code si chiede una modifica al file aperto e la si accetta come diff.

## Tappa 6 — Rifinitura e pubblicazione

- Installer, aggiornamento automatico (GE-7), lingue e temi (GE-5, GE-6).
- Guida per l'utente; nome definitivo e licenza.
- Prima versione pubblica per Windows.

Piccole cose da aggiungere (si possono fare anche prima):

- [ ] Pulizia del registro delle azioni per data: oggi cresce senza limite (ADR 0008).
- [ ] Variabili d'ambiente dei server MCP nel Gestore credenziali invece che
      nell'archivio SQLite: spesso contengono chiavi API (ADR 0009).
- [ ] Interfaccia divisa in parti caricate quando servono: oggi il file principale
      supera i 500 kB.

## Dopo

Linux e macOS (GE-2), Gemini (CL-3), modalità ibrida esplicita (LO-10), completamento del
codice in linea (VS-6), agenti che delegano ad altri agenti (AG-8).
