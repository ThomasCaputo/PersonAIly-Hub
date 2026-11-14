# 0005 — Provider cloud: due famiglie di API, chiavi nel portachiavi di sistema

**Stato:** accettata — l'API compatibile OpenAI è provata contro `llama-server` e, il
13-11-2026, contro un servizio esterno vero con una chiave vera (Google AI Studio,
indirizzo `https://generativelanguage.googleapis.com/v1beta/openai/`); quella di
Anthropic solo contro un server finto, non ancora con una chiave vera

Note dalla prova con Google AI Studio:

- alcuni modelli Gemma su quel servizio rispondono a volte `500 Internal error`: è un
  errore del servizio, non della richiesta;
- con temperatura 0 la stessa domanda può dare risposte diverse: i servizi cloud non
  garantiscono risposte ripetibili, nemmeno con il `seed`.

## Contesto

L'app deve usare modelli cloud di servizi diversi con la chiave dell'utente, permettere di
cambiare modello a metà conversazione e mostrare per ogni modello solo i parametri che
esso accetta davvero.

## Decisione

**Due famiglie di API**, ciascuna con il proprio modulo in `crates/core/src/providers/`:

| Famiglia | Servizi | Indirizzo |
|---|---|---|
| Anthropic | modelli Claude | `https://api.anthropic.com` |
| Compatibile OpenAI | OpenAI, OpenRouter, Groq, Mistral e chiunque esponga `/chat/completions` | quello del servizio, compreso `/v1` |

Un **provider** è una voce configurata dall'utente: nome, famiglia, indirizzo, elenco dei
modelli scaricato dal servizio. I provider stanno nell'archivio dell'app.

**Le chiavi API** stanno nel portachiavi del sistema operativo (su Windows il Gestore
credenziali, servizio `progetto-ai`). Non vengono scritte in file dell'app e non tornano
mai all'interfaccia: questa sa solo se una chiave esiste.

**Niente chiavi in chiaro sulla rete:** l'indirizzo di un provider deve essere `https://`;
`http://` è ammesso solo verso lo stesso computer (`127.0.0.1`, `localhost`).

**La conversazione è indipendente dal modello:** in archivio ci sono solo ruolo e testo
dei messaggi (più il nome del modello che ha risposto), quindi ogni messaggio può andare a
un modello diverso.

## Parametri per famiglia

L'interfaccia mostra solo i parametri della colonna giusta; gli altri non vengono inviati.

| Parametro | Locale | Compatibile OpenAI | Anthropic |
|---|---|---|---|
| Temperatura, top-p | sì | sì (inviati solo se diversi da 1) | **no** |
| Top-k, min-p, penalità di ripetizione | sì | no | no |
| Seed | sì | sì | no |
| Token massimi, sequenze di stop | sì | sì | sì |

- **Anthropic non accetta più i parametri di campionamento**: i modelli Claude attuali li
  rifiutano, e dalla versione `2026-10-21` dell'API li rifiuta ogni modello.
- **Compatibile OpenAI**: alcuni modelli rifiutano valori diversi da quello predefinito,
  per questo temperatura e top-p partono da 1 e vengono inviati solo se l'utente li cambia.
  Verso `api.openai.com` il limite di token si chiama `max_completion_tokens`.

## Dettagli dell'API Anthropic

- Intestazioni: `x-api-key`, `anthropic-version: 2026-10-21`.
- `max_tokens` è obbligatorio: senza indicazioni dell'utente si usa 64 000, o il massimo
  del modello se è più basso (lo dice l'elenco dei modelli).
- Si chiede il **riassunto del ragionamento** (`thinking: {type: "adaptive", display:
  "summarized"}`): senza, durante il ragionamento l'utente vedrebbe solo una lunga pausa.
- Per i modelli che lo prevedono si attiva il **ripiego automatico**
  (`fallbacks: "default"` con l'intestazione `anthropic-beta:
  server-side-fallback-2026-07-01`): se il modello rifiuta una richiesta per i suoi filtri
  di sicurezza, il servizio la ripete su un altro modello Claude, che viene pagato alla
  sua tariffa.
- Se il servizio risponde 400 (richiesta non valida), l'app **riprova una volta senza
  queste due aggiunte**: è il caso dei modelli più vecchi che non le conoscono.
- Un rifiuto (`stop_reason: "refusal"`) o una risposta troncata (`max_tokens`) non sono
  errori: compaiono come avviso sotto la risposta.

## Costo stimato

Per ogni risposta l'archivio conserva i token letti, quelli generati e, se il prezzo del
modello è noto, il costo in dollari. Da lì l'interfaccia ricava il costo della
conversazione e il contesto occupato, che quindi sopravvivono al riavvio.

| Famiglia | Da dove viene il prezzo |
|---|---|
| Compatibile OpenAI | Dal campo `pricing` dell'elenco dei modelli, se il servizio lo pubblica (OpenRouter lo fa) |
| Anthropic | Da una tabella dei prezzi di listino dentro `providers/anthropic.rs`: l'API non li comunica |

- È una **stima**: non considera sconti, cache dei prompt né tariffe speciali. La cifra
  addebitata dal servizio può essere diversa, e l'interfaccia lo dice.
- **Senza prezzo o senza conteggio dei token non si mostra nessun costo**: meglio niente
  che una cifra inventata.
- Il costo è calcolato al momento della risposta e non viene ricalcolato se i listini cambiano.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Solo l'API compatibile OpenAI, anche per Claude | Si perderebbero ragionamento, gestione dei rifiuti e, più avanti, gli strumenti nativi |
| Chiavi in un file di configurazione | Leggibili da qualunque programma e facili da pubblicare per errore |
| Un SDK ufficiale | Per Rust non esiste: si parla HTTP direttamente |

## Conseguenze

- Aggiungere un servizio compatibile OpenAI non richiede codice: basta l'indirizzo.
- Le particolarità di Anthropic vivono in un solo file (`providers/anthropic.rs`), da
  rivedere quando cambia la versione dell'API.
- La tabella dei prezzi di Anthropic va aggiornata a mano quando escono nuovi modelli o
  cambiano i listini; un modello che non vi compare resta senza stima.
- La memoria temporanea dei prompt del servizio (prompt caching), che riduce i costi
  nelle conversazioni lunghe, non è ancora usata.
