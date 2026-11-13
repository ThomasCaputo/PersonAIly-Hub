# 0008 — Agenti: sessioni nel nucleo, terminale con permessi, registro delle azioni

**Stato:** accettata — provata nell'app con un modello locale vero, Qwen 2.5 7B: compito
in più passi su una cartella (scrittura di file e comandi), pausa, arresto durante una
richiesta di conferma e ripresa con un messaggio. Il terminale è provato solo su Windows.

## Contesto

La Tappa 4 chiede agenti che lavorano da soli su un compito (AG-1 … AG-4), comandi nel
terminale soggetti a permessi (AG-5), un registro delle azioni (SI-6) e più agenti
insieme (AG-6). Le regole degli strumenti dell'[ADR 0007](0007-strumenti-e-permessi.md)
devono valere anche qui, senza una seconda copia dei controlli.

## Decisione

**Un agente è una definizione salvata** (tabella `agents`, in JSON): nome, descrizione,
istruzioni, modello (locale caricato o cloud), cartelle di lavoro, strumenti (file,
terminale, web), livello di permesso e passi massimi. Si crea, modifica, duplica ed
elimina dalla pagina Agenti.

**Una sessione è un agente al lavoro su un compito** (tabella `agent_sessions`). Ha una
conversazione propria, quindi messaggi, chiamate, risultati, costi e rilettura dopo il
riavvio sono quelli della chat; quelle conversazioni non compaiono nell'elenco della chat.
Alla partenza la sessione copia la definizione dell'agente: modificarlo dopo non cambia
una sessione già avviata.

**Il ciclo è lo stesso della chat** (`Core::run_rounds`), con tre differenze:

- va avanti da solo fino alla risposta senza strumenti, o fino ai passi massimi (50 di
  base, al massimo 500), poi si ferma e aspetta che l'utente risponda;
- gira in sottofondo: l'interfaccia ne legge lo stato ogni mezzo secondo
  (`Core::session_live`: testo in arrivo, richiesta di conferma, contatore dei messaggi
  salvati) e rilegge l'archivio solo quando cambia qualcosa;
- si controlla con *pausa* (si ferma prima del passo successivo, cioè prima di chiedere
  al modello o di eseguire lo strumento seguente), *riprendi* e *ferma* (interrompe la
  risposta in corso e il comando in esecuzione; le chiamate rimaste ricevono "Interrotto").

**Stati:** al lavoro, in pausa, aspetta conferma, finito, fermato (anche per limite di
passi o risposta troncata, con una nota), errore, interrotto (l'app è stata chiusa: lo
si scopre al riavvio). Da una sessione ferma si riparte scrivendo all'agente.

**Più sessioni insieme (AG-6).** Ogni sessione ha i propri comandi di controllo e le
proprie conferme: la chiave di una conferma è `s<sessione>:<chiamata>` (in chat
`c<conversazione>:<chiamata>`), così due cicli in parallelo non si rubano le risposte.
Con i servizi cloud le sessioni lavorano davvero insieme; con il modello locale
`llama-server` ha un solo flusso (`--parallel 1`, ADR 0003) e le richieste fanno la coda.

**Cartelle.** Le cartelle di lavoro dell'agente sono le *sue* cartelle concesse: scelte
dall'utente nell'editor dell'agente con la finestra di sistema, controllate con le stesse
regole dell'ADR 0007 (percorsi risolti, `..` e giunzioni compresi).

**Terminale (`run_command`, classe "esecuzione").** Solo per gli agenti: in chat non si
offre. Su Windows il comando gira in PowerShell (`-NoProfile -NonInteractive`, testo
passato con `-EncodedCommand` perché virgolette e accenti arrivino intatti, uscita in
UTF-8), senza finestra, senza input, dentro un *job* di Windows: a fine comando, a tempo
scaduto (120 s di base, al massimo 600) o all'arresto, il sistema termina anche i processi
che il comando aveva avviato. Al modello tornano codice di uscita, uscita ed errori
(inizio e fine se troppo lunghi; gli errori di PowerShell, scritti in CLIXML, vengono
riportati a testo). Un codice diverso da 0 torna come errore.

**Isolamento: bastano i permessi, niente sandbox** (questione aperta n. 2
dell'architettura). Il comando *parte* in una cartella concessa (anche `cwd` viene
controllato), ma un programma può comunque toccare tutto ciò che l'utente può toccare.
Per questo:

| Livello | Comandi |
|---|---|
| Chiedi sempre | chiedono conferma, mostrando il comando |
| Consenti modifiche ai file | chiedono conferma |
| Consenti tutto | partono senza chiedere — l'editor dell'agente avvisa che possono agire anche fuori dalle cartelle |

**Registro delle azioni (SI-6).** Ogni chiamata a uno strumento, in chat e negli agenti,
lascia una riga in `actions`: quando, dove (chat o agente, con il nome di allora), quale
strumento, che cosa (la stessa descrizione della richiesta di conferma), come è stata
decisa (consentita dai permessi, confermata, negata, rifiutata dall'app, interrotta),
esito e inizio del risultato. Le righe restano anche se la conversazione o la sessione
vengono eliminate: è un registro, non una cronologia. Si consulta nella scheda "Registro
azioni" della pagina Agenti.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Ciclo degli agenti separato da quello della chat | Due copie dei controlli di sicurezza; i miglioramenti andrebbero fatti due volte |
| Sessioni seguite con un canale aperto dall'interfaccia | Il canale vive quanto la chiamata: cambiando pagina si perderebbe la sessione. Lo stato nel nucleo lo legge chiunque, anche la futura API locale |
| Sandbox per i comandi (utente limitato, AppContainer, contenitori) | Complicata su Windows, rompe gli strumenti di sviluppo dell'utente (git, compilatori, pacchetti); i permessi con conferma coprono il caso d'uso. Riconsiderare se servissero agenti senza sorveglianza |
| `cmd.exe` invece di PowerShell | I modelli conoscono meglio PowerShell su Windows, che gestisce anche i caratteri non ASCII senza pagine di codice |
| Terminale anche in chat | Non richiesto; in chat un comando a sorpresa sarebbe più rischioso. Si può aggiungere più avanti con lo stesso strumento |
| Registro dentro i messaggi (come in Tappa 3) | Sparisce eliminando la conversazione e non si consulta tutto insieme |

## Conseguenze

- Un agente con "Consenti tutto" e il terminale può fare danni fuori dalle sue cartelle:
  l'interfaccia lo dice, ma resta una scelta dell'utente.
- La pausa agisce tra un passo e l'altro: una risposta lunga del modello o un comando in
  corso finiscono prima che la pausa abbia effetto ("Ferma" invece li interrompe subito).
- I processi avviati da un comando muoiono con lui: un agente non può lasciare acceso un
  server di sviluppo tra un comando e l'altro.
- Alla chiusura dell'app le sessioni al lavoro si fermano e al riavvio risultano
  "interrotte"; si riprendono scrivendo all'agente.
- Il registro cresce senza limite; se diventasse grande servirà una pulizia per data.
- Su Linux e macOS il terminale usa `sh -c`, senza job: i processi figli di un comando
  interrotto potrebbero restare in vita. Da rivedere con il port (GE-2).
- MCP (AG-7) entra come altra fonte di strumenti dentro lo stesso ciclo e con gli stessi
  permessi: vedi [ADR 0009](0009-mcp.md).
