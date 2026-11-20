# 0021 — Ultimi lavori per la 1.0.0

**Stato:** accettata e realizzata (20-11-2026). Tutto coperto dai test automatici; le prove
dal vivo (chiave Anthropic vera, server MCP pubblicati, primo avvio su un PC vuoto) restano
quelle elencate in [piano-1.0.0.md](../piano-1.0.0.md).

## Contesto

Il [piano per la 1.0.0](../piano-1.0.0.md) elencava, oltre alle prove dal vivo, lavori ancora
da fare: agenti più robusti con i modelli piccoli, prompt caching con Anthropic, MCP via HTTP
con le risorse e un catalogo di server noti, primo avvio guidato, accessibilità, prove
dell'interfaccia e del guscio Tauri, divisione di `lib.rs`, preparazione della firma
Authenticode e guida per l'utente.

## Decisioni

**Percorsi relativi per gli strumenti sui file.** Erano già accettati, ma il modello non lo
sapeva e copiava i percorsi completi. Ora il preambolo chiama la prima cartella concessa
*cartella di lavoro* e chiede percorsi relativi brevi; la descrizione del parametro `path` dice
lo stesso; `list_folder`, `write_file` ed `edit_file` rispondono con percorsi relativi, così il
modello continua a usarli. Un percorso come `\src\a.txt`, senza unità, è relativo alla
cartella di lavoro (prima finiva alla radice del disco e veniva rifiutato).

**Limite di token per passo** (`Agent.max_step_tokens`, 8192 di base, da 256 a 131.072).
Nelle sessioni degli agenti il limite di token della risposta non supera mai questo valore.
Un passo troncato non ferma più la sessione: le chiamate a metà si scartano e il modello
riceve, solo per il passo successivo e senza salvarlo, l'invito a riprovare in breve (percorsi
relativi, file lunghi in più parti). Dopo due tentativi di seguito la sessione si ferma con
l'avviso di risposta troncata. In chat nulla cambia.

**Prompt caching con Anthropic.** Due punti di cache: uno fisso alla fine del prompt di
sistema (copre anche gli strumenti, che vengono prima) e quello automatico (`cache_control`
nella richiesta) che segue la conversazione mentre cresce. Il secondo va con le altre
"comodità" della richiesta: se il servizio rifiuta (400) la richiesta essenziale tiene solo il
primo. Il costo stimato conta i token letti dalla cache a 0,1 volte il prezzo e quelli scritti
a 1,25 volte (`GenerationStats.cache_read_tokens` e `cache_write_tokens`). Per alcuni modelli
recenti la lettura costa ancora meno: la stima resta prudente.

**MCP via HTTP** ("streamable HTTP", ADR 0009). `McpServerConfig.transport` è `stdio` (come
prima, il valore di base) o `http`, con `url` e `headers`. Ogni messaggio è un `POST`; la
risposta può essere JSON o un flusso di eventi (SSE), dentro il quale l'app risponde ai `ping`
del server. `Mcp-Session-Id` e `MCP-Protocol-Version` si ripetono come chiede il protocollo.
Gli indirizzi devono essere `https://`, salvo i server su questo computer. I valori delle
intestazioni (di solito `Authorization`) vanno nel Gestore credenziali come le variabili
d'ambiente, con il prefisso `header:` nel nome. OAuth non è gestito: si usa un token.

**Risorse MCP.** Se il server le dichiara, il modello riceve due strumenti in più per server,
`list_resources` e `read_resource`, con gli stessi permessi degli altri strumenti MCP. Il
contenuto binario viene solo segnalato.

**Catalogo di server noti** nell'interfaccia (Filesystem, Memory, Sequential thinking, Fetch,
Git, Playwright, GitHub remoto): un clic riempie il modulo e dice che cosa completare. Sta
nell'interfaccia e non nel nucleo: sono solo valori iniziali.

**Primo avvio guidato.** Compare solo se `Preferences.onboarded` è falso e l'app è vuota
(nessun motore, modello o provider): chi arriva dalla 0.1.0 non lo vede. Mostra GPU e RAM,
installa la variante di llama.cpp consigliata dal nucleo e propone un Qwen2.5 Instruct Q4_K_M
scelto in base alla VRAM (con una GPU NVIDIA) o a metà della RAM che resta oltre 6 GB; il
download continua anche chiudendo la guida. In alternativa mostra la sezione dei provider
cloud. Si ripete dalla tavolozza dei comandi.

