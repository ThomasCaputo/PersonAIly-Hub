# Progetto AI

> Nome provvisorio. Stato: **Tappa 1 completata** — si può caricare un modello locale
> `.gguf` e chattarci (vedi la [roadmap](docs/roadmap.md)).

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
npm run check    # formattazione, lint e test del codice Rust
npm run build    # crea l'installer
```

Per usare i modelli locali serve llama.cpp: l'app lo scarica da sé dalla pagina
**Modelli → Motore** (dettagli nell'[ADR 0002](docs/adr/0002-motore-locale-llama-server.md#versione-in-uso)).

## Struttura del repository

```
apps/desktop/        App Tauri (interfaccia React + guscio Rust)
crates/core/         Nucleo: provider, modelli locali, strumenti, agenti, archivio
crates/server/       Nucleo avviabile senza interfaccia (per VS Code e altri client)
docs/                Documentazione
```

Arriveranno con le tappe successive: `extensions/vscode/` (estensione VS Code) e
`packages/api-client/` (client TypeScript dell'API locale).

## Licenza

Non ancora scelta. Va decisa **prima** di rendere pubblico il repository: senza licenza,
nessuno ha il diritto di usare o modificare il codice.
