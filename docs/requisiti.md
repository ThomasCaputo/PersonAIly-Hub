# Requisiti

Cosa deve fare l'app. Ogni requisito ha un codice per poterlo citare in issue e commit.
Priorità: **M** = indispensabile per la prima versione, **S** = importante ma successivo,
**F** = futuro.

## 1. Modelli cloud

| Codice | Requisito | Pr. |
|---|---|---|
| CL-1 | Collegare provider cloud inserendo la propria chiave API | M |
| CL-2 | Supporto ad Anthropic e a qualunque endpoint compatibile con l'API OpenAI (OpenAI, OpenRouter, Groq, Mistral…) | M |
| CL-3 | Supporto a Google Gemini | S |
| CL-4 | Le chiavi sono salvate nel Gestore credenziali di Windows, mai in file di testo | M |
| CL-5 | Elenco dei modelli disponibili per ogni provider, aggiornabile | M |
| CL-6 | Conteggio dei token e stima del costo per conversazione | S |

## 2. Modelli locali

| Codice | Requisito | Pr. |
|---|---|---|
| LO-1 | Eseguire modelli `.gguf` tramite llama.cpp | M |
| LO-2 | Libreria modelli: una o più cartelle scelte dall'utente, con import di file già presenti | M |
| LO-3 | Ricerca e download di modelli GGUF da Hugging Face, con ripresa dei download interrotti | S |
| LO-4 | La cache KV è quantizzata a `q8_0` di default; modificabile per modello | M |
| LO-5 | **Collocazione unica**: modello e cache KV risiedono solo in VRAM oppure solo in RAM, a scelta dell'utente. L'app non tiene una seconda copia nell'altra memoria | M |
| LO-6 | Prima del caricamento l'app stima la memoria necessaria e la confronta con quella libera; se non basta avvisa, invece di dividere il modello in silenzio | M |
| LO-7 | Indicatori in tempo reale: VRAM e RAM usate, token al secondo, contesto occupato | M |
| LO-8 | Caricare e scaricare il modello a mano; scaricamento automatico dopo un tempo di inattività configurabile | S |
| LO-9 | Scelta della variante di llama.cpp (CUDA, Vulkan, solo CPU) e suo aggiornamento dall'app | S |
| LO-10 | Modalità ibrida GPU + CPU per modelli che non entrano in VRAM, solo come scelta esplicita e avanzata | F |

## 3. Parametri dei modelli

Come in LM Studio: ogni modello ha un pannello di parametri facile da modificare.

| Codice | Requisito | Pr. |
|---|---|---|
| PA-1 | Parametri di generazione modificabili al volo: temperatura, top-p, top-k, min-p, penalità di ripetizione, seed, token massimi, sequenze di stop | M |
| PA-2 | Parametri di caricamento (richiedono di ricaricare il modello): lunghezza del contesto, collocazione VRAM/RAM, tipo di cache KV, thread CPU, dimensione del batch | M |
| PA-3 | Prompt di sistema per modello e per conversazione | M |
| PA-4 | Preset salvabili con un nome, esportabili e importabili come file JSON | S |
| PA-5 | L'interfaccia distingue chiaramente i parametri immediati da quelli che richiedono il ricaricamento | M |
| PA-6 | Per i modelli cloud sono mostrati solo i parametri che il provider accetta davvero | M |

## 4. Modalità chat

| Codice | Requisito | Pr. |
|---|---|---|
| CH-1 | Conversazioni con risposta in streaming, interrompibile | M |
| CH-2 | Cronologia salvata in locale, con ricerca, rinomina ed eliminazione | M |
| CH-3 | Cambio di modello a metà conversazione | M |
| CH-4 | Rendering di Markdown, codice con evidenziazione, formule | M |
| CH-5 | Allegati: file di testo, PDF, immagini (per i modelli con visione) | M |
| CH-6 | Ricerca web: il modello può cercare e leggere pagine, citando le fonti | M |
| CH-7 | Accesso ai file del PC: l'utente concede una o più cartelle e il modello può leggere, creare e modificare file al loro interno | M |
| CH-8 | Modificare un proprio messaggio e rigenerare la risposta | S |
| CH-9 | Esportazione della conversazione in Markdown | S |

