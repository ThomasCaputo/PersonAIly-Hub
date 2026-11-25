# 0026 — Plugin

**Stato:** accettata e realizzata (25-11-2026). Provata nell'app con un plugin di prova (una skill,
un agente, due comandi e il server MCP Memory): installazione, segni nelle Impostazioni, comandi
nel menu `/` di una conversazione senza server MCP, disinstallazione completa. Manca una prova con
un plugin pubblicato.

## Contesto

Skill ([ADR 0023](0023-skill.md)), server MCP ([ADR 0009](0009-mcp.md), [0022](0022-mcp-in-chat.md),
[0024](0024-oauth-mcp.md)), agenti e comandi `/` ([ADR 0025](0025-prompt-mcp.md)) si aggiungono uno
alla volta. Chi prepara un insieme pensato per un lavoro (per esempio "revisione del codice":
una skill, un server MCP, un agente e qualche comando) non ha modo di distribuirlo come un
pacchetto unico, e chi lo riceve deve configurare ogni pezzo a mano.

## Decisione

**Formato: quello dei plugin di Claude Code**, come per le skill, così i plugin già pubblicati
si installano così come sono:

| Parte | Dove | Che cosa diventa nell'app |
|---|---|---|
| Manifest | `.claude-plugin/plugin.json` (oppure `plugin.json` in cima) | nome (regole dei nomi delle skill), versione, descrizione, autore |
| Skill | `skills/<nome>/SKILL.md` | skill installate e accese |
| Server MCP | `.mcp.json`, oppure `mcpServers` nel manifest (oggetto o percorso di un file) | server MCP configurati, che nessuna conversazione o agente usa finché l'utente non li sceglie |
| Agenti | `agents/*.md` (intestazione con `name`, `description`, `tools`; il testo sono le istruzioni) | agenti, senza cartelle: le sceglie l'utente |
| Comandi | `commands/**/*.md` (intestazione con `description`, `argument-hint`) | comandi `/` in tutte le conversazioni, accanto ai prompt MCP |

- **Server MCP.** `${CLAUDE_PLUGIN_ROOT}` diventa la cartella del plugin installato. Le altre
  `${VARIABILE}` (anche `${VARIABILE:-predefinito}`) prendono il valore che hanno in Windows al
  momento dell'installazione; se non c'è, il valore resta vuoto e l'installazione lo segnala tra
  le cose da completare (nelle Impostazioni, come per ogni server). I server di tipo `sse` (il
  trasporto MCP vecchio) si saltano, con un avviso. I segreti finiscono nel Gestore credenziali
  come sempre. **Installare non avvia nulla**: i server partono solo quando l'utente li sceglie.
- **Agenti.** Il campo `tools` diventa gli strumenti dell'agente: `Read`, `Write`, `Edit`, `Glob`,
  `Grep`… accendono i file, `Bash` il terminale, `WebFetch` e `WebSearch` la ricerca web. Senza
  `tools`, gli strumenti predefiniti di un agente nuovo. `model` si ignora: l'agente usa il
  modello locale finché l'utente non ne sceglie un altro.
- **Comandi.** Il testo del file (senza intestazione) va nella casella del messaggio, come per
  i prompt MCP. `$ARGUMENTS` diventa ciò che l'utente scrive nel modulo del comando, `$1`, `$2`…
  le sue parole. Le righe che in Claude Code eseguono comandi (`` !`…` ``) restano testo, con un
  avviso all'installazione.
- **Non supportati**, con un avviso: hooks, server LSP e le altre parti che non hanno un
  equivalente nell'app.
- **Installazione.** Da **Impostazioni → Plugin**, da una cartella o da un `.zip`, entro 500 file e
  50 MB, con le stesse protezioni delle skill (collegamenti saltati, percorsi che escono dallo
  zip rifiutati). Il plugin si copia in `<dati>/plugins/<nome>/`. Un registro nell'archivio
  ricorda che cosa ha creato (skill, server, agenti, comandi). Se una skill del plugin ha il nome
  di una skill che non è sua, l'installazione si ferma prima di cambiare qualcosa. Se qualcosa va
  storto a metà, ciò che è già stato creato si toglie.
- **Disinstallazione** toglie tutto ciò che il plugin aveva creato, segreti e accessi OAuth dei
  suoi server compresi. **Sostituire** un plugin con lo stesso nome (un aggiornamento) è
  disinstallarlo e installare il nuovo: le impostazioni date nel frattempo ai suoi server e
  agenti ripartono da capo, e la richiesta di conferma lo dice.
- Le parti di un plugin restano modificabili come le altre. Skill e server di un plugin si
  riconoscono nelle Impostazioni e si eliminano solo disinstallando il plugin.
- **Backup.** La cartella `plugins/` entra nel `.zip` del backup, come `skills/` e `presets/`.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Un formato proprio | I plugin esistenti non sarebbero installabili |
| Avviare i server MCP all'installazione, per provarli | Un plugin scaricato eseguirebbe programmi senza che l'utente li abbia scelti |
| Eseguire le righe `` !`…` `` dei comandi | Comandi di sistema scritti da altri, eseguiti senza le conferme del terminale |
| Tenere le impostazioni di server e agenti quando si aggiorna un plugin | Le parti possono cambiare nome o sparire da una versione all'altra; meglio ripartire puliti e dirlo |
| Plugin accesi o spenti in blocco | Ogni parte si accende già per conto suo (skill, server per conversazione, agenti) |

## Conseguenze

- Un plugin si fida come un programma: può contenere server MCP che eseguono codice, quando
  l'utente li sceglie, e istruzioni che il modello segue. L'interfaccia lo ricorda.
- Gli agenti di un plugin vanno completati con le cartelle (e, se serve, il modello) prima di usarli.
