# PersonAIly Hub

> Stato: **Tappa 5 completata** — si chatta con modelli locali `.gguf`
> (scaricabili da Hugging Face) e con modelli cloud (Anthropic, servizi compatibili con
> OpenAI), con file, ricerca web e allegati. Gli agenti lavorano da soli su una cartella,
> con comandi nel terminale e strumenti esterni MCP. Un'API locale compatibile con OpenAI
> rende i modelli usabili da altri programmi, e un'estensione VS Code porta la chat
> nell'editor e propone le modifiche come diff. L'app è in italiano e in inglese.
> In corso: rifinitura e pubblicazione (vedi la [roadmap](docs/roadmap.md)).

Un'app desktop che riunisce in un unico posto:

- **modelli cloud** (Anthropic, OpenAI, Google e qualunque servizio compatibile con l'API OpenAI);
- **modelli locali** in formato `.gguf`, eseguiti sul proprio PC tramite llama.cpp;
- una **modalità chat** con ricerca web, allegati e accesso controllato ai file del computer;
- una **modalità agenti** per creare, avviare e seguire agenti che lavorano in autonomia;
- un **pannello parametri** per ogni modello (temperatura, top-p, contesto, layer su GPU…);
- un'**integrazione con VS Code**.

L'obiettivo è unire ciò che oggi richiede due programmi: un client tipo Claude Desktop
(chat + codice) e un gestore di modelli locali tipo Ollama / LM Studio.

Piattaforma principale: **Windows 10/11**. Linux e macOS sono previsti in un secondo momento.

## Scelte già prese

| Tema | Scelta |
|---|---|
| App desktop | Tauri 2 — interfaccia React + TypeScript, nucleo in Rust |
| Motore locale | llama.cpp (`llama-server`) avviato come processo separato |
| Formato modelli locali | GGUF |
| Cache KV | quantizzata a `q8_0` di default |
| Memoria | modello e cache risiedono **solo** in VRAM oppure **solo** in RAM, a scelta dell'utente |
| Integrazione editor | estensione VS Code che parla con il nucleo tramite API locale |

Le motivazioni sono nelle [decisioni di architettura](docs/adr/).

## Documentazione

| Documento | Contenuto |
|---|---|
| [docs/requisiti.md](docs/requisiti.md) | Cosa deve fare l'app e cosa no |
| [docs/architettura.md](docs/architettura.md) | Componenti, flussi, struttura del repository |
| [docs/roadmap.md](docs/roadmap.md) | Tappe di sviluppo |
| [docs/adr/](docs/adr/) | Decisioni tecniche e loro motivazioni |

## Sviluppo

Servono, su Windows:

- **Rust** tramite [rustup](https://rustup.rs) — la versione giusta è indicata in
  `rust-toolchain.toml` e viene installata da sola;
- **Node.js 26** (va bene anche la versione portable, purché la sua cartella sia nel `PATH`);
- **Build Tools di Visual Studio** con il carico di lavoro "Sviluppo di applicazioni
  desktop con C++";
- **WebView2** (già presente in Windows 11).

```
npm install      # una volta sola
npm run dev      # avvia l'app in modalità sviluppo
npm run check    # formattazione, lint e test del codice Rust, più il controllo delle traduzioni
npm run check:i18n -w desktop             # solo le traduzioni: elenca i testi senza inglese
npm run build    # crea l'installer
cargo build -p app-server                 # nucleo senza interfaccia (target/debug/app-server.exe)
npm run package -w personaily-hub-vscode     # estensione VS Code (extensions/vscode/personaily-hub.vsix)
```

L'estensione VS Code è descritta in [extensions/vscode/README.md](extensions/vscode/README.md).
Ad app chiusa le serve il percorso di `app-server.exe` (impostazione `personailyHub.serverPath`).

Per usare i modelli locali serve llama.cpp: l'app lo scarica da sé dalla pagina
**Modelli → Motore** (dettagli nell'[ADR 0002](docs/adr/0002-motore-locale-llama-server.md#versione-in-uso)).

## Sicurezza

I modelli agiscono solo dove l'utente lo permette: cartelle concesse, conferme per
modifiche e comandi, API locale raggiungibile solo da questo computer e protetta da un
token. Alcune protezioni chiedono qualche conferma in più e sono **accese di base**;
si possono spegnere in **Impostazioni → Sicurezza**:

- *Protezione dalla fuga di dati* — un modello che vede i tuoi file chiede conferma prima
  di aprire una pagina con un indirizzo che non viene da te né da una ricerca;
- *Conferma prima di aprire i collegamenti* — mostra l'indirizzo vero di un link prima di
  aprirlo nel browser.

I dettagli sono negli ADR [0007](docs/adr/0007-strumenti-e-permessi.md),
[0010](docs/adr/0010-api-locale-e-vscode.md) e [0011](docs/adr/0011-revisione-sicurezza-e-prestazioni.md).

## Lingue

L'app è in italiano e in inglese (**Impostazioni → Lingua**; all'inizio segue la lingua di
Windows). Chi aggiunge un testo all'interfaccia lo scrive in italiano dentro `t("…")` e ne
mette la traduzione in `apps/desktop/src/locales/en.ts`; `npm run check:i18n -w desktop`
segnala quelle mancanti ([ADR 0012](docs/adr/0012-lingue-interfaccia.md)).

## Struttura del repository

```
apps/desktop/        App Tauri (interfaccia React + guscio Rust)
crates/core/         Nucleo: provider, modelli locali, strumenti, agenti, archivio
crates/server/       API locale (usata anche dall'app) ed eseguibile app-server senza interfaccia
extensions/vscode/   Estensione VS Code: chat, contesto dell'editor, modifiche come diff
docs/                Documentazione
```

Arriverà più avanti `packages/api-client/` (client TypeScript dell'API locale, generato
dai tipi Rust).

## Licenza

Distribuito con licenza [Apache 2.0](LICENSE): si può usare, modificare e ridistribuire
liberamente, anche per scopi commerciali, mantenendo l'avviso di copyright e il file
[NOTICE](NOTICE).
