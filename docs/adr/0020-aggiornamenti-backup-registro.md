# 0020 — Aggiornamento automatico, backup dei dati, registro diagnostico

**Stato:** accettata e realizzata (20-11-2026) in preparazione della 1.0.0. Backup,
ripristino e aggiornamento di un archivio della 0.1.0 provati dai test automatici.
L'aggiornamento automatico va provato con la prima release firmata: la 0.1.0 non ha
l'aggiornamento, quindi chi la usa deve installare la 1.0.0 a mano una volta.

## Contesto

Con la 0.1.0 pubblicata, l'app ha utenti veri. Per arrivare a una 1.0.0 servono tre cose
che non c'erano: un modo per ricevere le versioni nuove senza riscaricarle a mano, un
modo per non perdere i dati (in un aggiornamento dello schema sbagliato, cambiando PC) e
un modo per capire che cosa è andato storto quando un utente segnala un problema.

## Decisione

**Aggiornamento automatico** con `tauri-plugin-updater`.

- All'avvio, 5 secondi dopo l'apertura della finestra, l'app legge
  `releases/latest/download/latest.json` da GitHub. È l'unica richiesta che fa da sola;
  non manda dati dell'utente e si spegne in Impostazioni → Aggiornamenti
  (`Preferences.check_updates`, acceso di base). Un errore di rete non mostra nulla.
- Gli installatori sono **firmati con una chiave del progetto** (minisign): la chiave
  pubblica sta in `tauri.conf.json`, quella privata solo nei segreti di GitHub. Con
  `requireSignedVersion` la firma copre anche il numero di versione, quindi un
  `latest.json` alterato non può riportare l'utente a una versione vecchia.
- I controlli e l'installazione passano da comandi Rust (`check_update`,
  `install_update`): l'interfaccia non riceve i permessi del plugin.
- Prima di avviare l'installatore l'app fa ciò che fa alla chiusura: toglie il file di
  blocco, ferma gli agenti, scarica il modello. L'installatore (modalità `passive`) chiude
  l'app e la riapre aggiornata.
- L'exe portatile non ha un installatore da avviare (lo si riconosce perché accanto non
  c'è `uninstall.exe`): mostra la versione nuova e apre la pagina delle release.
- `scripts/release.ps1` si rifiuta di costruire una release senza la chiave e scrive
  `latest.json` con le note prese dalla sezione della versione in `CHANGELOG.md`.

**Backup e ripristino** (`crates/core/src/backup.rs`).

- Il backup è uno `.zip` con l'archivio copiato con `VACUUM INTO` (coerente anche
  mentre l'app lavora), i preset e `backup.json` (versione, schema, data).
- Chiavi API, token e variabili dei server MCP restano nel Gestore credenziali e non
  sono inclusi; nemmeno modelli e llama.cpp. L'interfaccia lo dice.
- Il ripristino non tocca l'archivio aperto: controlla il file (deve essere un backup
  dell'app, integro, con uno schema non più recente di quello che l'app conosce), lo
  prepara in `restore-pending/` e riavvia l'app. All'avvio, prima di aprire l'archivio,
  i dati attuali passano in `backups/before-restore-<data>/` e quelli del backup al loro
  posto: anche un ripristino si può annullare a mano.
- **Copia prima di ogni aggiornamento dello schema**: aprendo un archivio di uno schema
  precedente, `Store::open` ne salva una copia in `backups/data-schema<N>-<data>.sqlite`
  prima di applicare i passi nuovi. Se ne tengono le ultime tre.
- Un test costruisce un archivio con i primi 6 passi (lo schema della 0.1.0) e i dati
  nella forma di allora, poi controlla che si aggiorni senza perdere nulla.

**Registro diagnostico** (`crates/core/src/log.rs`).

- `logs/core.log`: avvio, schema, caricamento dei modelli, errori mostrati
  all'interfaccia o restituiti dall'API locale, sessioni fallite, panic. Oltre 2 MB
  diventa `core.1.log`. Mai chiavi, token o testo delle conversazioni.
- "Copia le informazioni diagnostiche" (Impostazioni → Segnalare un problema) mette negli
  appunti versione, sistema, hardware, motore, modello caricato, quante cose sono
  configurate e le ultime righe dei registri, con la cartella utente e il nome utente
  sostituiti. Esce dal computer solo se l'utente lo incolla da qualche parte (GE-3).

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Solo un avviso "c'è una versione nuova", da scaricare a mano | Molti utenti resterebbero su versioni vecchie, anche con correzioni di sicurezza |
| Aggiornamento senza firma propria (fidarsi di HTTPS e GitHub) | Un account o un rilascio compromesso porterebbe codice qualunque su tutti i PC |
| Registro con `tracing` o `log` + `env_logger` | Dipendenze in più per ciò che serve: righe con data in un file che ruota |
| Telemetria o invio automatico degli errori | Contro GE-3: i dati restano sul PC finché l'utente non decide |
| Ripristino a caldo, con l'archivio aperto | Connessioni, sessioni e cache in memoria dovrebbero ripartire tutte; il riavvio è più semplice e sicuro |

## Conseguenze

- **Perdere la chiave privata** vuol dire che le app installate non si aggiornano più da
  sole: va conservata anche fuori da GitHub.
- La firma degli aggiornamenti non toglie l'avviso di SmartScreen: per quello serve la
  firma Authenticode degli eseguibili, ancora da fare.
- La cartella `backups/` cresce con i ripristini (le copie prima degli aggiornamenti si
  ripuliscono da sole).
