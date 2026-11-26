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
  "Questo modello non sa usare gli strumenti (file, ricerca web, documenti, server MCP): il suo template non li prevede, quindi ha risposto solo con quello che sa. Per usarli carica un modello che li supporta, come Qwen2.5 o Nemotron.":
    "This model cannot use tools (files, web search, documents, MCP servers): its template does not support them, so it answered only with what it knows. To use them, load a model that supports them, such as Qwen2.5 or Nemotron.",
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
  "Il file scelto non è un backup di PersonAIly Hub.": "The chosen file is not a PersonAIly Hub backup.",
  "Nessun aggiornamento da installare: controlla di nuovo.": "No update to install: check again.",
  "Questa è la versione portatile: scarica la nuova dalla pagina delle release.":
    "This is the portable version: download the new one from the releases page.",

  // Errori meno frequenti
  "Il limite di spesa deve essere un numero positivo.": "The spending limit must be a positive number.",
  "Ferma l'agente prima di annullare le sue modifiche.": "Stop the agent before undoing its changes.",
  "Il modello locale si sta caricando: aspetta.": "The local model is loading: wait.",
  "Il modello di embedding non è nella libreria.": "The embedding model is not in the library.",
  "Scegli un modello di embedding in Impostazioni → Documenti consultabili.":
    "Choose an embedding model in Settings → Searchable documents.",
  "L'indicizzazione è già in corso.": "Indexing is already running.",
  "Nessun brano pertinente nei documenti.": "No relevant passage in the documents.",
  'Il modello caricato non sa completare il codice in linea (gli manca il "fill-in-the-middle"): carica un modello per il codice, come Qwen2.5-Coder.':
    'The loaded model cannot complete code inline (it lacks "fill-in-the-middle"): load a code model, such as Qwen2.5-Coder.',
  "Collocazione ibrida: scegli quanti layer mettere sulla GPU (almeno uno, non tutti).":
    "Hybrid placement: choose how many layers go on the GPU (at least one, not all).",
  "Questa sessione non ha un punto di ripristino.": "This session has no restore point.",
  "Il punto di ripristino non c'è più.": "The restore point no longer exists.",
  "Cartelle riportate a prima della sessione.": "Folders brought back to before the session.",
  "L'API locale non è attiva.": "The local API is not running.",
  "non è un file GGUF": "not a GGUF file",
  "Il file di blocco del nucleo non si riesce a creare.": "The core lock file cannot be created.",
  "Questa versione di llama.cpp non è installata.": "This version of llama.cpp is not installed.",
  "Questa versione di llama.cpp è in uso: scarica prima il modello.":
    "This version of llama.cpp is in use: unload the model first.",
  "Un'installazione è in corso: riprova quando finisce.": "An installation is in progress: try again when it finishes.",
  "La cartella di destinazione non è tra quelle della libreria.":
    "The destination folder is not one of the library folders.",
  "Questo file si sta già scaricando.": "This file is already downloading.",
  "Si può modificare solo un tuo messaggio.": "Only your own messages can be edited.",
  "Il messaggio è vuoto.": "The message is empty.",
  "Questa conversazione sta già rispondendo: aspetta o interrompi la risposta.":
    "This conversation is already replying: wait or stop the reply.",
  "Il modello locale caricato non legge le immagini: caricalo con il suo componente visivo (mmproj), scegli un modello cloud che le supporti, oppure togli l'immagine.":
    "The loaded local model cannot read images: load it with its vision component (mmproj), choose a cloud model that supports them, or remove the image.",
  "Un server MCP di questo agente è stato eliminato: togli il segno dall'agente o configura di nuovo il server nelle Impostazioni.":
    "An MCP server of this agent was deleted: untick it in the agent or set the server up again in Settings.",
  "Interrotto prima del risultato.": "Interrupted before the result.",
  "il caricamento del modello ha superato il tempo massimo": "loading the model took longer than the time limit",
  "Ogni variabile d'ambiente ha bisogno di un nome valido.": "Every environment variable needs a valid name.",
  "Metodo non supportato": "Method not supported",
  "[Immagine restituita dal server: non mostrata al modello]": "[Image returned by the server: not shown to the model]",
  "[Audio restituito dal server: non mostrato al modello]": "[Audio returned by the server: not shown to the model]",
  "L'indirizzo deve iniziare con https:// (http:// è ammesso solo per questo computer): altrimenti la chiave API viaggerebbe in chiaro.":
    "The address must start with https:// (http:// is allowed only for this computer): otherwise the API key would travel unencrypted.",
  "Messaggio non trovato: forse è stato eliminato.": "Message not found: it may have been deleted.",
  "il PDF ha una struttura che non so interpretare": "the PDF has a structure that cannot be read",
  "Percorso senza nome di file.": "Path without a file name.",
  "old_text è vuoto: indica il testo da sostituire.": "old_text is empty: give the text to replace.",
  "old_text non compare nel file: rileggilo e copia il testo esatto.":
    "old_text does not appear in the file: read it again and copy the exact text.",
  "Il comando è stato terminato.": "The command was terminated.",
  "Il nucleo è quello dell'app desktop: si chiude chiudendo l'app.":
    "The core belongs to the desktop app: it closes when the app is closed.",
  "Il modello non è nella libreria: aggiungi la sua cartella nella pagina Modelli dell'app.":
    "The model is not in the library: add its folder on the app's Models page.",
  "Le pagine web non possono usare l'API locale.": "Web pages cannot use the local API.",
  "Indirizzo non ammesso.": "Address not allowed.",
  "Un'altra copia dell'app è già aperta: l'API locale è la sua.":
    "Another copy of the app is already open: the local API is its own.",
  "Le immagini vanno inviate come data URL in base64, non come indirizzo web.":
    "Images must be sent as base64 data URLs, not as web addresses.",
  "Immagine non valida: serve un data URL in base64.": "Invalid image: a base64 data URL is needed.",
  "Token mancante o sbagliato: lo trovi nell'app, in Impostazioni → API locale.":
    "Missing or wrong token: you find it in the app, under Settings → Local API.",
  "Indirizzo sconosciuto.": "Unknown address.",
  "non riceve più messaggi.": "no longer receives messages.",
  "ha chiuso il collegamento.": "closed the connection.",
  "Indica l'indirizzo completo del server MCP (https://…).": "Enter the full address of the MCP server (https://…).",
  "L'indirizzo del server MCP deve iniziare con https:// (http:// solo per un server su questo computer).":
    "The MCP server address must start with https:// (http:// only for a server on this computer).",
  "Ogni intestazione HTTP ha bisogno di un nome valido (lettere, cifre e trattini).":
    "Every HTTP header needs a valid name (letters, digits and hyphens).",
  // Skill (ADR 0023)
  "Lo SKILL.md deve cominciare con un'intestazione tra due righe «---» che contiene name e description.":
    "SKILL.md must start with a header between two “---” lines containing name and description.",
  "Manca la descrizione della skill: è ciò che il modello legge per decidere quando usarla.":
    "The skill has no description: it is what the model reads to decide when to use it.",
  "La descrizione della skill è troppo lunga (al massimo 1024 caratteri).":
    "The skill description is too long (1024 characters at most).",
  "Manca il file SKILL.md o non si legge.": "SKILL.md is missing or cannot be read.",
  "La skill è troppo grande: al massimo 200 file e 20 MB.": "The skill is too big: 200 files and 20 MB at most.",
  "Nella cartella o nello zip scelto non c'è un file SKILL.md.": "There is no SKILL.md file in the chosen folder or zip.",
  "Il file scelto non è un archivio .zip leggibile.": "The chosen file is not a readable .zip archive.",
  "Lo zip contiene più skill: importale una alla volta.": "The zip contains several skills: import them one at a time.",
  "Questa skill non esiste più.": "This skill no longer exists.",
  "Il server di questo prompt non è più attivo.": "The server of this prompt is no longer running.",
  "Questa conversazione non usa server MCP.": "This conversation uses no MCP servers.",
  // Plugin (ADR 0026)
  "Il plugin è troppo grande: al massimo 500 file e 50 MB.": "The plugin is too big: 500 files and 50 MB at most.",
  "Nella cartella o nello zip scelto non c'è un plugin (manca .claude-plugin/plugin.json).":
    "There is no plugin in the chosen folder or zip (.claude-plugin/plugin.json is missing).",
  "Lo zip contiene più plugin: importali uno alla volta.": "The zip contains several plugins: import them one at a time.",
  "Questo plugin non è installato.": "This plugin is not installed.",
  "Il plugin non ha più questo comando.": "The plugin no longer has this command.",
  "Il file di questo comando non c'è più: reinstalla il plugin.": "This command's file is gone: reinstall the plugin.",
  "Il file dei server MCP indicato nel manifest non c'è.": "The MCP server file named in the manifest is missing.",
  "Il plugin contiene hooks: l'app non li supporta e li salta.": "The plugin contains hooks: the app does not support them and skips them.",
  "Il plugin contiene server LSP: l'app non li supporta e li salta.": "The plugin contains LSP servers: the app does not support them and skips them.",
  "Il plugin contiene stili di risposta: l'app non li supporta e li salta.": "The plugin contains output styles: the app does not support them and skips them.",
  // Accesso OAuth ai server MCP (ADR 0024)
  "Il server non chiede un accesso OAuth: scegli «Intestazioni» (o nessuna) nel modulo del server.":
    "The server does not ask for an OAuth sign-in: choose “Headers” (or none) in the server form.",
  "Il server non indica il suo server di autorizzazione.": "The server does not name its authorization server.",
  "Il server di autorizzazione non pubblica i suoi metadati: l'accesso non si può fare.":
    "The authorization server does not publish its metadata: the sign-in cannot be done.",
  "Il server di autorizzazione non supporta PKCE con S256: l'accesso sarebbe insicuro.":
    "The authorization server does not support PKCE with S256: the sign-in would be unsafe.",
  "Il server di autorizzazione non indica la pagina di accesso.": "The authorization server does not name its sign-in page.",
  "Il server di autorizzazione non indica dove chiedere i token.": "The authorization server does not say where to request tokens.",
  "Il servizio non permette all'app di registrarsi da sola: registra un'app OAuth presso il servizio (indirizzo di ritorno http://127.0.0.1/callback) e scrivi il suo Client ID nel modulo.":
    "The service does not let the app register itself: register an OAuth app with the service (redirect address http://127.0.0.1/callback) and enter its Client ID in the form.",
  "Il servizio non ha restituito il codice di accesso.": "The service did not return the sign-in code.",
  "Salva il server prima di fare l'accesso.": "Save the server before signing in.",
  "Questo server non usa l'accesso dal browser: sceglilo nel modulo e salva.":
    "This server does not use browser sign-in: choose it in the form and save.",
  "Nessun accesso in corso per questo server.": "No sign-in in progress for this server.",
  "L'accesso è già in attesa.": "The sign-in is already waiting.",
  "L'accesso non è stato completato entro 5 minuti: riprova.": "The sign-in was not completed within 5 minutes: try again.",
  "Accesso annullato.": "Sign-in cancelled.",
  "Salva il server e fai l'accesso prima di provarlo.": "Save the server and sign in before testing it.",
  // Modelli di riserva
  "La coda di riserva accetta solo modelli cloud.": "The fallback queue accepts only cloud models.",
};

