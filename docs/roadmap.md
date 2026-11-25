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
- [x] Scelta manuale della variante di llama.cpp (LO-9): fatto nella Tappa 6
      ([ADR 0014](adr/0014-modelli-locali-varianti-visione-download.md)).

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

- [x] **Prova con chiave vera, compatibile OpenAI**: funziona con Google AI Studio
      (13-11-2026).
- [ ] **Prova con chiave vera, Anthropic**: provato solo contro un servizio finto su
      questo computer.
- [x] Modelli GGUF divisi in più file (`-00001-of-00003`) e più download insieme: fatto
      nella Tappa 6 ([ADR 0014](adr/0014-modelli-locali-varianti-visione-download.md)).

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
- [x] Immagini con i modelli locali tramite i file `mmproj`: fatto nella Tappa 6
      ([ADR 0014](adr/0014-modelli-locali-varianti-visione-download.md)). Provato solo
      dai test automatici: manca una prova con un modello visivo vero.
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
- [ ] MCP provato con server pubblicati (npm, PyPI).
- [x] Trasporto HTTP e risorse MCP ([ADR 0021](adr/0021-completamento-1-0-0.md)).

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

- [x] Percorso di `app-server.exe`: l'installer lo mette accanto all'app, l'app annota
      dove si trova (`install.json`) e l'estensione lo trova da sola (Tappa 6). L'exe
      portatile non lo contiene.
- [x] Tipi TypeScript dell'API generati dai tipi Rust con ts-rs (`packages/api-client`),
      usati dall'estensione; `cargo test` controlla che siano aggiornati (Tappa 6).
- [x] Le conferme degli agenti avviati da VS Code arrivano come notifiche in VS Code;
      comando "Segui gli agenti al lavoro" (Tappa 6).
- [ ] API ed estensione con modelli cloud provate solo dai test automatici (manca la chiave vera).

**Fatto quando:** da VS Code si chiede una modifica al file aperto e la si accetta come diff.

## Tappa 6 — Rifinitura e pubblicazione

- [x] Installer `setup.exe` (NSIS, senza permessi di amministratore) ed exe portatile:
      `scripts/release.ps1` toglie i percorsi locali dagli eseguibili e controlla che non
      restino dati della macchina; `.github/workflows/release.yml` li costruisce su GitHub
      a ogni tag `v*` e li allega a una release in bozza.
- [x] Tema chiaro e scuro (GE-6): Impostazioni → Aspetto, di base come il sistema.
- [x] Aggiornamento automatico (GE-7): controllo all'avvio, download firmato e installazione
      ([ADR 0020](adr/0020-aggiornamenti-backup-registro.md)).
- [ ] Firma degli eseguibili (Authenticode), per togliere l'avviso di SmartScreen: script
      pronti, manca il certificato ([ADR 0021](adr/0021-completamento-1-0-0.md)).
- [x] Lingue (GE-5): italiano e inglese nell'app, scelta in Impostazioni → Lingua
      ([ADR 0012](adr/0012-lingue-interfaccia.md)). L'estensione VS Code segue la lingua di
      VS Code; i modelli rispondono nella lingua dell'utente (o in quella scelta/di sistema).
- [x] Guida per l'utente: [guida-utente.md](guida-utente.md), [user-guide.md](user-guide.md).
- [x] Nome definitivo (**PersonAIly Hub**) e licenza (**Apache-2.0**).
- [x] Prima versione pubblica per Windows (0.1.0).

Piccole cose da aggiungere (fatte):

- [x] Pulizia del registro delle azioni per data: 90 giorni di base, scelta nella scheda
      "Registro azioni" (anche "per sempre"), più il pulsante "Svuota registro" (ADR 0008).
- [x] Variabili d'ambiente dei server MCP nel Gestore credenziali invece che
      nell'archivio SQLite; quelle già salvate si spostano da sole (ADR 0009).
