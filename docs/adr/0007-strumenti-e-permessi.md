# 0007 — Strumenti e permessi: il nucleo esegue, l'utente conferma

**Stato:** accettata — provata nell'app con un servizio finto (permessi, confini delle
cartelle, interruzione) e con un modello locale vero, Qwen 2.5 7B (lettura di file,
ricerca web, lettura di pagine). Il formato Anthropic è provato solo dai test automatici.

## Contesto

Nella Tappa 3 il modello deve poter leggere e scrivere file, cercare sul web e leggere
pagine (CH-6, CH-7), ricevere allegati (CH-5), sempre sotto il controllo dell'utente
(SI-1, SI-2, SI-3, SI-5). Le stesse regole dovranno valere per gli agenti (Tappa 4).

## Decisione

**Il ciclo vive nel nucleo** (`Core::send_message`): modello → richiesta di strumenti →
controllo → eventuale conferma → esecuzione → risultato al modello, fino alla risposta
finale (al massimo 25 giri). L'interfaccia mostra soltanto e raccoglie le conferme.

**Strumenti offerti** (`crates/core/src/tools/`):

| Strumento | Classe | Note |
|---|---|---|
| `list_folder`, `read_file` | lettura | `read_file` estrae anche il testo dei PDF |
| `write_file`, `edit_file` | modifica | `edit_file` sostituisce un testo che compare una sola volta |
| `web_search` | lettura | DuckDuckGo, versione HTML: nessuna chiave, nessun account |
| `read_web_page` | lettura | solo indirizzi pubblici |

Gli strumenti si offrono solo se servono: quelli sui file solo se c'è almeno una
cartella concessa, quelli web solo se l'utente li accende nella conversazione (le parole
cercate escono dal computer, quindi sono spenti di base).

**Permessi** — tre livelli per conversazione (SI-2), con valori predefiniti nelle
Impostazioni:

| Livello | Lettura | Modifica | Esecuzione (Tappa 4) |
|---|---|---|---|
| Chiedi sempre (predefinito) | consentita | chiede | chiede |
| Consenti modifiche ai file | consentita | consentita | chiede |
| Consenti tutto nelle cartelle | consentita | consentita | consentita |

Nella richiesta di conferma l'utente sceglie *Consenti*, *Consenti sempre in questa
conversazione* (alza il livello e lo salva) o *Nega*. Un rifiuto torna al modello come
errore con l'indicazione di non riprovare.

**Confini che nessun livello allarga:**

- **Cartelle concesse (SI-3).** Ogni percorso viene risolto (via `..`, collegamenti e
  giunzioni) e confrontato con le cartelle concesse risolte. Una chiamata fuori confine
  fallisce subito, senza nemmeno chiedere all'utente.
- **Solo internet pubblico.** `read_web_page` rifiuta questo computer, la rete locale e
  gli indirizzi riservati, controllando ogni tappa dei rinvii.
- **I dati non comandano (SI-5).** Nessuno strumento può cambiare permessi o cartelle;
  la conferma arriva solo da un clic nell'interfaccia (`answer_tool_request`). Il prompt
  di sistema avvisa il modello che file e pagine sono materiale, non ordini.
- **Strumenti non offerti non si eseguono**, anche se il modello ne conosce il nome; una
  chiamata con parametri illeggibili fallisce senza eseguire nulla.

**Archivio.** Chiamate e risultati sono messaggi della conversazione (colonna
`tool_data`), quindi si rivedono dopo il riavvio e valgono come primo registro delle
azioni (SI-6). Se una risposta viene interrotta, ogni chiamata rimasta riceve comunque un
risultato ("Interrotto"), perché i servizi rifiutano conversazioni con chiamate senza
risposta; lo stesso vale, alla rilettura, per quelle lasciate a metà da una chiusura
dell'app.

**Formati dei provider.**

- *Compatibile OpenAI e `llama-server`:* `tools` / `tool_calls` / messaggi `tool`. I
  parametri arrivano a pezzi e vengono ricomposti; i modelli locali li usano tramite il
  modello di chat del file GGUF.
- *Anthropic:* blocchi `tool_use` / `tool_result`, con i parametri che arrivano mentre il
  modello li scrive (`eager_input_streaming`). Mentre gli strumenti di una risposta sono
  in corso, quella risposta torna al servizio **identica, ragionamento compreso**, come
  l'API richiede; non viene mai modificato un turno già inviato. Le risposte concluse
  tornano come testo e chiamate.

**Allegati (CH-5).** Testo e PDF diventano testo dentro il messaggio (funzionano con ogni
modello); le immagini viaggiano come immagini e oggi solo verso i modelli cloud: con un
modello locale l'invio viene rifiutato con una spiegazione. Gli allegati sono salvati con
il messaggio, così la conversazione resta completa.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Ciclo degli strumenti nell'interfaccia | L'estensione VS Code e gli agenti dovrebbero rifarlo; i controlli di sicurezza devono stare in un solo posto |
| Ricerca nativa dei provider (es. web search di Anthropic) | Non esiste per i modelli locali, costa a parte e cambia da servizio a servizio |
| Brave, Tavily, SearXNG | Chiedono una chiave o un server da installare; restano possibili come alternative future |
| Confronto dei percorsi come testo | Un `..` o una giunzione porterebbero fuori dalla cartella |
| Seguire i rinvii automaticamente | Un sito pubblico potrebbe rinviare a un indirizzo della rete locale |

## Conseguenze

- La ricerca dipende dalla pagina HTML di DuckDuckGo: se cambia, l'estrazione dei
  risultati va aggiornata (`tools/web.rs`, con il suo test).
- I modelli locali piccoli a volte rispondono a memoria invece di usare gli strumenti
  (visto con Qwen 7B sulla ricerca web): è un limite del modello, non dell'app.
- Le immagini con i modelli locali richiedono il supporto multimodale di llama.cpp
  (file `mmproj`), non ancora gestito.
- Il formato Anthropic con strumenti va provato con una chiave vera, come il resto del
  provider (ADR 0005).