/** Testi con parti variabili: espressione → sostituzione, provate in ordine. */
export const CORE_PATTERNS_EN: [RegExp, string][] = [
  // Richieste di conferma e registro delle azioni
  [/^Elencare la cartella (.*)$/s, "List the folder $1"],
  [/^Leggere il file (.*)$/s, "Read the file $1"],
  [/^Ripristinare le cartelle: (.*)$/s, "Restore the folders: $1"],
  [/^Affidare a (.*?): (.*)$/s, "Hand over to $1: $2"],
  [/^Cercare nei documenti: (.*)$/s, "Search the documents: $1"],
  [/^Caricare la skill «(.*)»$/s, "Load the skill “$1”"],
  [/^Server MCP «(.*)» saltato: usa il trasporto SSE, che l'app non supporta\.$/s, "MCP server “$1” skipped: it uses the SSE transport, which the app does not support."],
  [/^Server MCP «(.*)» saltato: manca il comando o l'indirizzo\.$/s, "MCP server “$1” skipped: the command or address is missing."],
  [/^Server MCP «(.*)»: completa nelle Impostazioni i valori di (.*)\.$/s, "MCP server “$1”: fill in the values of $2 in Settings."],
  [
    /^Comando \/(.*): le righe che eseguono comandi \(!`…`\) restano testo, l'app non le esegue\.$/s,
    "Command /$1: the lines that run commands (!`…`) stay as text, the app does not run them.",
  ],
  [/^File del plugin illeggibile \((.*?)\): (.*)$/s, "Unreadable plugin file ($1): $2"],
  [
    /^Nome del plugin non valido: «(.*)»\. (.*)$/s,
    "Invalid plugin name: “$1”. The manifest needs a name with only lowercase letters, digits and hyphens.",
  ],
  [
    /^Il plugin contiene skill con lo stesso nome di skill già installate: (.*)\. Eliminale o rinominale prima di installarlo\.$/s,
    "The plugin contains skills with the same name as installed skills: $1. Delete or rename them before installing it.",
  ],
  [/^Esiste già una skill di nome «(.*)»\.$/s, "A skill named “$1” already exists."],
  [
    /^Fai l'accesso al server MCP «(.*)»: Impostazioni → Server MCP → Modifica → Accedi\.$/s,
    "Sign in to the MCP server “$1”: Settings → MCP servers → Edit → Sign in.",
  ],
  [/^Accesso non concesso dal servizio: (.*)$/s, "Access not granted by the service: $1"],
  [/^Il servizio non ha dato i token \((.*?)\)\. (.*)$/s, "The service did not issue the tokens ($1). $2"],
  [
    /^Il servizio ha rifiutato la registrazione dell'app \((.*?)\)\. (.*)$/s,
    "The service refused to register the app ($1). $2",
  ],
  [
    /^Il server di autorizzazione indica un indirizzo non sicuro \(serve https:\/\/\): (.*)$/s,
    "The authorization server gives an unsafe address (https:// is required): $1",
  ],
  [/^Leggere «(.*)» della skill «(.*)»$/s, "Read “$1” of the skill “$2”"],
  [/^Nome della skill non valido: «(.*)»\. (.*)$/s, "Invalid skill name: “$1”. Use only lowercase letters, digits and hyphens (64 characters at most)."],
  [/^Esiste già una skill di nome «(.*)»: scegline un altro\.$/s, "A skill named “$1” already exists: choose another name."],
  [/^Il nome nello SKILL\.md \(«(.*)»\) non corrisponde alla cartella\.$/s, "The name in SKILL.md (“$1”) does not match the folder."],
  [/^Sessione (\d+) di (.*?), stato: (\w+)\.(.*)$/s, "Session $1 of $2, status: $3.$4"],
  [/^Ripristino non riuscito per:\n(.*)$/s, "Restore failed for:\n$1"],
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
  [/^Elencare le risorse del server MCP «(.*)»$/s, "List the resources of the MCP server “$1”"],
  [/^Leggere una risorsa del server MCP «(.*)»$/s, "Read a resource of the MCP server “$1”"],
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

  // Errori meno frequenti
  [/^Manca la chiave API di (.*): inseriscila nella pagina Modelli\.$/s, "The API key for $1 is missing: enter it on the Models page."],
  [/^(.*) è un'immagine troppo grande \(massimo 5 MB\)\.$/s, "$1 is an image that is too large (5 MB at most)."],
  [
    /^(.*) non contiene testo leggibile \(un PDF fatto di sole immagini\?\)\.$/s,
    "$1 contains no readable text (a PDF made only of images?).",
  ],
  [
    /^Il servizio ha risposto con un errore \((.*?)\): (.*)\. Controlla la chiave API\.$/s,
    "The service replied with an error ($1): $2. Check the API key.",
  ],
  [
    /^Il servizio ha risposto con un errore \((.*?)\): (.*)\. Troppe richieste o credito esaurito: riprova più tardi\.$/s,
    "The service replied with an error ($1): $2. Too many requests or credit used up: try again later.",
  ],
  [/^Il servizio ha risposto con un errore \((.*?)\): (.*)\.$/s, "The service replied with an error ($1): $2."],
  [/^La coda di riserva può avere al massimo (\d+) modelli\.$/, "The fallback queue can hold at most $1 models."],
  [/^Il servizio ha interrotto la risposta: (.*)$/s, "The service interrupted the reply: $1"],
  [/^nessuna risposta a «(.*?)» entro (\d+) secondi\.$/s, "no reply to “$1” within $2 seconds."],
  [/^nessuna risposta a «(.*?)»\.$/s, "no reply to “$1”."],
  [/^ha mandato un messaggio più grande di (\d+) MB\.$/s, "sent a message larger than $1 MB."],
  [
    /^Il backup è troppo grande da ripristinare \(più di (\d+) GB estratto\)\.$/s,
    "The backup is too large to restore (more than $1 GB once extracted).",
  ],
  [/^ha chiuso il flusso senza rispondere a «(.*?)»\.$/s, "closed the stream without replying to “$1”."],
  [/^non raggiungibile \((.*)\)\.$/s, "not reachable ($1)."],
  [/^risposta illeggibile \((.*)\)\.$/s, "unreadable reply ($1)."],
  [/^collegamento interrotto \((.*)\)\.$/s, "connection interrupted ($1)."],
  [
    /^ha risposto (.*?)\. Controlla le intestazioni \(per esempio il token in Authorization\)\.(.*)$/s,
    "replied $1. Check the headers (for example the token in Authorization).$2",
  ],
  [/^ha risposto (.*?)\. Controlla l'indirizzo\.(.*)$/s, "replied $1. Check the address.$2"],
  [/^ha risposto (.*?)\.(.*)$/s, "replied $1.$2"],
  [/^dati non validi: (.*)$/s, "invalid data: $1"],
  [/^versione GGUF (.*) non supportata$/s, "GGUF version $1 not supported"],
  [/^(.*) è già presente nella cartella dei modelli\.$/s, "$1 is already in the models folder."],
  [
    /^(.*): il file scaricato non corrisponde a quello pubblicato \(download rovinato\)\. È stato eliminato: riprova\.$/s,
    "$1: the downloaded file does not match the published one (damaged download). It was deleted: try again.",
  ],
  [/^generatore casuale di sistema: (.*)$/s, "system random generator: $1"],
  [/^portachiavi di sistema: (.*)$/s, "system keyring: $1"],
  [/^file del modello non leggibile: (.*)$/s, "model file not readable: $T1"],
  [/^il modello è diviso in (\d+) file e ne mancano (\d+)$/s, "the model is split into $1 files and $2 are missing"],
  [
    /^Il modello non entra: servono circa (.*) GB di VRAM e (.*) GB di RAM, liberi (.*) GB e (.*) GB\.$/s,
    "The model does not fit: it needs about $1 GB of VRAM and $2 GB of RAM, $3 GB and $4 GB free.",
  ],
  [
    /^Il modello non entra nella (VRAM|RAM): servono circa (.*) GB, liberi (.*) GB\.$/s,
    "The model does not fit in $1: it needs about $2 GB, $3 GB free.",
  ],
  [/^(.*) — indirizzo scritto dal modello, non da te né da una ricerca: controlla che non contenga dati dei tuoi file\.$/s,
    "$T1 — an address written by the model, not by you or a search: check that it contains none of your files' data."],
  [/^\n?\[Immagine allegata che questo modello non può vedere: (.*)\]$/s, "[Attached image this model cannot see: $1]"],
  [
    /^La cartella (.*) non è tra quelle concesse ai modelli: concedila prima nelle Impostazioni\.$/s,
    "The folder $1 is not among those granted to the models: grant it first in Settings.",
  ],
  [/^modello di embedding: risposta senza vettori$/s, "embedding model: reply without vectors"],
  [/^modello di embedding: numero di vettori sbagliato$/s, "embedding model: wrong number of vectors"],
  [/^modello di embedding: (.*)$/s, "embedding model: $1"],
  [
    /^Il backup viene da una versione più recente dell'app \((.*)\): aggiorna l'app prima di ripristinarlo\.$/s,
    "The backup comes from a newer version of the app ($1): update the app before restoring it.",
  ],
  [/^backup non riuscito: (.*)$/s, "backup failed: $1"],
  [/^Controllo degli aggiornamenti non riuscito: (.*)$/s, "Update check failed: $1"],
  [/^Download dell'aggiornamento non riuscito: (.*)$/s, "Update download failed: $1"],
  [/^Installazione dell'aggiornamento non riuscita: (.*)$/s, "Update installation failed: $1"],
  [/^backup rovinato: (.*)$/s, "damaged backup: $1"],
  [/^archivio rovinato: (.*)$/s, "damaged database: $1"],
  [/^archivio non valido: (.*)$/s, "invalid archive: $1"],
  [/^estrazione non riuscita: (.*)$/s, "extraction failed: $1"],
  [/^(.*): il file scaricato è più grande del previsto$/s, "$1: the downloaded file is larger than expected"],
  [
    /^(.*): il file scaricato non corrisponde a quello atteso \(download incompleto o alterato\)$/s,
    "$1: the downloaded file does not match the expected one (incomplete or altered download)",
  ],
  [/^llama\.cpp non è installato: manca (.*) in (.*)$/s, "llama.cpp is not installed: $1 is missing in $2"],
  [/^llama-server si è chiuso durante il caricamento \((.*?)\)\.(.*)$/s, "llama-server closed while loading ($1).$2"],
  [/^Impossibile avviare il server MCP «(.*?)» \((.*?)\): (.*)$/s, "Cannot start the MCP server “$1” ($2): $3"],
  // Server MCP «nome»: motivo[ Il server si è chiuso.][a capo, ultime righe di errore del server]
  [/^Server MCP «(.*?)»: (.*?) Il server si è chiuso\.(\n.*)?$/s, "MCP server “$1”: $T2 The server has closed.$3"],
  [/^Server MCP «(.*?)»: (.*?)(\n.*)?$/s, "MCP server “$1”: $T2$3"],
  [/^Strumento del server MCP «(.*)»\.$/s, "Tool of the MCP server “$1”."],
  [/^Lo strumento "(.*)" non è disponibile\.$/s, 'The tool "$1" is not available.'],
  [
    /^Il nome del preset è troppo lungo \(massimo (\d+) caratteri\)\.$/s,
    "The preset name is too long ($1 characters at most).",
  ],
  [/^Contenuto di (.*):$/s, "Contents of $1:"],
  [/^… e altri (\d+) elementi non mostrati\.$/s, "… and $1 more items not shown."],
  [/^(.*) è troppo grande per essere letto \((.*) MB\)\.$/s, "$1 is too large to read ($2 MB)."],
  [/^Non riesco a leggere il testo di (.*?): (.*)$/s, "Cannot read the text of $1: $T2"],
  [/^(.*) non è un file di testo \(o non è in UTF-8\)\.$/s, "$1 is not a text file (or is not UTF-8)."],
  [/^(.*) è una cartella, non un file\.$/s, "$1 is a folder, not a file."],
  [/^(.*) è troppo grande per essere modificato\.$/s, "$1 is too large to edit."],
  [
    /^old_text compare (\d+) volte nel file: aggiungi righe vicine per renderlo unico\.$/s,
    "old_text appears $1 times in the file: add nearby lines to make it unique.",
  ],
  [/^(.*) non è una cartella esistente\.$/s, "$1 is not an existing folder."],
  [
    /^Il comando ha superato il tempo massimo di (\d+) secondi ed è stato terminato\.$/s,
    "The command exceeded the time limit of $1 seconds and was terminated.",
  ],
  [/^Codice di uscita: (.*)$/s, "Exit code: $1"],
  [/^La cartella (.*) non esiste\.$/s, "The folder $1 does not exist."],
  [/^Impossibile avviare il comando: (.*)$/s, "Cannot start the command: $1"],
  [/^Il motore di ricerca ha risposto con un errore \((.*)\)\.$/s, "The search engine replied with an error ($1)."],
  [/^Nessun risultato per "(.*)"\.$/s, 'No results for "$1".'],
  [/^Risultati per "(.*)":$/s, 'Results for "$1":'],
  [/^La pagina non è testo \((.*)\): non posso leggerla\.$/s, "The page is not text ($1): it cannot be read."],
  [
    /^La pagina (.*) non contiene testo leggibile \(forse è costruita con JavaScript\)\.$/s,
    "The page $1 contains no readable text (perhaps it is built with JavaScript).",
  ],
  [/^Testo di (.*):$/s, "Text of $1:"],
  [/^impossibile aprire una porta per l'API locale: (.*)$/s, "cannot open a port for the local API: $1"],
  [/^Un altro nucleo è già attivo \(processo (\d+), porta (\d+)\)\.$/s, "Another core is already running (process $1, port $2)."],
  [/^Il nucleo senza interfaccia \(processo (\d+)\) non si è chiuso\.$/s, "The headless core (process $1) did not close."],
  [/^Ruolo del messaggio sconosciuto: "(.*)"\.$/s, 'Unknown message role: "$1".'],
  [/^Parte del messaggio non gestita: "(.*)"\.$/s, 'Unsupported message part: "$1".'],
  [
    /^Limite di spesa mensile raggiunto: spesi circa (.*) \$ su (.*) \$\. Puoi alzarlo in Impostazioni → Spesa cloud, o usare un modello locale\.$/s,
    "Monthly spending limit reached: about $$1 spent of $$2. You can raise it in Settings → Cloud spending, or use a local model.",
  ],
  [
    /^Limite di spesa per conversazione raggiunto: spesi circa (.*) \$ su (.*) \$\. Puoi alzarlo in Impostazioni → Spesa cloud, o continuare in una conversazione nuova\.$/s,
    "Per-conversation spending limit reached: about $$1 spent of $$2. You can raise it in Settings → Cloud spending, or continue in a new conversation.",
  ],
];
