# Architettura

## Idea di fondo

Tutta la logica sta in un **nucleo** scritto in Rust. L'interfaccia desktop e l'estensione
VS Code sono due *client* dello stesso nucleo: non contengono logica propria su modelli,
strumenti o agenti. È questo che rende possibile l'integrazione con VS Code senza
riscrivere nulla.

```
┌──────────────────────┐      ┌──────────────────────┐      ┌─────────────────┐
│  App desktop (Tauri) │      │  Estensione VS Code  │      │  Altri client   │
│  React + TypeScript  │      │  TypeScript          │      │  (API OpenAI)   │
└──────────┬───────────┘      └──────────┬───────────┘      └────────┬────────┘
           │ comandi Tauri               │ HTTP + SSE su 127.0.0.1   │
           ▼                             ▼                           ▼
┌───────────────────────────────────────────────────────────────────────────┐
│                              NUCLEO (Rust)                                │
│                                                                           │
│  Conversazioni   Agenti   Strumenti   Permessi   Archivio   API locale    │
│                                                                           │
│  ┌─────────────────────── Provider (interfaccia unica) ────────────────┐  │
│  │  Anthropic   │   Compatibile OpenAI   │   Gemini   │   Locale       │  │
│  └──────┬───────────────────┬────────────────┬──────────────┬──────────┘  │
└─────────┼───────────────────┼────────────────┼──────────────┼─────────────┘
          ▼                   ▼                ▼              ▼
       Internet            Internet         Internet    llama-server
                                                        (processo separato,
                                                         modello .gguf)
```

## Componenti

### Nucleo — `crates/core`

Libreria Rust senza interfaccia grafica. Moduli:

