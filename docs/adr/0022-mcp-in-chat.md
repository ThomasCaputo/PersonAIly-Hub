# 0022 — Server MCP anche in chat

**Stato:** accettata e realizzata (25-11-2026). Provata nell'app con Qwen2.5 7B e
`@modelcontextprotocol/server-memory`: due messaggi usano lo stesso processo del server,
eliminare la conversazione lo chiude. Supera il punto «Solo agenti» dell'
[ADR 0009](0009-mcp.md).

## Contesto

Fino alla 1.0.0 i server MCP si offrivano solo agli agenti, come il terminale. Chi aggiunge
un server (Memory, GitHub, un database) però se lo aspetta anche in chat, e in chat i
permessi e le conferme degli strumenti ci sono già (ADR 0007).

## Decisione

- **Scelta per conversazione.** `ToolSettings` ha un nuovo campo `mcpServers` (gli
  identificatori dei server). Nella barra degli strumenti della chat un menu «Server MCP»
  elenca i server configurati, da spuntare. Vale anche per gli strumenti predefiniti delle
  nuove conversazioni. Nelle sessioni degli agenti il campo non conta: lì valgono i server
  dell'agente.
- **I server restano accesi tra un messaggio e l'altro.** Avviarli a ogni messaggio
  costerebbe secondi (con `npx` anche decine) e perderebbe lo stato del server. Il nucleo
  tiene un insieme di server per conversazione e lo riusa finché:
  - la scelta dei server o la loro configurazione non cambia (allora si riavvia);
  - un server stdio non si è chiuso (allora si riavvia al messaggio successivo);
  - la conversazione resta senza messaggi per **10 minuti** (li chiude la manutenzione del
    nucleo, ogni 30 secondi);
  - non si elimina la conversazione.
  Al massimo **4 conversazioni** tengono server accesi insieme: oltre, si chiudono quelli
  della conversazione usata meno di recente. Ogni conversazione ha i suoi processi, come
  ogni sessione (ADR 0009): niente stato condiviso tra conversazioni.
- **Avvio dopo il messaggio.** I server partono dopo aver salvato il messaggio dell'utente,
  così mentre partono si vede il messaggio e l'attesa della risposta; «Interrompi» ferma
  anche l'avvio. Se un server non parte la risposta fallisce con l'errore del server, come
  quando un servizio cloud non risponde, e il messaggio si può rimandare.
- **Server eliminati.** Un server spuntato e poi eliminato dalle Impostazioni viene
  saltato in silenzio: in chat non c'è, come negli agenti, un editor dove togliere il segno,
  e la barra mostra solo i server che esistono.
- **Conferme più strette che negli agenti.** In chat il livello «Consenti tutto nelle
  cartelle» non copre gli strumenti MCP: il suo nome promette azioni solo nelle cartelle
  concesse, mentre un server MCP agisce dove vuole. Ogni strumento MCP chiede conferma
  finché non si sceglie «Consenti sempre in questa conversazione» per **quello** strumento.
- Il resto è come negli agenti: strumenti in classe "esecuzione", risorse elencabili e
  leggibili, parametri mostrati nella conferma, ogni chiamata nel registro delle azioni.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Avviare i server a ogni messaggio | Secondi di attesa a ogni messaggio e stato del server perso |
| Un solo insieme di server per tutte le conversazioni | Due risposte in parallelo si contenderebbero lo stesso server e il suo stato |
| Server sempre accesi finché l'app resta aperta | Con l'app nell'area di notifica per settimane resterebbero processi inutili |
| «Consenti tutto» che vale anche per MCP, come negli agenti | In chat il livello parla di cartelle: approverebbe di nascosto azioni fuori da lì |
| Errore per un server eliminato | La conversazione resterebbe bloccata senza un posto evidente dove togliere il segno |

## Conseguenze

- Il primo messaggio con un server nuovo aspetta il suo avvio (con `npx` la prima volta
  anche il download, entro 90 secondi).
- Un server con stato (Memory) lo tiene per tutta la conversazione, poi lo perde se non lo
  salva da sé su file.
- Le conferme in chat possono essere più numerose che negli agenti: è voluto.
