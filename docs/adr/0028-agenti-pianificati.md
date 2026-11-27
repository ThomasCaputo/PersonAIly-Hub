# 0028 — Agenti pianificati (AG-9)

**Stato:** accettata e realizzata (27-11-2026). Provata con `app-server` e Qwen2.5 7B: con
niente caricato la pianificazione carica il modello, l'agente crea il file e il modello si
scarica; con lo stesso modello già caricato resta caricato; con Gemma 3 1B caricata,
"Aspetta" non la tocca e parte appena la si scarica, "Sostituiscilo" la sostituisce. Le
notifiche e "Avvia con Windows" sono provati solo dai test automatici: manca una prova
nell'app installata.

## Contesto

Oggi un agente lavora solo quando l'utente lo avvia. Si vuole poter dire "ogni mattina alle
8 riassumi i file nuovi di questa cartella" o "ogni venerdì controlla le dipendenze del
progetto", e che il lavoro parta da solo. Il modello locale deve caricarsi solo quando serve
e poi andarsene, per non tenere occupata la VRAM tutto il giorno.

Il terreno è già in buona parte pronto:

- il nucleo vive anche con la finestra chiusa, nell'area di notifica
  ([ADR 0013](0013-nucleo-in-area-di-notifica.md)), e ha già un compito di sottofondo che
  gira ogni 30 secondi (`Core::maintenance`);
- una sessione d'agente ha già registro, punti di ripristino, limite di passi, limite di
  spesa e risposta dell'utente a lavoro finito;
- il modello locale si carica e si scarica dal nucleo (`load_model`, `unload_model`).

Restano tre problemi veri. (1) Un agente con modello locale oggi usa "quello caricato"
(`ModelRef::Local`): all'ora stabilita può non esserci niente di caricato, o un altro
modello. (2) Nessuno è davanti allo schermo a confermare gli strumenti: una sessione che
chiede conferma resterebbe ferma per sempre in "Aspetta". (3) L'app può essere chiusa del
tutto, o il PC spento, proprio all'ora stabilita.

## Decisione

### Cosa si pianifica

- Una **pianificazione** lega un agente a un **compito scritto** e a una **ricorrenza**.
  Lo stesso agente può avere più pianificazioni (compiti diversi, orari diversi). Si
  possono sospendere senza cancellarle.
- Ricorrenze: **una volta** (data e ora); **ogni giorno** all'ora scelta, in tutti i giorni
  o solo in quelli scelti (un solo giorno = ogni settimana); **ogni N ore** (da 1 a 24), che
  ricomincia ogni giorno dall'ora di partenza (ogni 5 ore dalle 08:00 = 08, 13, 18, 23),
  anche questa nei giorni scelti. Niente sintassi cron nell'interfaccia: questi casi coprono
  l'uso normale e non si sbagliano.
