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
| `models` | Libreria dei file GGUF: scansione cartelle, lettura dei metadati, download da Hugging Face |
| `presets` | Parametri per modello, divisi in *caricamento* e *generazione* |
| `tools` | Strumenti offerti ai modelli: file, ricerca web, lettura pagine, terminale, client MCP |
| `permissions` | Decide se una chiamata a uno strumento è consentita, negata o va chiesta all'utente |
| `chat` | Ciclo di una conversazione: messaggio → modello → eventuali strumenti → risposta |
| `agents` | Definizioni degli agenti e loro sessioni; riusa il ciclo di `chat` senza attendere l'utente a ogni passo |
| `store` | Archivio SQLite: conversazioni, messaggi, agenti, sessioni, impostazioni |
| `secrets` | Chiavi API nel Gestore credenziali di Windows (portachiavi di sistema sulle altre piattaforme) |

### Server — `crates/server`

Un eseguibile piccolo che avvia il nucleo ed espone l'**API locale**. Serve a due cose:

1. far funzionare l'estensione VS Code quando l'app desktop è chiusa;
2. offrire un endpoint compatibile con OpenAI (`/v1/chat/completions`, `/v1/models`)
   utilizzabile da qualsiasi programma.

Quando l'app desktop è aperta, è lei ad avviare lo stesso server dentro il proprio
processo: esiste sempre **un solo nucleo attivo**, così non si rischia di caricare due
volte un modello. Un file di blocco nella cartella dati (`core.lock`) indica porta e
token del nucleo in esecuzione; se all'apertura l'app trova `app-server` attivo, gli
chiede di chiudersi e prende il suo posto ([ADR 0010](adr/0010-api-locale-e-vscode.md)).

Il crate è insieme libreria (l'API, usata anche dall'app) ed eseguibile `app-server`.

### App desktop — `apps/desktop`

Tauri 2. Il lato Rust è sottile: crea la finestra e inoltra i comandi al nucleo.
Il lato web (React + TypeScript) ha quattro aree:

- **Chat** — conversazioni, allegati, selettore del modello, pannello parametri laterale;
- **Agenti** — elenco agenti, sessioni in corso, vista dettagliata dell'attività;
- **Modelli** — libreria locale, download, provider cloud e chiavi, stato della memoria;
- **Impostazioni** — cartelle, permessi predefiniti, variante di llama.cpp, lingua, tema.

### Estensione VS Code — `extensions/vscode`

TypeScript, senza dipendenze a runtime. Legge il file di blocco, si collega all'API
locale (avviando `app-server` se serve) e mostra un pannello chat che usa le
conversazioni dell'app. Aggiunge al contesto il file aperto o la selezione e presenta le
modifiche proposte come diff, da accettare o rifiutare. La logica che non dipende da VS
Code (`core.ts`, `edits.ts`) si prova con Node da sola.

### Client API — `packages/api-client`

Tipi TypeScript e funzioni per chiamare l'API locale, generati dalle definizioni Rust per
non doverli tenere allineati a mano. **Non ancora realizzato**: per ora l'estensione ha i
suoi tipi scritti a mano.

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

Gli strumenti esterni si aggiungono tramite **MCP**, senza modificare il nucleo.

Non tutti i modelli locali sanno usare bene gli strumenti: `providers` espone questa
capacità e l'interfaccia segnala quando il modello scelto non è adatto.

## Dati su disco

Tutto in `%APPDATA%\<nome app>\` (cartella equivalente sulle altre piattaforme):

| Cosa | Dove |
|---|---|
| Conversazioni, agenti, sessioni, impostazioni | `data.sqlite` |
| Preset dei modelli | `presets\*.json` (leggibili e condivisibili) |
| Preset con nome | `presets\named\*.json` (parametri di generazione e prompt di sistema, applicabili a qualsiasi modello) |
| Binari di llama.cpp scaricati | `bin\llama.cpp\<versione>-<variante>\` |
| File di blocco del nucleo | `core.lock` (porta, token, pid; esiste solo mentre il nucleo è attivo) |
| Chiavi API e token dell'API locale | Gestore credenziali di Windows — **non** su disco |
| Modelli GGUF | cartelle scelte dall'utente, fuori dalla cartella dati |

## API locale

Due famiglie di endpoint, solo su `127.0.0.1` (porta preferita 41241), con token:

- `/v1/...` — compatibile con OpenAI, per i programmi esistenti: `GET /v1/models`,
  `POST /v1/chat/completions`. Senza archivio: la cronologia la manda il client. Modelli:
  `local` (quello caricato) oppure `<provider>/<modello>`;
- `/api/...` — API propria: conversazioni, invio di messaggi con strumenti e conferme
  (`POST /api/tool-requests`), modelli, caricamento e scaricamento, agenti e sessioni,
  registro delle azioni, chiusura del nucleo senza interfaccia. Le risposte in corso
  arrivano via Server-Sent Events (`text`, `reasoning`, `saved`, `toolRequest`, `done`,
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
| 5 | Nome definitivo e licenza | Prima della pubblicazione |
