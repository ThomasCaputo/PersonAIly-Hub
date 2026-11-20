# Piano per la 1.0.0

Dalla 0.1.0, già pubblicata, si passa direttamente alla 1.0.0: deve essere completa e
affidabile. Questo file raccoglie ciò che si era deciso di fare il 20-11-2026, in ordine di
importanza. Quando una voce è finita si segna `[x]` e, se serve, si rimanda all'ADR o alla
tappa della [roadmap](roadmap.md).

Già fatto (20-11-2026, [ADR 0020](adr/0020-aggiornamenti-backup-registro.md)):
aggiornamento automatico firmato e chiave nei segreti di GitHub, registro diagnostico,
backup e ripristino, prova dell'aggiornamento di un archivio della 0.1.0, prove
dell'interfaccia con vitest, `cargo audit` e `npm audit` nei controlli automatici,
`CHANGELOG.md` e `scripts/set-version.mjs`.

Fatto in seguito (20-11-2026, [ADR 0021](adr/0021-completamento-1-0-0.md)): tutto ciò che
non richiede il titolare del progetto o una prova dal vivo.

## 1. Indispensabile prima di pubblicare

### Prove dal vivo

Funzioni scritte e coperte dai test automatici, ma mai provate in condizioni vere:

- [ ] **Anthropic con una chiave vera**: chat, strumenti, agenti, API locale ed
      estensione VS Code (Tappe 2, 3, 4, 5 della roadmap), più il prompt caching
      (`cache_read_input_tokens` maggiore di zero dalla seconda richiesta).
- [ ] **Immagini con un modello locale** e il suo componente visivo `mmproj` (ADR 0014).
- [ ] **RAG con un vero modello di embedding** (nomic-embed-text, multilingual-e5; ADR 0017).
- [ ] **Completamento in linea in VS Code** con un modello per il codice (Qwen2.5-Coder; ADR 0019).
- [ ] **Trascinamento dei file nella chat** con il mouse.
- [ ] **Clic su un collegamento** dentro la risposta di un modello (ADR 0011).
- [ ] **MCP con server pubblicati** (npm, PyPI), non solo con il server di prova, e un
      server remoto via HTTP (per esempio GitHub con un token; ADR 0021).
- [ ] **Backup, ripristino e informazioni diagnostiche nell'app vera**: provati solo
      dai test automatici e dalla build, non dalle schermate.
- [ ] **Aggiornamento automatico**: la prima prova vera è possibile solo da una 1.0.0
      installata verso una versione successiva (per esempio una 1.0.1 di prova).
      Chi ha la 0.1.0 deve installare la 1.0.0 a mano una volta.
- [ ] **Primo avvio guidato su un PC (o un profilo) vuoto**, fino al download del modello.
- [ ] **Agente con Qwen2.5 7B** sullo stesso compito che il 20-11 si incastrava sui percorsi
      lunghi, con i percorsi relativi e il limite di token per passo.

### Prove dell'interfaccia

- [x] Prove automatiche dei percorsi principali: chat con risposta in streaming e
      richiesta di permesso, caricamento di un modello, agente in attesa di conferma
      (`src/paths.test.tsx`, vitest con happy-dom e un finto nucleo); primo avvio e
      contrasti dei temi (ADR 0021).
- [x] Prove del guscio Tauri (`apps/desktop/src-tauri`): indirizzi apribili, pagine della
      finestra, forma degli eventi, menu dell'area di notifica, comandi chiamati da `api.ts`
      tutti registrati.

### Prima del tag

- [x] Scritto nel README che la 1.0.0 è **solo per Windows** (Linux e macOS restano dopo
      la 1.0.0).
- [x] Aggiornata la sezione "Status" del README.
- [x] `node scripts/set-version.mjs 1.0.0` e data nella sezione `## [1.0.0]` di
      `CHANGELOG.md` (da correggere se il tag arriva un altro giorno), `npm run check`.
- [ ] Commit e tag `v1.0.0` (il tag avvia la build della release su GitHub).

## 2. Da aggiungere per una 1.0.0 completa

- [x] **Primo avvio guidato.** Riconosce la GPU, installa la variante giusta di llama.cpp
      e propone un modello iniziale adatto alla VRAM, oppure collega un provider cloud.
- [x] **Guida per l'utente**: [guida-utente.md](guida-utente.md) e
      [user-guide.md](user-guide.md) (installazione, primi passi, modelli locali e
      collocazione della memoria, agenti e permessi, MCP, VS Code, backup).
- [x] **Percorsi relativi per gli strumenti sui file degli agenti**, più un limite di token
      per passo con nuovo tentativo invece dell'arresto.
- [x] **Prompt caching con Anthropic**, contato nel costo stimato.
- [x] **MCP via HTTP** ("streamable HTTP") e risorse MCP, più un piccolo catalogo di
      server noti da aggiungere con un clic. OAuth non c'è: si usa un token.
- [x] **Accessibilità.** Navigazione da tastiera, annunci per i lettori di schermo,
      etichette, contrasti dei due temi controllati da una prova (ADR 0021).

## 3. Pulizia del codice

- [x] **Divisi i metodi di `crates/core/src/lib.rs`** (oltre 4000 righe) in `src/ops/`:
      `lib.rs` ne ha meno di 600, le prove stanno in `src/tests.rs`.

## Dopo la 1.0.0

- Linux e macOS (GE-2): terminale senza job di Windows, contatori della VRAM, WebKit.
- Gemini con la sua API (CL-3). Oggi funziona già come servizio compatibile OpenAI
  (provato con Google AI Studio).
- Più modelli locali caricati insieme (questione aperta n. 3 dell'[architettura](architettura.md)).
- OAuth per i server MCP remoti e prompt MCP.

### Firma degli eseguibili

- [ ] **Firma Authenticode** di `setup.exe`, dell'exe portatile e di `app-server.exe`.
      Oggi Windows SmartScreen avvisa a ogni installazione. Per i progetti open source
      SignPath offre un certificato gratuito: la richiesta va fatta dal titolare del
      progetto. Nota: la firma degli aggiornamenti (minisign) non toglie questo avviso.
  - [x] Script pronti (ADR 0021): `scripts/sign.ps1` firma con SignPath o con un
        certificato installato; `release.ps1` firma `app-server.exe`, app e installatore
        prima della firma degli aggiornamenti e controlla il risultato; `release.yml` passa i
        segreti `SIGNPATH_API_TOKEN`, `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`,
        `SIGNPATH_POLICY_SLUG`. Senza segreti la build resta non firmata, con un avviso.
  - [ ] Richiesta del certificato a SignPath e segreti su GitHub (titolare del progetto).
        Alla prima release firmata, controllare che i nomi dei parametri di
        `Submit-SigningRequest` corrispondano al modulo SignPath installato.