- [x] Interfaccia divisa in parti caricate quando servono: il file principale passa da
      899 kB a 252 kB. Agenti, Modelli e Impostazioni si caricano all'apertura; il
      Markdown (KaTeX ed evidenziazione, circa 600 kB) subito dopo la comparsa della finestra.

Revisione di sicurezza e prestazioni (14-11-2026, [ADR 0011](adr/0011-revisione-sicurezza-e-prestazioni.md)):

- [x] Protezione dalla fuga di dati (SI-7) e conferma dei collegamenti (SI-8), accese di
      base e disattivabili in Impostazioni → Sicurezza (SI-9).
- [x] La finestra non naviga più fuori dall'app; i link si aprono nel browser di sistema.
- [x] Lettura delle pagine collegata agli indirizzi già controllati (niente DNS rebinding);
      intervalli riservati completati.
- [x] Scritture dei file atomiche; limite di dimensione anche per `edit_file`.
- [x] Download senza impronta pubblicata segnalati come "solo dimensione controllata".
- [x] Comandi dell'app fuori dal thread dell'interfaccia; SQLite in WAL; ricerca nei
      messaggi con indice FTS5 a trigrammi; allegati non riletti per la visualizzazione.
- [x] llama.cpp non risulta più "non installato" se si apre Modelli mentre un modello si carica.

Rimandato:

- [x] Estensione VS Code in inglese (testi, comandi e impostazioni).
- [x] Messaggi d'errore rari del nucleo in inglese, nell'app e nell'estensione (stesso
      dizionario `core-en.ts`). Restano in italiano i testi letti solo dai modelli.
- [x] `cargo audit` e `npm audit` nei controlli automatici su GitHub (20-11-2026).
- [ ] Prova completa del clic su un collegamento dentro la risposta di un modello.

Richieste del 16-11-2026:

- [x] Nucleo staccato dall'interfaccia: chiudendo la finestra l'app resta nell'area di
      notifica con modello, agenti e API locale; la finestra si ricrea con un clic
      sull'icona o riaprendo l'app ([ADR 0013](adr/0013-nucleo-in-area-di-notifica.md)).
- [x] Scaricamento del modello dopo un periodo di inattività (LO-8), anche ad app chiusa
      nell'area di notifica e in `app-server`.
- [x] Modificare un proprio messaggio e rigenerare la risposta (CH-8); esportare la
      conversazione in Markdown (CH-9).
- [x] Risposte dei modelli nella lingua dell'utente; se non si capisce, in quella scelta
      per l'interfaccia o del sistema.

Richieste del 20-11-2026 (fatte):

Prestazioni ([misure](adr/0003-collocazione-memoria.md), revisione del 16-11):

- [x] VRAM letta dalla libreria del driver (NVML) invece di avviare `nvidia-smi` ogni 2
      secondi: lo stato risponde in 6 ms invece di 99, senza processi esterni.
- [x] Capito da dove vengono i 5,6 GB di memoria impegnata di `llama-server` in VRAM:
      è il modello di memoria di Windows (WDDM), non una copia (ADR 0003, punto d).
- [x] GE-4 riferito alla finestra chiusa (47 MB misurati; con la finestra aperta WebView2
      porta il totale a circa 450 MB).
- [x] Font di KaTeX solo in woff2: l'interfaccia passa da 2,0 a 1,3 MB.
- [x] Durante lo streaming il Markdown si rianalizza solo nell'ultimo blocco (a 3000
      token ogni ridisegno costava oltre 60 ms).
- [x] Un file di blocco lasciato da un nucleo chiuso male non blocca più l'API locale se
      Windows ha riassegnato il suo numero di processo a un altro programma.

Funzioni:

- [x] Rami della conversazione: modificare o rigenerare non cancella più, le frecce
      "‹ 2 di 3 ›" passano da una versione all'altra; esportazione in JSON con tutti i rami
      ([ADR 0019](adr/0019-rami-e-completamento-in-linea.md)).
- [x] Limite di spesa per i modelli cloud, al mese e per conversazione (Impostazioni →
      Spesa cloud), controllato prima di ogni richiesta anche negli agenti.