| Modulo | Compito |
|---|---|
| `providers` | Interfaccia unica verso i modelli: invio messaggi in streaming, elenco modelli, capacità (visione, strumenti, contesto massimo). Un'implementazione per famiglia di API |
| `local` | Gestione di `llama-server`: avvio, arresto, controllo di salute, costruzione degli argomenti, stima della memoria, monitoraggio di VRAM e RAM |
| `models` | Libreria dei file GGUF: scansione delle cartelle scelte dall'utente |
| `gguf` | Lettura dei metadati di un file GGUF (solo l'intestazione, non i pesi) |
| `hub` | Ricerca e download di modelli GGUF da Hugging Face (file `.part`, ripresa, impronta) |
| `named_presets` | Preset con nome: parametri di generazione e prompt di sistema, esportabili e importabili. I preset per modello (caricamento e generazione) li gestisce il nucleo in `ops/library.rs` |
| `tools` | Strumenti offerti ai modelli (file, ricerca web, lettura pagine, terminale) e permessi: decide se una chiamata è consentita, negata o va chiesta all'utente |
| `mcp` | Client MCP: server stdio avviati dall'app o remoti via HTTP, loro strumenti e risorse |
| `attachments` | Allegati a un messaggio: testo, PDF e immagini |
| `chat` | Ciclo di una conversazione: messaggio → modello → eventuali strumenti → risposta |
| `agents` | Definizioni degli agenti e loro sessioni; riusa il ciclo di `chat` senza attendere l'utente a ogni passo |
| `snapshots` | Punti di ripristino delle cartelle degli agenti, con git in un repository separato |
| `rag` | Documenti consultabili: pezzi di testo, embedding e ricerca per somiglianza |
| `store` | Archivio SQLite (modalità WAL): conversazioni, messaggi con indice di ricerca a trigrammi, agenti, sessioni, impostazioni |
| `secrets` | Chiavi API nel Gestore credenziali di Windows (portachiavi di sistema sulle altre piattaforme) |
| `instance` | Un solo nucleo attivo: file di blocco `core.lock` con porta e token |
| `export` | Esportazione di una conversazione in Markdown o JSON |
| `backup` | Backup e ripristino dei dati in un file `.zip` ([ADR 0020](adr/0020-aggiornamenti-backup-registro.md)) |
| `log` | Registro diagnostico `logs/core.log`, con rotazione, e testo per le segnalazioni |

`lib.rs` contiene i tipi condivisi e `Core` con il suo costruttore; i metodi di `Core` stanno
in `ops/`, un file per argomento ([ADR 0021](adr/0021-completamento-1-0-0.md)):

| File in `ops/` | Metodi |
|---|---|
| `library` | Cartelle dei modelli, preset, preset con nome, download da Hugging Face |
| `engine` | Varianti di llama.cpp, caricamento, stato della memoria, completamento in linea, manutenzione |
| `settings` | Cartelle concesse, strumenti predefiniti, lingua, preferenze, sicurezza, spesa |
| `conversation` | Conversazioni, invio e rinvio dei messaggi, rami, esportazione |
| `rounds` | Il ciclo modello → strumenti → modello, conferme, registro delle azioni |
| `sessions` | Agenti, sessioni, deleghe, punti di ripristino |
| `mcp_servers` | Server MCP configurati |
| `documents` | Documenti consultabili (RAG) |
| `providers` | Provider cloud |
| `diagnostics` | Backup, registri, informazioni diagnostiche |

### Server — `crates/server`

Un eseguibile piccolo che avvia il nucleo ed espone l'**API locale**. Serve a due cose:

1. far funzionare l'estensione VS Code quando l'app desktop è chiusa;
2. offrire un endpoint compatibile con OpenAI (`/v1/chat/completions`, `/v1/models`)
   utilizzabile da qualsiasi programma.

Quando l'app desktop è accesa, è lei ad avviare lo stesso server dentro il proprio
processo: esiste sempre **un solo nucleo attivo**, così non si rischia di caricare due
volte un modello. Un file di blocco nella cartella dati (`core.lock`) indica porta e
token del nucleo in esecuzione; se all'apertura l'app trova `app-server` attivo, gli
chiede di chiudersi e prende il suo posto ([ADR 0010](adr/0010-api-locale-e-vscode.md)).

Il crate è insieme libreria (l'API, usata anche dall'app) ed eseguibile `app-server`.

### App desktop — `apps/desktop`

Tauri 2. Il lato Rust è sottile: crea la finestra e inoltra i comandi al nucleo.
Il nucleo vive quanto il processo, non quanto la finestra: chiudendola l'interfaccia si
distrugge e l'app resta nell'area di notifica con modello, agenti e API locale; un clic
sull'icona (o riaprire l'app, che ha una sola copia) ricrea la finestra
([ADR 0013](adr/0013-nucleo-in-area-di-notifica.md)).
Il lato web (React + TypeScript) ha cinque aree:

- **Chat** — conversazioni, allegati, selettore del modello, pannello parametri laterale;
- **Agenti** — elenco agenti, sessioni in corso, vista dettagliata dell'attività;
- **Modelli** — libreria locale, download, provider cloud e chiavi, stato della memoria;
- **Confronto** — due modelli affiancati sullo stesso messaggio;
- **Impostazioni** — lingua, tema, chiusura della finestra, cartelle, permessi predefiniti,
  protezioni di sicurezza, server MCP, API locale. La variante di llama.cpp e lo
  scaricamento per inattività stanno in Modelli → Motore.

I testi dell'interfaccia sono in italiano nel codice e passano da `t()`; la traduzione
inglese sta in `src/locales/` ([ADR 0012](adr/0012-lingue-interfaccia.md)). I comandi
Tauri che toccano archivio o disco girano fuori dal thread dell'interfaccia. La finestra
mostra solo l'app: i collegamenti esterni si aprono nel browser di sistema con il comando
`open_external` ([ADR 0011](adr/0011-revisione-sicurezza-e-prestazioni.md)).

### Estensione VS Code — `extensions/vscode`

