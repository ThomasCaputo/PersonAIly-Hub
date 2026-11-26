# 0027 — Modelli di riserva

**Stato:** accettata e realizzata (26-11-2026). Provata dai test automatici con servizi finti
(errore 503/429/500, errore a metà risposta); manca una prova con un servizio vero (per esempio
Google AI Studio gratuito, che risponde spesso "modello occupato").

## Contesto

I piani gratuiti dei servizi cloud (Google AI Studio, OpenRouter, Groq…) rispondono spesso con
"modello occupato" (503) o "troppe richieste" (429), anche a metà di un lavoro. Finora la
risposta si fermava con l'errore e l'utente doveva cambiare modello e riscrivere.

## Decisione

- **Una coda per ogni chat e per ogni agente**, perché in conversazioni diverse si vogliono
  modelli diversi. La coda di Impostazioni → Modelli di riserva (impostazione
  `fallback_models`) è quella **predefinita**: una chat o un agente la segue finché non gliene si
  dà una propria (`fallbackModels` negli strumenti della conversazione, o nella definizione
  dell'agente; assente = quella delle Impostazioni, vuota = nessuna riserva). Scegliendo "una
  propria", la coda parte come copia di quella delle Impostazioni, da confermare o cambiare. In
  chat si sceglie dal menu "Riserva" sopra i messaggi, per un agente nel suo modulo (solo se il
  suo modello è cloud). Ogni coda ha al massimo 10 modelli cloud, senza doppioni; tutto finisce
  nei backup con il resto dei dati. Gli strumenti predefiniti delle nuove conversazioni non
  portano una coda: le nuove chat seguono quella delle Impostazioni.
- **Quando scatta:** a ogni richiesta a un modello cloud in chat o in un agente. Se il servizio
  risponde con un errore di qualunque tipo (occupato, limite di richieste, credito finito,
  chiave rifiutata, richiesta troppo lunga per quel modello…), si prova il primo modello della
  coda che non sia quello appena fallito, poi il successivo. Modelli irraggiungibili (provider
  eliminato, chiave mancante) si saltano. Finita la coda, resta l'errore dell'ultimo provato.
  Il limite di spesa si controlla prima di ogni tentativo.
- Vale anche per le conversazioni dell'API locale usate dall'estensione VS Code
  (`/api/conversations/…/messages`), che ricevono l'evento `modelSwitch`.
- **Non scatta** se l'utente ha interrotto la risposta, con il modello locale e nel confronto
  tra modelli (conversazioni temporanee): lì ogni colonna deve rispondere con il suo modello.
- **Testo a metà:** se il primo modello aveva già scritto qualcosa prima dell'errore, quel testo
  non si salva; l'interfaccia lo scarta con l'evento `saved` e la risposta riparte intera dal
  modello di riserva.
- **Per tutta la risposta** si resta sul modello che ha funzionato (anche nei giri con gli
  strumenti e nei passi di un agente): ritentare ogni volta il primo costerebbe una richiesta
  fallita per passo. Il messaggio successivo riparte dal modello scelto.
- **Chi ha risposto:** ogni messaggio salva già il nome del modello che l'ha scritto, mostrato
  in chat e negli agenti. In più l'evento `ModelSwitch` (`modelSwitch` nell'API locale) dice da
  quale modello a quale si è passati e perché: la chat lo mostra sopra la risposta, il registro
  diagnostico lo annota.
- I parametri di generazione restano quelli del modello scelto; quelli che l'altro provider non
  accetta non si inviano (come sempre).

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Ripartire solo con certi errori (429, 5xx) | L'utente chiede di passare oltre "per qualunque problema"; con altri errori (chiave scaduta, contesto troppo lungo) un altro modello spesso risponde lo stesso |
| Una coda unica per tutta l'app (la prima versione) | In un'altra conversazione si può volere tutt'altro: l'utente l'ha chiesta per singola chat, con quella globale come punto di partenza |
| Ritentare lo stesso modello dopo un'attesa | Con i piani gratuiti l'attesa è spesso di minuti: meglio passare subito a un altro modello |
| Riserva anche nell'API compatibile OpenAI (`/v1/chat/completions`) | Chi la usa sceglie un modello preciso e riceve il nome del modello nella risposta: cambiarlo di nascosto lo confonderebbe |

## Conseguenze

- Una risposta può costare richieste a più servizi; il costo stimato è quello del modello che
  ha risposto.
- Nelle sessioni degli agenti il cambio si vede dal nome del modello sui messaggi, non da un
  avviso a parte.