- [x] Tavolozza dei comandi (`Ctrl+K`): pagine, nuova conversazione, esportazione,
      scaricamento del modello, tema, ricerca nelle conversazioni. File trascinati nella
      chat come allegati.
- [x] Punti di ripristino per gli agenti ([ADR 0015](adr/0015-punti-di-ripristino-agenti.md)).
- [x] Confronto tra due modelli affiancati (pagina Confronto).
- [x] Collocazione ibrida GPU + CPU, LO-10 ([ADR 0016](adr/0016-collocazione-ibrida.md)).
- [x] Agenti che delegano ad altri agenti, AG-8 ([ADR 0018](adr/0018-deleghe-tra-agenti.md)).
- [x] Completamento del codice in linea in VS Code, VS-6 (ADR 0019).
- [x] Documenti consultabili, RAG locale ([ADR 0017](adr/0017-rag-locale.md)).

Da provare:

- [ ] Completamento in linea in VS Code con un modello per il codice (Qwen2.5-Coder).
- [ ] RAG con un vero modello di embedding (nomic-embed-text, multilingual-e5).
- [ ] Trascinamento dei file nella chat con il mouse (provato solo il collegamento all'evento).

## Verso la 1.0.0

Indispensabile:

- [x] Aggiornamento automatico firmato ([ADR 0020](adr/0020-aggiornamenti-backup-registro.md)).
- [x] Registro diagnostico e "Copia le informazioni diagnostiche" per le segnalazioni.
- [x] Backup e ripristino dei dati; copia automatica prima di ogni aggiornamento dello schema.
- [x] Prova automatica dell'aggiornamento di un archivio della 0.1.0.
- [x] Prove dell'interfaccia (vitest), con il controllo che le traduzioni del nucleo
      corrispondano ancora ai testi Rust.
- [x] `CHANGELOG.md` e `scripts/set-version.mjs`.
- [x] Chiave di firma degli aggiornamenti, salvata nei segreti di GitHub.
- [x] `cargo audit` e `npm audit` nei controlli automatici su GitHub.

- [x] Primo avvio guidato, guida per l'utente, percorsi relativi e limite di token per
      passo negli agenti, prompt caching, MCP via HTTP con risorse e catalogo,
      accessibilità, prove dell'interfaccia e del guscio Tauri, divisione di `lib.rs`
      ([ADR 0021](adr/0021-completamento-1-0-0.md)).

Ciò che resta (certificato Authenticode, prove dal vivo, tag) è in
[piano-1.0.0.md](piano-1.0.0.md).

## Dopo la 1.2

- [x] Server MCP anche in chat, scelti per conversazione e accesi tra un messaggio e l'altro
      ([ADR 0022](adr/0022-mcp-in-chat.md)).
- [x] Skill nel formato `SKILL.md`: create nell'app o importate, caricate dal modello quando
      servono, anche nei backup ([ADR 0023](adr/0023-skill.md)).
- [x] Accesso OAuth dal browser ai server MCP remoti, con registrazione dinamica e rinnovo del
      token ([ADR 0024](adr/0024-oauth-mcp.md)). Scoperta provata su Notion e Linear; accesso
      completo provato con un servizio finto.
- [ ] Accesso OAuth provato con un account vero (Notion o Linear).
- [x] Prompt dei server MCP come comandi `/` in chat ([ADR 0025](adr/0025-prompt-mcp.md)).
- [x] Plugin nel formato di Claude Code: skill, server MCP, agenti e comandi `/` in un pacchetto
      ([ADR 0026](adr/0026-plugin.md)).
- [ ] Plugin provato con uno pubblicato (finora uno scritto per la prova).

## Dopo

Linux e macOS (GE-2), Gemini (CL-3).

Idee ancora da valutare: catalogo di plugin da installare con un clic, suggerimenti per gli argomenti dei prompt MCP, più modelli locali
caricati insieme.