- Gli orari sono nell'**ora locale** del PC, cambi di ora legale compresi: un'ora che quel
  giorno non esiste (passaggio all'ora legale) diventa la prima ora valida dopo, una che
  esiste due volte vale solo la prima. Serve una libreria di date con i fusi orari
  (`jiff`), che il nucleo oggi non ha.
- Ogni esecuzione apre una **sessione d'agente normale**, collegata alla pianificazione
  (`schedule_id`): si vede nella pagina Agenti con il segno "pianificata", ha il suo
  registro, i suoi punti di ripristino e si può proseguire scrivendo all'agente. Al compito
  si aggiunge una riga con data e ora dell'esecuzione, così il modello sa "quando" è.

### Quale modello

- Per un agente con modello **cloud** non c'è niente da caricare: si usa quello, con la
  sua coda di riserva ([ADR 0027](0027-modelli-di-riserva.md)).
- Per un agente con modello **locale** la pianificazione indica **quale file GGUF**. Si
  carica con i parametri del preset salvato per quel modello, come quando lo si carica a
  mano. Senza un modello scelto la pianificazione non si salva.
- **Prima di partire**, guardando cosa è caricato:
  - *niente*: si carica il modello, si lavora, e a sessione finita **si scarica**;
  - *lo stesso modello*: si usa e si lascia caricato com'era;
  - *un altro modello*: dipende dalla scelta della pianificazione, **"Aspetta"**
    (predefinita) oppure **"Sostituiscilo"**. Con "Aspetta" il modello dell'utente non si
    tocca: si riguarda ogni 30 secondi e si parte appena qualcuno lo scarica (a mano o per
    inattività). Con "Sostituiscilo" lo si sostituisce appena il motore è libero (nessuna
    risposta in chat, nessuna sessione): non si interrompe mai un lavoro in corso. In
    entrambi i casi, dopo 60 minuti di attesa l'esecuzione si **salta** e lo si annota. A
    lavoro finito il modello di prima **non** si ricarica da solo.
- **Scaricare alla fine** solo se il modello l'ha caricato la pianificazione, è ancora
  quello, e nel frattempo nessun altro ha lavorato: nessuna risposta in chat (anche dalle
  conversazioni dell'API locale), nessun completamento in linea, nessuna sessione avviata
  dall'utente. Se qualcuno l'ha usato, resta caricato e ci pensa lo scaricamento per
  inattività già esistente. Il nucleo lo sa da un contatore che cresce a ogni uso di
  questo tipo.
- (Superato dall'[ADR 0029](0029-piu-modelli-locali.md): ora vanno una alla volta solo
  quelle sullo stesso modello locale.) Le pianificazioni dovute insieme partono **una alla volta**, in ordine di orario:
  così non si contendono il motore e i caricamenti non si accavallano. Una pianificazione
  il cui lavoro precedente è ancora in corso **salta** quel turno invece di partire due
  volte.

### Senza nessuno a confermare

- Una sessione pianificata è **senza sorveglianza** (colonna `unattended`): ogni strumento
  che chiederebbe conferma viene **rifiutato** subito invece di aspettare. Al modello torna
  un messaggio chiaro ("non disponibile in un'esecuzione pianificata: nessuno può
  confermare"), e il rifiuto finisce nel registro delle azioni con l'esito `unattended`. Il
  prompt di sistema gli dice anche che nessuno lo sta guardando e di non fermarsi a fare
  domande.
- Cosa si può fare lo decide quindi il **livello di permessi dell'agente** (Chiedi /
  Modifiche / Tutto, ADR 0007) e i suoi "Consenti sempre", scelti dall'utente in anticipo.
  Il modulo della pianificazione dice già **quali strumenti verrebbero rifiutati** con i
  permessi attuali dell'agente, prima di salvare.
- Le protezioni restano come sono: le cartelle concesse, la protezione dalla fuga di dati
  con file e web insieme (che qui rifiuta invece di chiedere), il limite di spesa, il limite
  di passi e di token per passo. In più ogni esecuzione ha una **durata massima** (60 minuti
  di base, da 5 a 480), oltre la quale la sessione si ferma.
- Le **deleghe** (ADR 0018) ereditano lo stato "senza sorveglianza": anche le sessioni
  figlie rifiutano invece di chiedere.
- Se l'agente si ferma comunque per chiedere un'informazione, la sessione finisce lì e la
  notifica riporta la prima riga della domanda. L'utente risponde nella sessione come
  oggi: **rispondere spegne lo stato senza sorveglianza**, perché ora qualcuno c'è, e le
  conferme tornano a chiedersi.
- I **punti di ripristino** seguono l'impostazione dell'agente. Il modulo suggerisce di
  tenerli accesi per gli agenti che modificano file.

### Quando l'app è chiusa o il PC è spento

- Le pianificazioni girano dentro il **compito di manutenzione del nucleo**, quindi solo
  mentre il nucleo è acceso: nell'app (anche solo nell'area di notifica) o in `app-server`.
  `app-server` non si chiude per inattività mentre un'esecuzione pianificata è in corso,
  ma non resta acceso per aspettare quelle future.
- Nuova scelta **"Avvia con Windows"** (spenta di base), in cima alla scheda
  Pianificazioni: l'app parte all'accesso con l'argomento `--tray`, direttamente nell'area
  di notifica, senza finestra. La scheda avvisa anche se l'app non resta nell'area di
  notifica chiudendo la finestra (preferenza "Resta attiva", ADR 0013), perché in quel caso
  chiudere la finestra ferma anche le pianificazioni; e con quella preferenza spenta
  `--tray` apre comunque la finestra. Si realizza con la voce `Run` del registro
  dell'utente (`tauri-plugin-autostart`), non con permessi di amministratore. Non è una
  preferenza dell'archivio: lo stato vero è quello del registro di Windows.
- **Esecuzioni perse** (app chiusa, PC spento o in sospensione all'ora stabilita): appena
  il nucleo si accorge che un'esecuzione è in ritardo, applica la scelta della
  pianificazione: **"Recupera una volta"** (predefinita), che la esegue subito anche se ne
  sono state perse diverse, oppure **"Salta"**. Un ritardo oltre il periodo della
  ricorrenza (per esempio più di 24 ore per una giornaliera) si salta sempre: il turno
  successivo è già vicino.
- Il PC **non viene risvegliato** dalla sospensione: niente timer di risveglio.

### Esito e notifiche

- A ogni esecuzione conclusa, fallita, fermata o saltata arriva una **notifica di
  Windows** (`tauri-plugin-notification`) con il nome dell'agente, l'esito, la prima riga
  del resoconto e quante azioni sono state rifiutate. Si può scegliere "Sempre", "Solo se
  qualcosa va storto" (esito diverso da "finito", o azioni rifiutate) o "Mai". La notifica
  non apre la sessione con un clic (il plugin non lo permette sul desktop): nella scheda
  c'è "Apri la sessione" accanto all'ultimo esito. Le note fisse del nucleo si traducono in
  inglese nella notifica e nell'interfaccia.
- Ogni pianificazione mostra la **prossima esecuzione** e le ultime eseguite (le sessioni
  collegate). Le sessioni pianificate **non si cancellano da sole**: si eliminano a mano
  come le altre. Eliminare una pianificazione non elimina le sue sessioni.
- Un pulsante **"Esegui ora"** fa partire subito il compito, per provarlo prima di
  lasciarlo andare da solo. Segue le stesse regole di un'esecuzione pianificata, compresi
  i rifiuti degli strumenti: è proprio quello che serve vedere.
- Il registro diagnostico (ADR 0020) annota partenze, attese, salti, caricamenti e
  scaricamenti fatti dalle pianificazioni.

### Dove sta

- Nuova tabella `schedules` nell'archivio (agente, compito, ricorrenza, modello locale,
  scelte su conflitto, ritardo e notifiche, durata massima, attiva o sospesa, ultima e
  prossima esecuzione) e colonna `schedule_id` nelle sessioni. Finisce nei backup con il
  resto dei dati.
- Eliminare un agente elimina le sue pianificazioni (lo dice la conferma).
- Interfaccia: nella pagina Agenti, una scheda "Pianificazioni" e un pulsante "Pianifica"
  sulla scheda di ogni agente; le sessioni pianificate hanno il segno "pianificata"
  nell'elenco. API locale: `GET/POST /api/schedules`, `PUT/DELETE /api/schedules/{id}`,
  `POST /api/schedules/{id}/run`; nessun comando nuovo nell'estensione VS Code.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Utilità di pianificazione di Windows per ogni esecuzione | Un punto d'ingresso in più (l'app avviata "per un compito" a orari qualsiasi), voci di sistema da tenere allineate con l'archivio e da ripulire, e un meccanismo diverso su Linux. "Avvia con Windows" e il nucleo nell'area di notifica coprono quasi tutto con molto meno |
| Risvegliare il PC dalla sospensione | Sorprende chi l'ha messo in sospensione apposta; con "Recupera una volta" il lavoro si fa comunque al risveglio |
| Lasciare che le sessioni pianificate chiedano conferma e aspettino | Al ritorno dell'utente il lavoro sarebbe fermo da ore, e il modello resterebbe caricato per niente. Meglio che i permessi si decidano prima e si veda subito cosa verrebbe rifiutato |
| Concedere da sole tutti i permessi alle sessioni pianificate | Un agente che gira senza nessuno davanti è proprio il caso in cui una pagina o un file malevolo fa più danni: i permessi restano quelli scelti per l'agente |
| Usare "il modello caricato" anche per le pianificazioni | All'ora stabilita può esserci qualunque cosa, o niente: lo stesso compito darebbe risultati diversi a seconda di cosa l'utente stava facendo |
| Sostituire sempre il modello dell'utente | L'utente che torna al PC troverebbe un altro modello senza averlo chiesto; per questo "Aspetta" è la scelta predefinita |
| Espressioni cron nell'interfaccia | Facili da sbagliare e poco leggibili; le ricorrenze proposte coprono l'uso normale |
| Pianificazioni in parallelo | Con un solo motore locale si alternerebbero comunque, e i caricamenti di modelli diversi si annullerebbero a vicenda. Rivisto con più modelli caricati insieme: ora quelle su modelli diversi lavorano insieme ([ADR 0029](0029-piu-modelli-locali.md)) |

## Conseguenze

- Il nucleo deve sapere distinguere un modello "caricato dalla pianificazione" da uno
  caricato dall'utente, per decidere se scaricarlo.
- Le sessioni hanno un nuovo modo, "senza sorveglianza", che cambia la decisione dei
  permessi da "chiedi" a "rifiuta"; va provato che valga anche per deleghe e server MCP.
- Con più modelli caricati insieme (lavoro successivo) il caso "un altro modello" potrà
  diventare "affiancalo, se c'è memoria": la scelta "Aspetta / Sostituiscilo" resterà per
  quando la memoria non basta.
- Rimandato: passare all'agente il resoconto dell'esecuzione precedente ("cosa è cambiato
  da ieri"), condizioni diverse dall'orario (un file nuovo in una cartella), risveglio del
  PC.
