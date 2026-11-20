// Traduzione inglese dei testi dell'estensione. La chiave è il testo italiano
// scritto nel codice; `npm run check:i18n` segnala quelli che mancano.

export const EN: Record<string, string> = {
  // ── Collegamento al nucleo ──
  "Il nucleo di PersonAIly Hub non è attivo e app-server.exe non si trova. Apri l'app desktop, oppure indica nell'impostazione “personailyHub.serverPath” il percorso di app-server.exe per avviarlo senza app.":
    "The PersonAIly Hub core is not running and app-server.exe cannot be found. Open the desktop app, or set “personailyHub.serverPath” to the path of app-server.exe to start it without the app.",
  "Avvio del nucleo senza interfaccia: {path}": "Starting the headless core: {path}",
  "Avvio non riuscito: {error}": "Start failed: {error}",
  "Impossibile avviare {path}: {error}": "Cannot start {path}: {error}",
  "Il nucleo senza interfaccia non è partito entro 20 secondi.": "The headless core did not start within 20 seconds.",
  "Il nucleo non risponde: {error}": "The core does not respond: {error}",
  "Errore {status}": "Error {status}",
  "Risposta vuota dal nucleo.": "Empty reply from the core.",

  // ── Modelli ──
  "PersonAIly Hub: scegli il modello": "PersonAIly Hub: choose the model",
  "Scegli il modello": "Choose the model",
  "Locale": "Local",
  "caricato": "loaded",
  "da caricare": "to load",
  "Sostituisce il modello caricato ora.": "Replaces the model loaded now.",
  "Nessun modello disponibile: aggiungi una cartella di modelli o un provider cloud nella pagina Modelli dell'app.":
    "No model available: add a model folder or a cloud provider on the app's Models page.",
  "Caricamento di {name}…": "Loading {name}…",
  "{error} Caricarlo comunque? Potrebbe rallentare molto il computer.":
    "{error} Load it anyway? It could slow the computer down a lot.",
  "Carica comunque": "Load anyway",

  // ── Chat ──
  "PersonAIly Hub: apri una conversazione": "PersonAIly Hub: open a conversation",
  "Nuova conversazione": "New conversation",
  "Includi il file aperto o la selezione": "Include the open file or the selection",
  "Scrivi un messaggio… (Invio per inviare, Maiusc+Invio per andare a capo)":
    "Write a message… (Enter to send, Shift+Enter for a new line)",
  "Invia": "Send",
  "Interrompi": "Stop",
  "Sto rispondendo…": "Replying…",
  "Sto ragionando…": "Thinking…",
  "Fai una domanda sul codice. Con la casella sotto spuntata il modello vede il file aperto, o solo la selezione. Per modificare direttamente il file: Ctrl+Alt+I.":
    "Ask a question about the code. With the box below ticked the model sees the open file, or just the selection. To edit the file directly: Ctrl+Alt+I.",
  "Applica come diff": "Apply as diff",
  "Mostra la modifica nel file e chiedi conferma": "Show the change in the file and ask for confirmation",
  "Copia": "Copy",
  "Copiato": "Copied",
  "strumento": "tool",
  "Usa: {tools}": "Uses: {tools}",
  "Il modello chiede: {summary}": "The model asks: {summary}",
  "Consenti": "Allow",
  "Consenti sempre": "Always allow",
  "Nega": "Deny",
  "Apri il file a cui applicare il codice.": "Open the file to apply the code to.",

  // ── Contesto dell'editor e modifiche ──
  "La selezione è troppo lunga ({n} caratteri, massimo {max}).": "The selection is too long ({n} characters, {max} at most).",
  "Il file è troppo lungo ({n} caratteri, massimo {max}): seleziona la parte che ti interessa.":
    "The file is too long ({n} characters, {max} at most): select the part you care about.",
  "Righe {start}-{end} del file {file}.": "Lines {start}-{end} of the file {file}.",
  "File intero: {file}.": "Whole file: {file}.",
  "Istruzione:": "Instruction:",
  "{file}, righe {start}-{end} selezionate": "{file}, lines {start}-{end} selected",
  "{file}, file intero": "{file}, whole file",
  "Apri il file da modificare.": "Open the file to edit.",
  "righe {start}-{end}": "lines {start}-{end}",
  "tutto il file": "the whole file",
  "Che modifica vuoi su {where}?": "What change do you want on {where}?",
  "Per esempio: aggiungi la gestione degli errori": "For example: add error handling",
  "{model} sta scrivendo la modifica": "{model} is writing the change",
  "{n} caratteri": "{n} characters",
  "Il modello non ha proposto codice.": "The model did not propose any code.",
  "Accetta la modifica": "Accept the change",
  "Rifiuta": "Reject",
  "Il modello non ha proposto cambiamenti.": "The model did not propose any changes.",
  "{name} ↔ proposta di PersonAIly Hub": "{name} ↔ PersonAIly Hub proposal",
  "Il file è cambiato nel punto da modificare dopo la proposta: chiedi di nuovo la modifica.":
    "The file changed where the edit goes after the proposal: ask for the change again.",
  "Non è stato possibile applicare la modifica.": "The change could not be applied.",

  // ── Agenti ──
  "Nessun agente: crealo nella pagina Agenti dell'app.": "No agents: create one on the app's Agents page.",
  "PersonAIly Hub: scegli l'agente": "PersonAIly Hub: choose the agent",
  "Che cosa deve fare?": "What should it do?",
  "{agent} è al lavoro. Le richieste di conferma arrivano qui come notifiche.":
    "{agent} is working. Approval requests arrive here as notifications.",
  "Nessun agente al lavoro.": "No agent is working.",
  "PersonAIly Hub: agenti al lavoro": "PersonAIly Hub: working agents",
  "Ferma": "Stop",
  "{n} agenti": "{n} agents",
  "Agenti al lavoro: {names}": "Working agents: {names}",
  "Consenti sempre in questa sessione": "Always allow in this session",
  "{agent} chiede il permesso di: {summary}": "{agent} asks permission to: {summary}",
  "Mostra il resoconto": "Show the report",
  "Compito": "Task",
  "(nessuna risposta)": "(no reply)",
  "al lavoro": "working",
  "in pausa": "paused",
  "aspetta una conferma": "waiting for approval",
  "ha finito": "finished",
  "fermato": "stopped",
  "non riuscito": "failed",
  "interrotto": "interrupted",
  "Completamento in linea: {error}": "Inline completion: {error}",
};