## 5. Modalità agenti

Un **agente** è la combinazione di: modello, istruzioni, strumenti concessi, permessi e
cartella di lavoro.

| Codice | Requisito | Pr. |
|---|---|---|
| AG-1 | Creare, modificare, duplicare ed eliminare agenti | M |
| AG-2 | Avviare un agente su un compito e seguirne l'attività passo per passo (messaggi, strumenti usati, file toccati) | M |
| AG-3 | Mettere in pausa, fermare e riprendere un agente | M |
| AG-4 | Vista d'insieme di tutte le sessioni in corso e concluse | M |
| AG-5 | Esecuzione di comandi nel terminale, soggetta a permessi | M |
| AG-6 | Più agenti in parallelo (con i modelli locali, nei limiti della memoria disponibile) | S |
| AG-7 | Supporto a MCP (Model Context Protocol) per aggiungere strumenti esterni | S |
| AG-8 | Agenti che delegano parti del lavoro ad altri agenti | F |

## 6. Permessi e sicurezza

| Codice | Requisito | Pr. |
|---|---|---|
| SI-1 | Ogni strumento che modifica qualcosa (scrittura file, comandi, rete) chiede conferma, salvo regole impostate dall'utente | M |
| SI-2 | Livelli di permesso per conversazione o agente: chiedi sempre / consenti modifiche ai file / consenti tutto nella cartella | M |
| SI-3 | L'accesso ai file è limitato alle cartelle concesse | M |
| SI-4 | L'API locale ascolta solo su `127.0.0.1` ed è protetta da un token | M |
| SI-5 | Il contenuto di pagine web e file è trattato come dato, non come istruzione: non può concedersi permessi da solo | M |
| SI-6 | Registro consultabile delle azioni compiute dagli strumenti | S |
| SI-7 | Un testo letto (file, pagina) non può far uscire i dati dei file concessi verso un sito scelto dal modello senza una conferma; protezione accesa di base, disattivabile | S |
| SI-8 | I collegamenti nelle risposte si aprono nel browser di sistema, mai nella finestra dell'app, mostrando prima l'indirizzo vero (conferma disattivabile) | S |
| SI-9 | Le protezioni che chiedono conferme in più sono accese di base e l'utente può spegnerle nelle Impostazioni | S |

## 7. Integrazione con VS Code

| Codice | Requisito | Pr. |
|---|---|---|
| VS-1 | Il nucleo espone un'API locale compatibile con OpenAI, così gli strumenti esistenti possono già usarne i modelli | M |
| VS-2 | Estensione con pannello chat che usa modelli, agenti e impostazioni dell'app | S |
| VS-3 | L'estensione passa al modello il file aperto e la selezione | S |
| VS-4 | Le modifiche proposte appaiono come diff da accettare o rifiutare | S |
| VS-5 | L'estensione funziona anche con l'app chiusa, avviando il nucleo senza interfaccia | S |
| VS-6 | Completamento del codice in linea | F |

## 8. Requisiti generali

| Codice | Requisito | Pr. |
|---|---|---|
| GE-1 | Windows 10/11 a 64 bit come piattaforma principale | M |
| GE-2 | Niente che dipenda da Windows nel nucleo, salvo dietro un'interfaccia sostituibile, per arrivare poi a Linux e macOS | M |
| GE-3 | Tutti i dati restano sul PC; nessuna telemetria | M |
| GE-4 | L'app a riposo, senza modelli caricati, usa meno di 200 MB di RAM | S |
| GE-5 | Interfaccia in italiano e inglese | S |
| GE-6 | Tema chiaro e scuro | S |
| GE-7 | Installer e aggiornamento automatico | S |

## Fuori dal perimetro

- Addestramento o fine-tuning di modelli.
- Formati diversi da GGUF (safetensors, ONNX, MLX…).
- Generazione di immagini, audio e video.
- Account, sincronizzazione cloud, uso da più utenti.
- App per telefono.

## Macchina di riferimento

Lo sviluppo e le prove partono da un PC di fascia media: CPU a 10 core, GPU NVIDIA con
12 GB di VRAM, 16 GB di RAM. I 16 GB di RAM sono il motivo per cui LO-5 e GE-4 contano tanto.
