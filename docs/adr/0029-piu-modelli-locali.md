# 0029 — Più modelli locali caricati insieme, e agenti che cooperano

**Stato:** accettata e realizzata (27-11-2026). Provata con `app-server`: Qwen2.5 7B (4,6 GB
di VRAM) e Gemma 3 1B (1,0 GB) caricati insieme su una RTX 3060 da 12 GB, con 4,6 GB ancora
disponibili tolto il margine; `local`, `local/<nome>` e il principale rispondono con il
modello giusto, un nome sconosciuto dà errore; un coordinatore su Qwen ha fatto partire due
aiutanti nello stesso secondo, uno su Qwen e uno su Gemma (quest'ultimo si è fermato perché
il template di Gemma 3 1B non prevede gli strumenti, come l'app ha segnalato). L'interfaccia
è provata solo dai test automatici.

## Contesto

Il nucleo tiene **un solo** modello locale alla volta (`runtime: Mutex<LocalRuntime>`, un
solo `llama-server`): caricarne un altro scarica il primo. Il secondo `llama-server` del
modello di embedding ([ADR 0017](0017-rag-locale.md)) mostra che più processi convivono
senza problemi, ma è un caso a parte, sulla CPU.

Con 12 GB di VRAM (il PC su cui è sviluppata l'app) ci stanno, per esempio, un 7B in Q4
(circa 5 GB con il contesto) e un modello da 1-3B, oppure un modello per il codice accanto a
uno per la chat; chi ha una GPU da 24 GB o 64 GB di RAM ne tiene di più, chi ha 8 GB forse
uno solo. Il limite quindi non può essere un numero fisso uguale per tutti. Si vuole:

1. avere più modelli locali caricati nello stesso momento e scegliere quale usare in chat,
   negli agenti, nell'API locale;
2. far **cooperare** i modelli: un agente "coordinatore" con un modello grande (o cloud)
   che affida pezzi di lavoro ad aiutanti con modelli più piccoli e veloci, anche in
   parallelo.

Oggi "il modello locale" compare in pochi punti: `ModelRef::Local` (chat, agenti, API
compatibile OpenAI con `local`), lo stato del motore (`RuntimeStatus.loaded`), lo
scaricamento per inattività, il completamento in linea e le pianificazioni
([ADR 0028](0028-agenti-pianificati.md)).

## Decisione

### Più motori, uno per modello

- Il motore unico diventa un **insieme di motori**, uno per file GGUF caricato, ognuno con
  il suo `llama-server`, la sua porta e la sua chiave casuale. Lo stesso file non si carica
  due volte.
- Nuova impostazione **"Più modelli locali insieme"** (Modelli → Motore), **spenta** di
  base: caricare sostituisce, come oggi. Chi non la accende non vede differenze.
- Accesa, il limite è **la memoria di quel PC**, non un numero: **"Carica"** aggiunge il
  modello accanto agli altri **se entra**. Il controllo è quello di oggi (stima contro
  memoria libera misurata), che vede già la memoria presa dagli altri modelli, più un
  **margine** che si adatta all'hardware, perché con più modelli il sistema e le altre
  app non devono restare a secco:
  - VRAM: dopo il caricamento deve restare libero almeno il 5% della VRAM totale, e non
    meno di 512 MB (circa 600 MB su una scheda da 12 GB, 1,2 GB su una da 24 GB);
  - RAM: deve restare libero almeno il 10% della RAM totale, e non meno di 2 GB (circa
    1,6 GB → 2 GB su 16 GB, 6,4 GB su 64 GB).
  Il margine vale solo quando si carica **accanto** ad altri modelli: per il primo, o
  sostituendo, il controllo resta quello di oggi.
- Chi vuole può fissare un tetto in più, **"Al massimo"** (vuoto di base = nessun tetto, decide
  la memoria), per esempio per tenere la VRAM libera per un gioco.
- Se un modello non entra, l'errore dice quanto manca e l'interfaccia offre **"Scarica gli
  altri e carica"**; niente si scarica da solo.
- La sezione dei modelli caricati dice **quanto posto resta** (VRAM e RAM libere meno il
  margine), così si capisce prima di provare se un altro modello ci sta.
- Ogni modello ha il suo **sconfinamento in RAM**, misurato sul suo processo come oggi, e il
  suo **scaricamento per inattività** (stesso tempo per tutti, contato per modello).
- Il modello di embedding resta a parte e non conta nel numero.

### Quale modello: il principale e quelli scelti

- Uno dei modelli caricati è il **principale**: l'ultimo caricato a mano dall'utente, o
  quello scelto con "Rendi principale". Se lo si scarica, diventa principale quello caricato
  da più tempo tra i rimasti. I modelli caricati da un agente o da una pianificazione non
  diventano mai principali da soli.
- `ModelRef::Local` riceve un campo facoltativo con il percorso del file:
  `{"kind": "local"}` (come oggi, e come tutto ciò che è già salvato) vuol dire **il
  principale**; `{"kind": "local", "path": "C:\\modelli\\qwen.gguf"}` vuol dire **quel
  modello**. Niente da migrare nell'archivio.
- **Chat:** il selettore del modello elenca ogni modello locale caricato per nome, più "Il
  principale". Una conversazione che ha scelto un modello preciso e lo trova scaricato dà
  l'errore "non è caricato" con il pulsante per caricarlo, come oggi quando non c'è nessun
  modello.
- **Agenti:** il modulo permette di scegliere "il principale" (come oggi) o **un file GGUF
  preciso**. Con un modello preciso, all'avvio della sessione il nucleo lo **carica da solo
  se non c'è**, purché entri accanto agli altri e non si superi il numero massimo; se non
  entra, la sessione si ferma con l'errore che dice quale modello scaricare. Questo vale per
  le deleghe ed è ciò che rende possibile la cooperazione. Il modello caricato così resta
  caricato a fine sessione e se ne va con lo scaricamento per inattività.
- **API compatibile OpenAI:** `local` è il principale; `local/<nome>` sceglie il modello
  caricato con quel nome di file (senza `.gguf`). `GET /v1/models` elenca ogni modello
  caricato come `local/<nome>`. Un nome che non corrisponde a nessun modello caricato dà un
  errore chiaro invece di usare il principale di nascosto.
- **Completamento in linea (VS Code):** usa il principale, come oggi.
- **Pianificazioni** ([ADR 0028](0028-agenti-pianificati.md)): se il modello scelto non c'è
  ma entra accanto agli altri, si carica accanto, senza conflitto. Solo se non entra
  valgono "Aspetta" e "Sostituiscilo"; "Sostituiscilo" scarica il modello inattivo da più
  tempo, mai il principale se ce n'è un altro, e mai uno al lavoro. A lavoro finito si
  scarica solo il modello caricato dalla pianificazione, con le regole di oggi.

### Stato e interfaccia

- `RuntimeStatus` riceve l'elenco `models` (per ogni modello: dati di caricamento, RAM e
  VRAM del processo, sconfinamento, principale sì/no), `multiModel` e lo spazio rimasto
  (`roomVramBytes`, `roomRamBytes`). `loaded` resta e indica il principale, così
  l'estensione VS Code già installata continua a funzionare; memoria del processo e
  sconfinamento in cima sono la somma di tutti i modelli.
- **Modelli:** la sezione "Modelli caricati" mostra una riga per ciascuno, con memoria,
  "Rendi principale" e "Scarica"; la scheda di un modello che non entra offre "Scarica gli
  altri e carica". La barra di stato mostra il principale e "+N altri". L'impostazione sta in
  Modelli → Motore.
- **Confronto tra modelli:** le colonne possono usare modelli locali diversi; solo due
  colonne sullo stesso modello si danno ancora il turno.
- API locale: `POST /api/local/load` riceve `{"path", "replace"}` (con `replace` scarica
  prima gli altri; senza, il modello va accanto se l'impostazione è accesa),
  `POST /api/local/unload` riceve `{"path"}` facoltativo (senza corpo: tutti, come oggi),
  nuovo `POST /api/local/primary` con `{"path"}`.

### Cooperazione

- La base c'è già: le **deleghe** ([ADR 0018](0018-deleghe-tra-agenti.md)). Con un modello
  per agente, il coordinatore e gli aiutanti lavorano su `llama-server` diversi (o sul cloud).
- **Deleghe in parallelo:** se in un solo turno il modello chiede più `delegate_task`, le
  sessioni figlie partono **insieme** invece che una dopo l'altra, e i resoconti tornano
  tutti insieme, nell'ordine delle chiamate. Anche qui il limite non è un numero legato
  all'hardware: un aiutante che deve caricare il suo modello passa dal controllo della
  memoria descritto sopra, e se non c'è posto si ferma con l'errore; più aiutanti sullo
  stesso modello si mettono in coda sul suo `llama-server` senza occupare altra memoria.
  Resta solo un **tetto di sicurezza** di 8 deleghe per turno, contro un modello che si
  incastra a ripetere la stessa chiamata: quelle in più ricevono un errore che lo dice. Le conferme, se servono, si
  chiedono prima di far partire il gruppo, una per chiamata. Due aiutanti sullo stesso
  modello locale si alternano sul suo `llama-server`: funziona, ma senza guadagno di tempo.
  Gli altri strumenti di quel turno restano in sequenza, come oggi.
- La descrizione dello strumento dice al coordinatore che può affidare più pezzi
  indipendenti nello stesso turno, e quale modello usa ogni aiutante, così può scegliere.
- Nessun nuovo "tipo" di agente: il coordinatore è un agente normale con degli aiutanti.
  Nella guida si aggiunge un esempio (coordinatore con Qwen2.5 7B, due aiutanti con un
  modello da 1-3B).

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Un solo `llama-server` con più modelli (modalità "router" di llama.cpp) | Dipende da una funzione recente di llama.cpp e dalla sua gestione della memoria; con un processo per modello lo sconfinamento e la memoria si misurano per processo come oggi, e un modello che si blocca non porta giù gli altri |
| Più di un modello caricato già di base | Cambierebbe il comportamento di "Carica" per chi c'è abituato e riempirebbe la VRAM senza che l'utente l'abbia deciso |
| Un numero massimo di modelli uguale per tutti (la prima versione di questo ADR: 3) | Chi ha più memoria ne terrebbe di più, chi ne ha meno ne entrerebbero meno comunque: il limite vero è la memoria, misurata su quel PC, con un margine proporzionale |
| Scaricare da solo il modello meno usato quando non c'è posto | Sorprende: si perde un modello che l'utente aveva caricato. Meglio dirlo e offrire il pulsante |
| `ModelRef::Local` con il nome del modello invece del percorso | Due file diversi possono avere lo stesso nome; il percorso è quello che il nucleo usa già per i preset |
| Un modo "squadra" o "orchestratore" a parte, con regole sue | Le deleghe fanno già questo lavoro con sessioni, permessi e registro che l'utente conosce; serve solo farle partire in parallelo |
| Deleghe in parallelo senza alcun tetto | Un modello che si incastra potrebbe aprire decine di sessioni in un turno; il tetto di 8 è una protezione contro gli errori, non un limite di memoria |

## Conseguenze

- Il motore diventa un elenco di motori (`local::pool::Engine`), ognuno con il suo
  lucchetto, tenuto solo per caricare e scaricare: un caricamento in corso non blocca le
  risposte degli altri modelli. Ogni motore ha il suo registro (`llama-server.log`,
  `llama-server-2.log`…).
- Per le deleghe in parallelo il segnale di arresto sveglia tutte le sessioni figlie in
  attesa, non solo la prima.
- I punti che oggi leggono "il modello caricato" (chat, sessioni, diagnostica, scaricamento
  per inattività, pianificazioni, `/v1/models`, l'estensione) passano dal principale o dal
  modello scelto: va provato che con l'impostazione a 1 tutto si comporti come prima.
- Più modelli in VRAM vuol dire meno contesto per ciascuno: la guida lo dice, e la stima
  prima del caricamento resta l'unica difesa (più lo sconfinamento misurato dopo).
- Alla chiusura dell'app si scaricano tutti.
- Rimandato: modello per il completamento in linea scelto in VS Code, uno strumento
  "consulta un altro modello" in chat, deleghe in parallelo oltre il primo livello
  (gli aiutanti degli aiutanti restano in sequenza).