TypeScript, senza dipendenze a runtime. Legge il file di blocco, si collega all'API
locale (avviando `app-server` se serve: lo trova da `install.json`, scritto dall'app, o
nella cartella dell'installer) e mostra un pannello chat che usa le conversazioni
dell'app. Segue gli agenti avviati da VS Code e ne mostra le richieste di conferma come
notifiche. Parla la lingua di VS Code. Aggiunge al contesto il file aperto o la selezione e presenta le
modifiche proposte come diff, da accettare o rifiutare. La logica che non dipende da VS
Code (`core.ts`, `edits.ts`) si prova con Node da sola.

### Client API — `packages/api-client`

Tipi TypeScript dell'API locale (`index.d.ts`), generati dai tipi Rust con ts-rs
(`crates/server/src/bindings.rs`): l'estensione li importa invece di tenerne una copia
scritta a mano, e `cargo test` fallisce se il file non è aggiornato.

## Modelli locali

Il nucleo non contiene llama.cpp: avvia `llama-server` come processo figlio e gli parla
via HTTP sulla macchina locale (vedi [ADR 0002](adr/0002-motore-locale-llama-server.md)).

**Ciclo di vita di un modello:**

1. l'utente sceglie il modello e la collocazione (VRAM o RAM);
2. il nucleo legge i metadati del GGUF e stima la memoria: pesi + cache KV + margine;
3. confronta la stima con la memoria libera reale; se non basta, avvisa e si ferma;
4. avvia `llama-server` con gli argomenti ricavati dal preset;
5. attende che il server risponda "pronto", poi inoltra le richieste;
6. allo scaricamento termina il processo: tutta la memoria torna al sistema.

Un processo per modello caricato. Terminare il processo è il modo più sicuro per
garantire che la memoria venga davvero liberata.

**Collocazione della memoria** — vedi [ADR 0003](adr/0003-collocazione-memoria.md).
In sintesi:

| | Solo VRAM | Solo RAM |
|---|---|---|
| Layer del modello | tutti su GPU | nessuno su GPU |
| Cache KV | su GPU | in RAM |
| Quantizzazione KV | `q8_0` | `q8_0` |
| Copia nell'altra memoria | nessuna | nessuna |

**Parametri** — due gruppi distinti, anche nell'interfaccia:

- *di caricamento* → diventano argomenti di avvio di `llama-server`; cambiarli richiede di
  ricaricare il modello (contesto, collocazione, tipo di cache KV, thread, batch);
- *di generazione* → viaggiano con ogni richiesta; hanno effetto dal messaggio successivo
  (temperatura, top-p, top-k, min-p, penalità, seed, token massimi, stop).

## Strumenti e permessi

Ogni strumento dichiara nome, schema dei parametri e **classe di rischio**:

| Classe | Esempi | Comportamento predefinito |
|---|---|---|
| Lettura | leggere file nelle cartelle concesse, cercare sul web | consentito |
| Modifica | creare o modificare file | chiede conferma |
| Esecuzione | comandi nel terminale | chiede conferma |

Flusso di una chiamata: il modello richiede uno strumento → `permissions` valuta le regole
della conversazione o dell'agente → se serve, l'interfaccia mostra la richiesta all'utente
→ lo strumento viene eseguito → il risultato torna al modello.

Il risultato di uno strumento (una pagina web, il contenuto di un file) è sempre **dato**:
non può modificare i permessi né essere trattato come un ordine dell'utente.

Né può far uscire i dati dei file: con la **protezione dalla fuga di dati** (accesa di
base, si spegne in Impostazioni → Sicurezza) un modello che vede i file chiede conferma
prima di aprire una pagina il cui indirizzo non viene dall'utente né da una ricerca
([ADR 0011](adr/0011-revisione-sicurezza-e-prestazioni.md)). Regola generale: le
protezioni che costano conferme in più sono accese di base e l'utente può spegnerle;
quelle che non costano nulla sono sempre attive.

Gli strumenti esterni si aggiungono tramite **MCP**, senza modificare il nucleo.

Non tutti i modelli locali sanno usare bene gli strumenti: `providers` espone questa
capacità e l'interfaccia segnala quando il modello scelto non è adatto.

## Dati su disco

Tutto in `%APPDATA%\<nome app>\` (cartella equivalente sulle altre piattaforme):

| Cosa | Dove |
|---|---|
| Conversazioni, agenti, sessioni, impostazioni (lingua e protezioni comprese) | `data.sqlite`, con `data.sqlite-wal` e `data.sqlite-shm` (modalità WAL) |
| Preset dei modelli | `presets\*.json` (leggibili e condivisibili) |
| Preset con nome | `presets\named\*.json` (parametri di generazione e prompt di sistema, applicabili a qualsiasi modello) |
| Binari di llama.cpp scaricati | `bin\llama.cpp\<versione>-<variante>\` (anche più varianti; quella in uso è un'impostazione) |
| File di blocco del nucleo | `core.lock` (porta, token in chiaro, pid; esiste solo mentre il nucleo è attivo; protetto dai permessi della cartella utente) |
| Dove è installato `app-server` | `install.json` (scritto dall'app a ogni avvio, letto dall'estensione VS Code) |
| Registro diagnostico e uscita di `llama-server` | `logs\core.log` (con `core.1.log`, al massimo 2 MB ciascuno), `logs\llama-server.log` |
| Copie dell'archivio | `backups\` (prima di ogni aggiornamento dello schema e prima di un ripristino) |
| Chiavi API, token dell'API locale, variabili d'ambiente dei server MCP | Gestore credenziali di Windows — **non** su disco |
| Modelli GGUF | cartelle scelte dall'utente, fuori dalla cartella dati; quelli scaricati dall'app in `<autore>\<nome>` |

## API locale

Due famiglie di endpoint, solo su `127.0.0.1` (porta preferita 41241), con token:

- `/v1/...` — compatibile con OpenAI, per i programmi esistenti: `GET /v1/models`,
  `POST /v1/chat/completions`. Senza archivio: la cronologia la manda il client. Modelli:
  `local` (quello caricato) oppure `<provider>/<modello>`;
- `/api/...` — API propria: conversazioni, invio di messaggi con strumenti e conferme
  (`POST /api/tool-requests`), modelli, caricamento e scaricamento, agenti e sessioni,
  registro delle azioni, chiusura del nucleo senza interfaccia. Le risposte in corso
  arrivano via Server-Sent Events (`text`, `reasoning`, `saved`, `toolRequest`, `modelSwitch`, `done`,
  `error`); lo stato degli agenti si legge con `GET /api/sessions/{id}/live`.

Sono respinte le richieste senza token, quelle con intestazione `Origin` (pagine web) e
quelle con `Host` diverso da `127.0.0.1`/`localhost` (vedi [ADR 0010](adr/0010-api-locale-e-vscode.md)).

## Questioni aperte

| # | Questione | Da decidere entro |
|---|---|---|
| 1 | ~~Quale servizio per la ricerca web~~ — **deciso:** DuckDuckGo senza chiave ([ADR 0007](adr/0007-strumenti-e-permessi.md)) | Tappa 3 |
| 2 | ~~Isolamento dei comandi eseguiti dagli agenti~~ — **deciso:** bastano i permessi, niente sandbox ([ADR 0008](adr/0008-agenti-e-terminale.md)) | Tappa 4 |
| 3 | Più modelli locali caricati insieme: consentirlo, e con quali limiti? Per ora uno solo: le sessioni locali in parallelo fanno la coda | Tappa 4 |
| 4 | ~~Estrazione del testo dai PDF~~ — **deciso:** crate `pdf-extract` | Tappa 3 |
| 5 | ~~Nome definitivo e licenza~~ — **deciso:** PersonAIly Hub, Apache-2.0 | Tappa 6 |
