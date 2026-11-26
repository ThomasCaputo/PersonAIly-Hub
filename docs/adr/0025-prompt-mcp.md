# 0025 — Prompt MCP come comandi `/`

**Stato:** accettata e realizzata (25-11-2026). Provata nell'app con
`@modelcontextprotocol/server-everything`: elenco, filtro, scelta da tastiera, argomenti
obbligatori e facoltativi.

## Contesto

Oltre a strumenti e risorse, un server MCP può offrire dei **prompt**: testi pronti, spesso con
argomenti, pensati per essere scelti dall'utente (non dal modello). Molti client li mostrano
come comandi `/`. Con i server MCP in chat ([ADR 0022](0022-mcp-in-chat.md)) mancava il modo
di usarli.

## Decisione

- Il client legge la capacità `prompts` del server e impara `prompts/list` (con paginazione) e
  `prompts/get`.
- In chat, scrivendo `/` all'inizio del messaggio si apre un menu con i prompt dei server MCP
  della conversazione. Il testo dopo `/` filtra per nome del prompt o del server. Frecce e
  Invio (o Tab) scelgono, Esc chiude.
- Un prompt con argomenti apre un piccolo modulo (gli obbligatori sono segnati con `*`).
- **Il testo del prompt prende il posto del messaggio nella casella**: l'utente lo vede, può
  cambiarlo e poi lo invia come un messaggio qualunque. I messaggi del prompt si uniscono in
  ordine; quelli dell'assistente sono segnati con `[assistente]`, le risorse incluse portano il
  loro testo, immagini e audio sono solo nominati.
- I prompt si leggono dagli stessi server accesi per la conversazione (stessa cartella di
  lavoro, quindi stessi processi): aprire il menu li avvia se non lo sono ancora.
- Un server che non sa elencare i prompt viene saltato e annotato nel registro diagnostico;
  gli altri restano nel menu.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Inviare subito il prompt scelto | L'utente non vedrebbe che cosa manda: il testo viene da un programma esterno |
| Inserire i messaggi del prompt come più messaggi della conversazione, con i loro ruoli | Più fedele al protocollo, ma un messaggio "dell'assistente" scritto da un server confonde la cronologia e i rami |
| Prompt anche negli agenti | Un agente lavora da solo su un compito scritto dall'utente: i prompt sono fatti per essere scelti a mano |

## Conseguenze

- Il primo `/` di una conversazione può aspettare l'avvio dei server (con `npx` la prima volta
  anche il download).
- Gli argomenti con completamento si scrivono a mano: i suggerimenti non ci sono ancora.
  *Aggiornamento del 26-11-2026:* ora ci sono, vedi sotto.

## Suggerimenti per gli argomenti (26-11-2026)

- Se il server dichiara la capacità `completions`, mentre si scrive un argomento il client manda
  `completion/complete` con il prompt (`ref/prompt`), l'argomento, ciò che è già scritto e, come
  `context`, gli altri argomenti compilati. La richiesta parte 200 ms dopo l'ultimo tasto e
  all'arrivo nel campo; conta solo la risposta all'ultima richiesta.
- I valori compaiono in un elenco sotto la casella, scelto con frecce e Invio o con un clic.
  Invio con un valore evidenziato sceglie solo quel valore; senza, inserisce il prompt come
  prima. Esc chiude prima l'elenco, poi il modulo. Si è scartato il `<datalist>` del browser
  perché il suo comportamento con Invio non si può controllare né provare.
- Sono un aiuto, non un passaggio obbligato: al massimo 50 valori di testo, senza doppioni né
  vuoti, ciascuno al massimo di 500 caratteri; un server che non risponde entro 5 secondi, o
  che è occupato da uno strumento per più di 5 secondi, viene saltato senza errori.
