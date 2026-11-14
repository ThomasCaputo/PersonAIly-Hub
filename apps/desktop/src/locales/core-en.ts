// Traduzione inglese dei testi che arrivano dal nucleo, che scrive in italiano:
// avvisi sotto le risposte, richieste di conferma, errori frequenti. Se il
// nucleo cambia uno di questi testi, va cambiato anche qui (altrimenti esce in
// italiano, senza altri danni).

/** Testi esatti. */
export const CORE_EN: Record<string, string> = {
  // Avvisi sotto le risposte
  "La risposta è stata troncata: raggiunto il limite di token per risposta.":
    "The reply was cut off: the per-reply token limit was reached.",
  "Il modello ha rifiutato di rispondere a questa richiesta.": "The model refused to answer this request.",
  "Il modello ha usato strumenti troppe volte di seguito: mi sono fermato. Scrivi per farlo continuare.":
    "The model used tools too many times in a row, so it was stopped. Write to make it continue.",
  "Nuova conversazione": "New conversation",

  // Risultati degli strumenti
  "Interrotto dall'utente.": "Interrupted by the user.",
  "Non eseguito: raggiunto il limite di passi.": "Not run: step limit reached.",
  "L'utente ha negato il permesso per questa azione. Non riprovare: chiedigli come vuole procedere.":
    "The user denied permission for this action. Do not retry: ask them how they want to proceed.",
  "Strumento sconosciuto.": "Unknown tool.",
  "I parametri della chiamata non sono un oggetto JSON valido: riprova.":
    "The call parameters are not a valid JSON object: try again.",
  "Nessuna cartella concessa: l'utente deve aggiungerne una nelle Impostazioni.":
    "No folder granted: the user must add one in Settings.",

  // Errori frequenti
  "Agente non trovato: forse è stato eliminato.": "Agent not found: it may have been deleted.",
  "Allegato senza nome.": "Attachment without a name.",
  "C'è già un download in corso.": "A download is already in progress.",
  "Dai un nome al preset.": "Give the preset a name.",
  "Dai un nome al provider.": "Give the provider a name.",
  "Dai un nome al server MCP.": "Give the MCP server a name.",
  "Dai un nome all'agente.": "Give the agent a name.",
  "Ferma l'agente prima di eliminare la sessione.": "Stop the agent before deleting the session.",
  "Fermato durante l'avvio dei server MCP.": "Stopped while starting the MCP servers.",
  "Il file non è di testo: non si può modificare.": "The file is not text: it cannot be edited.",
  "Il file scelto non è un preset valido.": "The chosen file is not a valid preset.",
  "Il sito ha risposto con un rinvio non valido.": "The site replied with an invalid redirect.",
  "Il sito ha risposto con un rinvio senza destinazione.": "The site replied with a redirect without a destination.",
  "Indica il comando che avvia il server MCP.": "Enter the command that starts the MCP server.",
  "Indirizzo non valido: deve essere completo, con https://.": "Invalid address: it must be complete, with https://.",
  "Indirizzo senza nome del sito.": "Address without a site name.",
  "Inserisci prima la chiave API di questo provider.": "First enter this provider's API key.",
  "L'agente sta già lavorando.": "The agent is already working.",
  "La definizione dell'agente di questa sessione è illeggibile.": "This session's agent definition is unreadable.",
  "La ricerca è vuota.": "The search is empty.",
  "La richiesta non contiene messaggi.": "The request contains no messages.",
  "La sessione non sta lavorando.": "The session is not working.",
  "Nessun modello locale caricato.": "No local model loaded.",
  "Nome del file non valido.": "Invalid file name.",
  "Nome del repository non valido.": "Invalid repository name.",
  "Provider non trovato: forse è stato eliminato.": "Provider not found: it may have been deleted.",
  "Scrivi il compito da affidare all'agente.": "Write the task to give the agent.",
  "Scrivi qualcosa all'agente.": "Write something to the agent.",
  "Sessione non trovata: forse è stata eliminata.": "Session not found: it may have been deleted.",
  "Troppi rinvii: il sito non porta a una pagina.": "Too many redirects: the site does not lead to a page.",
  "Un'installazione è già in corso.": "An installation is already in progress.",
  "Si possono leggere solo pagine http:// o https://.": "Only http:// or https:// pages can be read.",
  "Indirizzo non valido.": "Invalid address.",
};

/** Testi con parti variabili: espressione → sostituzione, provate in ordine. */
export const CORE_PATTERNS_EN: [RegExp, string][] = [
  // Richieste di conferma e registro delle azioni
  [/^Elencare la cartella (.*)$/s, "List the folder $1"],
  [/^Leggere il file (.*)$/s, "Read the file $1"],
  [/^Scrivere il file (.*) \((\d+) caratteri\)$/s, "Write the file $1 ($2 characters)"],
  [/^Modificare il file (.*)$/s, "Edit the file $1"],
  [/^Cercare sul web: (.*)$/s, "Search the web: $1"],
  [
    /^Leggere la pagina (.*) — indirizzo scritto dal modello, non da te né da una ricerca: controlla che non contenga dati dei tuoi file\.$/s,
    "Read the page $1 — an address written by the model, not by you or a search: check that it contains none of your files' data.",
  ],
  [/^Leggere la pagina (.*)$/s, "Read the page $1"],
  [/^Eseguire nel terminale \(in (.*?)\): (.*)$/s, "Run in the terminal (in $1): $2"],
  [/^Eseguire nel terminale: (.*)$/s, "Run in the terminal: $1"],
  [/^Usare «(.*)» del server MCP «(.*)»$/s, "Use “$1” from the MCP server “$2”"],
  [/^Usare lo strumento (.*)$/s, "Use the tool $1"],
  // Avvisi ed errori
  [
    /^L'agente ha fatto (\d+) passi, il massimo previsto, e si è fermato\. Rispondigli per farlo continuare\.$/s,
    "The agent took $1 steps, the maximum allowed, and stopped. Reply to make it continue.",
  ],
  [/^Lo strumento "(.*)" non è disponibile in questa conversazione\.$/s, 'The tool "$1" is not available in this conversation.'],
  [
    /^(.*) è fuori dalle cartelle concesse dall'utente: accesso negato\.$/s,
    "$1 is outside the folders granted by the user: access denied.",
  ],
  [/^Percorso non valido: (.*)$/s, "Invalid path: $1"],
  [/^Manca il parametro "(.*)"\.$/s, 'The parameter "$1" is missing.'],
  [/^Sito non trovato: (.*)$/s, "Site not found: $1"],
  [
    /^(.*) non è un sito pubblico: questo strumento non raggiunge il computer dell'utente né la sua rete locale\.$/s,
    "$1 is not a public site: this tool does not reach the user's computer or local network.",
  ],
  [/^Il sito ha risposto con un errore \((.*)\)\.$/s, "The site replied with an error ($1)."],
  [/^Lingua non disponibile: (.*)$/s, "Language not available: $1"],
  [/^Si aprono solo indirizzi web: (.*) non è ammesso\.$/s, "Only web addresses can be opened: $1 is not allowed."],
];
