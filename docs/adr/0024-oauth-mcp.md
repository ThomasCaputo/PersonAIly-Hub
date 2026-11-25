# 0024 — Accesso OAuth ai server MCP remoti

**Stato:** accettata e realizzata (25-11-2026). Scoperta provata su Notion e Linear (entrambi con
registrazione dinamica); accesso completo provato nell'app con un servizio finto. Manca una prova
con un account vero.

## Contesto

Dalla 1.0.0 un server MCP via HTTP si collega con un token messo a mano nelle intestazioni
(ADR 0021). Molti server remoti diffusi (Notion, Linear, Sentry…) però non danno token da
copiare: seguono la parte del protocollo MCP sull'autorizzazione, cioè OAuth 2.1 con accesso
dal browser. Senza, quei connettori non si possono usare.

## Decisione

Una nuova scelta nel modulo dei server HTTP: **Accesso dal browser (OAuth)**, con il pulsante
**Accedi**. Il flusso segue la specifica MCP (versione 2025-06-18):

1. **Scoperta.** L'app manda una richiesta al server senza token. Dalla risposta 401 legge
   `WWW-Authenticate` (`resource_metadata`, `scope`); se manca prova gli indirizzi noti
   `/.well-known/oauth-protected-resource`. Dai metadati della risorsa (RFC 9728) prende il
   server di autorizzazione e ne legge i metadati (RFC 8414, poi OpenID Connect). Per i server
   che seguono la specifica del 2025-03-26 il server di autorizzazione è il server stesso, con
   gli indirizzi `/authorize`, `/token`, `/register` se non ne pubblica altri.
2. **Registrazione.** Se il server offre la registrazione dinamica (RFC 7591), l'app si
   registra da sola come client pubblico (`token_endpoint_auth_method: none`), a ogni accesso,
   con l'indirizzo di ritorno di quell'accesso. Altrimenti l'utente indica un **Client ID** (e,
   se serve, un segreto) registrato da lui presso il servizio.
3. **Accesso.** PKCE con `S256`, `state` casuale, parametro `resource` (RFC 8707) con
   l'indirizzo del server MCP. L'app apre la pagina di accesso nel browser di sistema e aspetta
   il ritorno su `http://127.0.0.1:<porta casuale>/callback` (app nativa, RFC 8252), per
   5 minuti al massimo; «Annulla» smette di aspettare.
4. **Token.** Il codice si scambia con i token. Token di accesso, token di rinnovo e segreto
   del client vanno nel **Gestore credenziali**, divisi in pezzi se troppo lunghi per una
   voce sola. Nell'archivio restano solo dati non segreti: indirizzo dei token, client ID,
   scadenza.
5. **Uso.** Ogni volta che il server parte (chat, agenti, Prova) l'app aggiunge
   `Authorization: Bearer …`; se il token è scaduto, o scade entro un minuto, prima lo rinnova.
   Se il rinnovo non riesce, l'errore chiede di accedere di nuovo.

**Controlli.** Tutti gli indirizzi scoperti (metadati, autorizzazione, token, registrazione)
devono essere `https://`, salvo `http://` su questo computer, come gli indirizzi dei server.
Se il server dichiara i metodi PKCE e tra questi non c'è `S256`, l'accesso si rifiuta. Il
`state` deve tornare uguale. La pagina che si apre nel browser è del servizio: l'app mostra a
quale indirizzo, prima di aprirla.

**Esci** cancella i token; eliminare il server cancella anche quelli.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Un crate OAuth completo | Il flusso usato è piccolo e i controlli (indirizzi, PKCE, `resource`) li vogliamo vedere e provare noi, come per il client MCP (ADR 0009) |
| Finestra di accesso dentro l'app (WebView) | L'utente non vede l'indirizzo reale né usa le password salvate nel suo browser; RFC 8252 la sconsiglia |
| Indirizzo di ritorno con schema proprio (`personaily://`) | Va registrato in Windows e non vale per `app-server`; il ritorno su `127.0.0.1` è quello previsto per le app native |
| Un client registrato una volta sola, riusato | Molti servizi accettano solo l'indirizzo di ritorno esatto, porta compresa: una registrazione per accesso costa poco |
| Token nell'archivio SQLite | Sono credenziali: stanno nel Gestore credenziali come chiavi API e intestazioni |

## Conseguenze

- I servizi senza registrazione dinamica (per esempio GitHub) chiedono di registrare un'app
  OAuth propria e di indicarne il Client ID, con indirizzo di ritorno `http://127.0.0.1/callback`.
- L'accesso si fa dall'app con la finestra; `app-server` (VS Code ad app chiusa) usa e rinnova
  i token già ottenuti.