**Accessibilità.** Collegamento "Vai al contenuto", navigazione con nome, titolo della finestra
e fuoco sul contenuto a ogni cambio di pagina; i messaggi della chat e i passi degli agenti
sono un `log` annunciato a risposta finita (`aria-busy` durante lo streaming); errori con
`role="alert"`; contatori (VRAM, RAM, contesto) con `role="meter"`; etichette ai campi che ne
erano privi; `aria-expanded` sulle schede dei modelli; `F2` per rinominare una conversazione;
movimento ridotto se il sistema lo chiede. Una prova controlla che i colori dei due temi
rispettino i contrasti WCAG AA (4,5:1 per il testo, 3:1 per il fuoco).

**Prove dell'interfaccia.** Con vitest e happy-dom, al posto del nucleo un finto `invoke`:
chat con risposta in streaming e richiesta di permesso, caricamento di un modello, sessione di
un agente in attesa di conferma. Più del pilotaggio dell'app vera via CDP (usato nelle prove a
mano) perché gira in pochi secondi nei controlli automatici, senza modelli né GPU.

**Prove del guscio Tauri.** Le parti con logica propria sono diventate funzioni provate: quali
indirizzi si aprono nel browser, quali pagine può mostrare la finestra, la forma degli eventi
mandati all'interfaccia, il menu dell'area di notifica. Una prova legge `api.ts` e controlla
che ogni comando chiamato dall'interfaccia sia registrato.

**Divisione di `lib.rs`.** Tipi, `Core` e costruttore restano in `lib.rs` (meno di 600
righe); i metodi stanno in `src/ops/`, un blocco `impl Core` per argomento: `library`,
`engine`, `diagnostics`, `settings`, `conversation`, `rounds` (il ciclo modello → strumenti),
`mcp_servers`, `sessions`, `documents`, `providers`. Le prove sono in `src/tests.rs`.
L'interfaccia pubblica del nucleo non cambia.

**Firma Authenticode, pronta da accendere.** `scripts/sign.ps1` firma un eseguibile con
SignPath (variabili `SIGNPATH_*`) o con un certificato installato (`WINDOWS_CERT_THUMBPRINT`).
`release.ps1` lo usa per `app-server.exe` e lo passa a Tauri come `signCommand`: app e
installatore si firmano **prima** della firma degli aggiornamenti, che altrimenti non
corrisponderebbe più al file. Senza variabili la build continua con un avviso. Il certificato
va chiesto dal titolare del progetto.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Rifiutare i percorsi assoluti | Servono per le cartelle concesse oltre la prima |
| Fermare la sessione al primo passo troncato | Era il comportamento di prima: con i modelli piccoli l'agente si fermava proprio quando serviva poco per proseguire |
| Salvare l'invito a riprovare nella conversazione | Comparirebbe come messaggio dell'utente, che non l'ha scritto |
| Un solo punto di cache automatico | La parte più costosa (strumenti e prompt di sistema) resterebbe senza un punto sicuro quando la conversazione cambia |
| Il vecchio trasporto HTTP+SSE (due indirizzi) | Sostituito dallo streamable HTTP nel protocollo; i server nuovi offrono quello |
| OAuth per i server remoti | Grande da fare bene; i server diffusi accettano un token nelle intestazioni |
| Catalogo dei server scaricato da internet | Una richiesta in più e un elenco che l'app non controlla |
| Primo avvio anche per chi ha già dei dati | Disturberebbe chi aggiorna dalla 0.1.0 |
| Prove dell'interfaccia via CDP nell'app vera | Lente e bisognose di modelli veri: restano per le prove a mano |
| Firmare con Authenticode dopo la build | Cambierebbe l'installatore dopo la firma degli aggiornamenti |

## Conseguenze

- Gli agenti salvati prima ricevono il limite di 8192 token per passo; con un preset che ne
  chiede di più vale il più basso.
- Il prompt di sistema di Anthropic ora viaggia come blocco di testo con `cache_control`;
  servizi "compatibili Anthropic" di terze parti che non conoscono il campo potrebbero
  rifiutarlo.
- `happy-dom` è una nuova dipendenza di sviluppo dell'interfaccia (solo per le prove).
- Per una correzione nel nucleo si parte dal file di `ops/` con il nome dell'argomento; i
  metodi usati da più file sono `pub(crate)`.
