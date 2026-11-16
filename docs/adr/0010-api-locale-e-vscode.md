# 0010 — API locale con axum, file di blocco, estensione VS Code con diff

**Stato:** accettata — provata con un modello locale vero (Gemma 3 1B e Qwen 2.5 7B):
endpoint compatibile OpenAI con e senza streaming, conversazioni via API, estensione in un
VS Code vero (chat con il file aperto, modifica di una selezione accettata come diff,
avvio automatico di `app-server`, passaggio di consegne all'app desktop). I modelli cloud
via API sono provati solo dai test automatici, come nelle tappe precedenti.

## Contesto

La Tappa 5 realizza ciò che l'[ADR 0004](0004-nucleo-condiviso-api-locale.md) aveva
deciso a grandi linee: un'API locale protetta (VS-1, SI-4), un nucleo senza interfaccia
(VS-5) e un'estensione VS Code con chat, contesto dell'editor e diff (VS-2, VS-3, VS-4).
Restavano da scegliere il server HTTP, la forma del file di blocco, chi chiude chi, e
come il modello propone una modifica.

## Decisione

**Server HTTP: axum, nella libreria `crates/server`.** Lo stesso codice gira in due posti:
nell'eseguibile `app-server` e dentro l'app desktop, che lo avvia all'apertura.
Porta preferita fissa, **41241** (così i programmi configurati a mano la ritrovano); se è
occupata se ne usa una libera.

**Protezioni (SI-4).** Ascolto solo su `127.0.0.1`. Ogni richiesta, anche verso indirizzi
inesistenti, deve avere `Authorization: Bearer <token>` (confronto a tempo costante).
Sono respinte le richieste con intestazione `Origin` — i browser la aggiungono sempre,
quindi nessuna pagina web può usare l'API anche conoscendo il token — e quelle con `Host`
diverso da `127.0.0.1:<porta>`/`localhost:<porta>` (DNS rebinding). Il token è di 32 byte
casuali, sta nel Gestore credenziali e resta lo stesso fra un avvio e l'altro; si rigenera
da Impostazioni → API locale.

**Un solo nucleo: file di blocco `core.lock`** nella cartella dati, con porta, token, pid,
chi l'ha scritto (`app` o `server`) e versione. Si crea in modo esclusivo
(`create_new`): due processi avviati insieme non possono credersi entrambi soli. Un file
lasciato da un processo terminato male (pid non più vivo) si sostituisce.

- `app-server` trova un nucleo vivo → stampa dove si trova ed esce.
- L'app desktop trova `app-server` → gli chiede `POST /api/shutdown`, aspetta che il
  processo finisca (scaricando il modello) e prende il suo posto. I client si
  ricollegano rileggendo il file di blocco.
- L'app desktop non si chiude via API (risponde 409): la chiude solo chi la usa.
- `app-server` si chiude da solo dopo 30 minuti senza richieste e senza lavoro in corso
  (`--idle-minutes`), così un modello non resta in VRAM per sempre se VS Code si chiude
  male. L'estensione lo chiude quando si chiude la finestra che l'ha avviato.

**Due famiglie di endpoint.**

- `/v1/models`, `/v1/chat/completions` (con e senza streaming, `stream_options`,
  strumenti, immagini come data URL): compatibili con OpenAI, **senza archivio**. La
  cronologia arriva intera dal client e non finisce tra le conversazioni dell'app; gli
  strumenti dichiarati dal client li esegue il client. Nomi dei modelli: `local` per il
  modello locale caricato, `<provider>/<modello>` per quelli cloud. I parametri assenti
  si prendono dal preset del modello.
- `/api/...`: conversazioni, invio di messaggi con gli strumenti dell'app e le loro
  conferme, modelli (anche caricamento e scaricamento, solo per file della libreria),
  agenti e sessioni, registro delle azioni. Lo streaming usa Server-Sent Events con
  nome: `text`, `reasoning`, `saved`, `toolRequest`, poi `done` o `error`. Gli errori
  prevedibili (nessun modello caricato, chiave mancante) arrivano come risposta HTTP
  prima di aprire il flusso.

Se il client si scollega a metà risposta, la risposta si interrompe. Più risposte in chat
possono generare insieme (app e VS Code), una per conversazione; con il modello locale
fanno la coda in `llama-server`.

**Come il modello propone una modifica.** Si chiede il frammento modificato *per intero*
in un unico blocco di codice, non un diff né istruzioni "cerca e sostituisci": i modelli
locali piccoli sbagliano spesso i numeri di riga e le ricerche esatte, mentre riscrivere
un pezzo di codice lo fanno bene. Il pezzo è la selezione estesa a righe intere, oppure il
file intero se non c'è selezione (fino a 60 000 caratteri). L'estensione prende il blocco
di codice più lungo della risposta, sistema gli a capo, la chiusura e il rientro (i modelli
tolgono spesso il rientro comune delle righe selezionate) e apre il diff tra il file e un
documento virtuale con la proposta. Accettando, il testo si sostituisce solo se in quel
punto il file non è cambiato nel frattempo. Nulla tocca il disco: il file resta da salvare.

**Accetta / Rifiuta nella barra di stato** (e come icone nel diff), visibili solo mentre si
guarda una proposta. Una notifica con i pulsanti è stata provata e scartata: non si può
chiudere da programma, e restava a chiedere una decisione già presa.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| hyper senza framework, o un server scritto a mano | axum è sottile sopra hyper e tokio (già usati), con SSE e limiti del corpo pronti |
| Token in chiaro in un file permanente | Il file di blocco esiste solo mentre il nucleo è attivo; il token permanente sta nel portachiavi |
| Archiviare anche le richieste `/v1` | Gli altri programmi rimandano tutta la cronologia a ogni richiesta: l'archivio si riempirebbe di copie |
| Modifiche come diff unificato o "cerca e sostituisci" | Fragili con i modelli locali (vedi sopra) |
| Due nuclei (app e `app-server`) attivi insieme | Due copie dello stesso modello in memoria e due scrittori sullo stesso archivio |
| Estensione con interfaccia in React | Il pannello è piccolo: HTML e JavaScript semplici, con i colori del tema di VS Code, bastano e non richiedono un passo di build |

## Conseguenze

- Qualsiasi programma compatibile con OpenAI può usare i modelli dell'app (indirizzo e
  token in Impostazioni → API locale).
- Ad app chiusa l'estensione ha bisogno di `app-server.exe`. Dal 16-11-2026 l'installer lo
  mette accanto all'app (`bundle.externalBin` in `tauri.bundle.conf.json`, preparato da
  `scripts/build-server.mjs`), l'app annota dove si trova in `install.json` e l'estensione
  lo trova da sola; `personailyHub.serverPath` serve solo per l'exe portatile o una build.
- Le conferme degli agenti avviati da VS Code arrivano (dal 16-11-2026) come notifiche in
  VS Code, oltre che nella pagina Agenti dell'app. Aprire l'app chiude
  `app-server` e con lui le sessioni in corso, che restano interrotte e si riprendono
  scrivendo all'agente.
- I tipi dell'API sono generati dai tipi Rust in `packages/api-client` (dal 16-11-2026).
