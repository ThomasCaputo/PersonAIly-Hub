# Guida per l'utente

Questa guida spiega come usare PersonAIly Hub, dall'installazione al primo agente.
La versione inglese è in [user-guide.md](user-guide.md).

- [Installazione](#installazione)
- [Primo avvio](#primo-avvio)
- [Modelli locali](#modelli-locali)
- [Modelli cloud](#modelli-cloud)
- [Chat](#chat)
- [Agenti e permessi](#agenti-e-permessi)
- [Server MCP](#server-mcp)
- [Skill](#skill)
- [Plugin](#plugin)
- [Documenti consultabili](#documenti-consultabili)
- [VS Code](#vs-code)
- [Backup, aggiornamenti e segnalazioni](#backup-aggiornamenti-e-segnalazioni)
- [Tastiera e accessibilità](#tastiera-e-accessibilità)
- [Problemi frequenti](#problemi-frequenti)

## Installazione

PersonAIly Hub 1.0 funziona su **Windows 10 e 11 a 64 bit**. Dalla pagina
[Releases](https://github.com/ThomasCaputo/personaily-hub/releases) scarica:

- `PersonAIly-Hub_<versione>_x64-setup.exe`, l'installatore (consigliato): non chiede i
  permessi di amministratore, aggiunge l'app al menu Start e riceve gli aggiornamenti;
- oppure `PersonAIly-Hub_<versione>_x64-portable.exe`, un solo file da avviare dove vuoi,
  senza installazione (gli aggiornamenti si scaricano a mano).

Se Windows SmartScreen avvisa che l'app è sconosciuta, scegli **Ulteriori informazioni →
Esegui comunque**, oppure controlla prima il file con `SHA256SUMS.txt`.

## Primo avvio

Alla prima apertura una breve configurazione guidata:

1. mostra la scheda video e la RAM del computer;
2. ti fa scegliere tra **un modello sul tuo computer** (gratuito e privato) e **un servizio
   cloud** (con la tua chiave API);
3. per il modello locale installa la versione giusta di llama.cpp (CUDA con una GPU NVIDIA,
   Vulkan con AMD e Intel) e propone un modello adatto alla memoria, da scaricare con un clic.

Puoi saltarla e farla più tardi: `Ctrl+K` → **Configurazione guidata del primo avvio**.

## Modelli locali

Tutto si fa nella pagina **Modelli**.

- **Motore:** llama.cpp, il programma che esegue i modelli. L'app lo scarica e lo
  aggiorna; puoi tenere più varianti (CUDA, Vulkan, solo processore) e scegliere quale usare.
- **Cartelle:** le cartelle in cui tieni i file `.gguf`. Quelli scaricati dall'app finiscono
  in una di queste.
- **Scarica un modello:** cerca su Hugging Face, scegli una quantizzazione (Q4_K_M è un buon
  compromesso) e scarica. I download si possono interrompere e riprendere.
- **Caricare un modello:** apri la sua scheda, scegli i parametri e premi **Carica**.

**Dove sta il modello (collocazione).** Il modello e la sua memoria di lavoro (cache KV)
stanno **o tutti nella VRAM** (veloce) **o tutti nella RAM** (più lento, ma la RAM di solito è
di più). Prima di caricare l'app stima quanta memoria serve e ti avvisa se non basta. Per i
modelli troppo grandi per la VRAM c'è la collocazione **ibrida**: scegli quanti layer mettere
sulla GPU e il resto va in RAM.

**Contesto.** È quanta conversazione il modello riesce a leggere. Più è lungo, più memoria
serve. Sotto la chat una barra mostra quanto è pieno.

**Immagini.** Alcuni modelli leggono le immagini grazie a un file `mmproj`: scaricalo insieme
al modello e, al caricamento, spunta il componente visivo.

**Scaricamento automatico.** In Modelli → Motore puoi far scaricare il modello dopo qualche
minuto senza uso, per liberare la memoria.

**Più modelli insieme.** Accendi **Più modelli locali insieme** in Modelli → Motore: da quel
momento **Carica** aggiunge il modello accanto a quelli già caricati, finché entra nella memoria
del tuo PC. L'app tiene sempre libero un margine per il sistema (il 5% della VRAM e il 10% della
RAM, con un minimo), quindi su un PC con più memoria ne entrano di più. Se vuoi un tetto,
scrivilo in **Al massimo**. Se un modello non entra, la sua scheda offre **Scarica gli altri e
carica**.

In **Modelli caricati** vedi ogni modello con la sua memoria. Uno è il **principale**: è quello
usato dove non ne scegli uno (le chat con «Il principale», gli agenti creati prima, `local`
nell'API, VS Code); di solito è l'ultimo che hai caricato, e puoi cambiarlo con **Rendi
principale**. In chat, nel confronto e negli agenti il selettore del modello elenca ogni modello
caricato per nome.

## Modelli cloud

In **Modelli → Provider cloud** aggiungi un servizio: Anthropic, oppure uno compatibile con
OpenAI (OpenAI, OpenRouter, Groq, Mistral, Google AI Studio…). Incolla la chiave API e
premi **Aggiorna modelli**.

- La chiave sta nel **Gestore credenziali di Windows**, mai in un file.
- Con i modelli di cui si conosce il prezzo l'app stima il costo di ogni risposta. Con
  Anthropic usa la memoria temporanea del servizio (prompt caching): le conversazioni lunghe
  e gli agenti costano molto meno.
- In **Impostazioni → Spesa cloud** puoi fissare un limite al mese e per conversazione.
- **Modelli di riserva:** una coda di modelli cloud da provare, in ordine, quando quello scelto
  dà un errore (per esempio "modello occupato" o "troppe richieste" con i piani gratuiti). La
  risposta riparte dal primo modello della coda che funziona; sopra la risposta un avviso dice
  chi ha risposto al posto di chi, e ogni messaggio porta il nome del suo modello. In
  **Impostazioni → Modelli di riserva** prepari la coda predefinita; ogni chat (menu "Riserva"
  sopra i messaggi) e ogni agente con un modello cloud (nel suo modulo) la segue, oppure ne ha
  una propria, che parte come copia di quella predefinita. Non vale nel confronto tra modelli.

## Chat

- Scegli il modello in alto; puoi cambiarlo a metà conversazione.
- **Invio** manda il messaggio, **Maiusc+Invio** va a capo, **Interrompi** ferma la risposta.
- **Allegati:** 📎 oppure trascina i file nella finestra (testo, PDF, immagini).
- **Modifica** un tuo messaggio o **Rigenera** l'ultima risposta: la versione vecchia resta e
  la ritrovi con le frecce ‹ ›.
- **Strumenti** (sopra i messaggi): file delle cartelle concesse, ricerca web, documenti.
  Il livello di permesso decide che cosa il modello può fare senza chiedere.
- **Esporta** salva la conversazione in Markdown (o in JSON, con tutti i rami).
- Il pannello a destra contiene i parametri (temperatura, top-p, contesto…) e il prompt di
  sistema, salvati per ogni modello. I preset con nome si esportano e importano.
- **Confronto:** la pagina omonima manda lo stesso messaggio a due modelli affiancati. Le due conversazioni sono temporanee: con «Salva come conversazione» (o «Salva entrambe») le tieni nella chat, le altre si cancellano con «Nuovo confronto» o chiudendo l'app.

## Agenti e permessi

Un **agente** è un assistente che lavora da solo su un compito: legge e scrive i file delle
sue cartelle, usa il terminale (PowerShell) e gli strumenti dei server MCP.

1. In **Agenti → Agenti** creane uno: nome, modello, istruzioni, cartelle di lavoro,
   strumenti e livello di permesso.
2. In **Sessioni → Nuovo compito** scegli l'agente e scrivi il compito.
3. Segui ogni passo: messaggi, strumenti usati, file toccati. Puoi mettere in **pausa**,
   **fermare** o rispondere all'agente per farlo continuare.

**Permessi.** Tre livelli:

| Livello | Senza chiedere | Chiede conferma |
|---|---|---|
| Chiedi sempre | leggere | scrivere, modificare, comandi |
| Consenti modifiche ai file | leggere, scrivere, modificare | comandi |
| Consenti tutto | tutto | — |

Quando l'agente chiede conferma vedi esattamente che cosa farebbe: **Consenti** (una volta),
**Consenti sempre in questa sessione** o **Nega**. Fuori dalle cartelle concesse non si va,
a nessun livello.

**Punto di ripristino.** Prima di ogni sessione l'app fotografa le cartelle dell'agente (serve
git installato): con **Annulla le modifiche** le riporti com'erano.

**Limiti.** *Passi massimi* ferma l'agente dopo un certo numero di giri; *token massimi per
passo* interrompe un passo troppo lungo (succede con i modelli locali piccoli) e fa riprovare
l'agente in breve.

**Deleghe.** Un agente può affidare parti del compito ad altri agenti scelti da te; ognuno
lavora con le sue cartelle e i suoi permessi.

**Agenti che cooperano.** Ogni agente può avere un **suo modello locale**: sceglilo nel
selettore del modello, anche fra quelli non ancora caricati («Da caricare quando parte»).
Quando l'agente parte, l'app carica il suo modello se non c'è, purché entri accanto agli altri.
Così puoi avere, per esempio, un coordinatore con Qwen2.5 7B che affida pezzi del lavoro ad
aiutanti con un modello più piccolo. Se il coordinatore affida più pezzi nello stesso turno, gli
aiutanti lavorano **insieme** e i resoconti tornano tutti insieme. Gli aiutanti devono avere un
modello che sa usare gli strumenti (per esempio Qwen2.5): l'app avvisa se il modello non li
supporta.

### Agenti pianificati

Un agente può lavorare da solo a un orario o a intervalli: per esempio ogni mattina alle 8, o
ogni 6 ore. In **Agenti → Pianificazioni** (o con **Pianifica** sulla scheda dell'agente)
scegli l'agente, scrivi il compito per intero e decidi quando: una volta, ogni giorno (anche
solo in certi giorni) oppure ogni tot ore.

- **Modello locale.** Se l'agente usa un modello locale, scegli quale: si carica all'ora
  stabilita e si scarica a lavoro finito, se l'aveva caricato la pianificazione e nessun altro
  l'ha usato nel frattempo. Se c'è già caricato un altro modello, la pianificazione **aspetta**
  che tu lo scarichi, oppure lo **sostituisce** appena nessuno lo sta usando, come preferisci.
  Dopo un'ora di attesa salta.
- **Nessuno a confermare.** Mentre lavora da solo, l'agente non può chiederti niente: ciò che
  chiederebbe conferma viene **rifiutato**. Il modulo ti dice quali azioni verranno rifiutate
  con i permessi attuali dell'agente; per consentirle, cambia i permessi dell'agente.
  Rispondendo all'agente in una sessione pianificata, le conferme tornano a chiedersi.
- **Esito.** Ogni esecuzione è una sessione normale, con il segno "pianificata": la trovi in
  **Sessioni** con passi, file toccati e punto di ripristino. Una notifica di Windows ti dice
  com'è andata (sempre, solo se qualcosa va storto, o mai). **Esegui ora** prova il compito
  subito, con le stesse regole.
- **App chiusa o PC spento.** Le pianificazioni girano finché l'app è accesa, anche solo
  nell'area di notifica. Con **Avvia con Windows** l'app parte da sola all'accesso, senza
  finestra. Un'esecuzione persa si recupera una volta appena l'app si riaccende, oppure si
  salta, a tua scelta; il PC non viene risvegliato dalla sospensione.

## Server MCP

I server [MCP](https://modelcontextprotocol.io) aggiungono strumenti alle chat e agli agenti
(GitHub, browser, database…). Si configurano in **Impostazioni → Server MCP**:

- **Server noti:** un elenco di server diffusi da aggiungere con un clic (alcuni chiedono di
  completare un percorso o un token);
- **Programma su questo computer (stdio):** un comando come `npx -y @nome/server`;
- **Server a un indirizzo (HTTP):** l'indirizzo `https://…` di un server remoto. Per farti
  riconoscere scegli:
  - **Intestazioni**, con un token scritto da te (di solito `Authorization: Bearer <token>`);
  - **Accesso dal browser (OAuth)**, per i servizi come Notion o Linear: salva, premi
    **Accedi**, poi **Apri nel browser** e completa l'accesso nella pagina del servizio. L'app
    rinnova il token da sola; **Esci** lo cancella. I servizi che non permettono all'app di
    registrarsi da sola (per esempio GitHub) chiedono di registrare un'app OAuth tua, con
    indirizzo di ritorno `http://127.0.0.1/callback`, e di scriverne il Client ID nel modulo.

Variabili d'ambiente, intestazioni e token di accesso stanno nel Gestore credenziali. **Prova** avvia il
server e mostra i suoi strumenti. Poi, nell'editor dell'agente, spunta i server da usare. Se il
server offre delle risorse (documenti, file), l'agente le può elencare e leggere. Gli strumenti
MCP chiedono sempre conferma, salvo con «Consenti tutto».

**In chat:** apri il menu **Server MCP** nella barra degli strumenti sopra i messaggi e spunta
i server di quella conversazione. Partono con il messaggio successivo e restano accesi mentre
chatti; dopo 10 minuti senza messaggi si chiudono e ripartono quando servono. In chat ogni
strumento MCP chiede conferma finché non scegli **Consenti sempre in questa conversazione** per
quello strumento: «Consenti tutto nelle cartelle» non li copre.

**Comandi `/`:** se i server della conversazione offrono dei prompt (testi pronti, a volte con
argomenti), scrivi `/` all'inizio del messaggio: compare l'elenco, che si restringe mentre
scrivi. Scegli con le frecce e Invio (o con un clic), compila gli eventuali argomenti e il
testo del prompt finisce nella casella del messaggio, da rivedere e inviare. Se il server sa
suggerire i valori di un argomento, mentre scrivi compare un elenco sotto la casella: scegli con
le frecce e Invio (o con un clic), oppure continua a scrivere.

## Skill

Una skill è un insieme di istruzioni pronte per un certo compito (scrivere un verbale, rivedere
un testo con le tue regole…), nel formato aperto `SKILL.md`, lo stesso di Claude: le skill
già scritte per altri programmi funzionano anche qui. Si gestiscono in **Impostazioni → Skill**:

- **Nuova skill:** scrivi nome (lettere minuscole, cifre e trattini), quando usarla e le
  istruzioni;
- **Importa una cartella** o **Importa un .zip:** una skill con il suo `SKILL.md` ed eventuali
  file di supporto (modelli, esempi);
- la casella accanto a ogni skill la accende o la spegne; **Modifica** cambia lo `SKILL.md`.

Poi, in chat o negli agenti, accendi lo strumento **Skill**: il modello vede solo nome e
descrizione delle skill accese e, quando la richiesta corrisponde, carica le istruzioni e i
file che gli servono. Leggere una skill non chiede conferma; tutto il resto (file, web,
terminale) segue i permessi di sempre. Importa solo skill di cui ti fidi: il modello segue
le loro istruzioni come se le avessi scritte tu. Le skill entrano nei backup.

## Plugin

Un plugin è un pacchetto con skill, server MCP, agenti e comandi `/`, nel formato dei plugin di
Claude Code: quelli già pubblicati si installano così come sono. In **Impostazioni → Plugin**
scegli **Importa una cartella** o **Importa un .zip**. L'app copia il plugin, crea le sue parti e
mostra gli avvisi, per esempio le variabili di un server da completare o le parti che non supporta
(come gli *hooks*).

- Le **skill** compaiono tra le skill, accese.
- I **server MCP** compaiono tra i server: nessuno parte finché non lo scegli in una conversazione
  o in un agente.
- Gli **agenti** compaiono nella pagina Agenti, senza cartelle: sceglile prima di usarli.
- I **comandi** compaiono nel menu `/` di ogni conversazione: scegline uno, scrivi gli eventuali
  argomenti e il testo finisce nella casella del messaggio.

Skill e server di un plugin portano il segno «plugin …» e si tolgono solo con **Disinstalla**,
che elimina tutto ciò che il plugin aveva creato. Reinstallare un plugin già presente lo sostituisce:
le modifiche fatte ai suoi server e agenti si perdono. Un plugin si fida come un programma:
installa solo quelli di cui ti fidi. I plugin entrano nei backup.

## Documenti consultabili

In **Impostazioni → Documenti consultabili** scegli un modello di embedding (per esempio
nomic-embed-text o multilingual-e5, in formato GGUF) e le cartelle da indicizzare (tra
quelle concesse). Poi, in chat o negli agenti, accendi lo strumento **Documenti**: il modello
cerca i brani più vicini alla domanda. Tutto resta sul tuo computer.

## VS Code

Installa `personaily-hub-vscode_<versione>.vsix` (in VS Code: **Estensioni → ⋯ → Installa da
VSIX…**). L'estensione usa l'app, o da sola il nucleo senza finestra (`app-server`), se l'app è
chiusa.

- Pannello chat con le conversazioni dell'app e il file aperto come contesto.
- `Ctrl+Alt+I` chiede una modifica alla selezione: arriva come diff da accettare o rifiutare.
- Agenti da avviare e seguire, con le conferme come notifiche.
- Completamento del codice in linea con un modello locale per il codice (per esempio
  Qwen2.5-Coder), spento di base.

Indirizzo e token dell'API locale sono in **Impostazioni → API locale**: servono anche ad
altri programmi compatibili con OpenAI.

## Backup, aggiornamenti e segnalazioni

- **Impostazioni → Backup dei dati:** salva conversazioni, agenti, impostazioni, preset, skill e plugin in un
  `.zip`; **Ripristina da un backup** lo rimette al riavvio. Le chiavi API restano nel Gestore credenziali.
- **Impostazioni → Aggiornamenti:** l'app controlla all'avvio se c'è una versione nuova, la
  scarica, ne verifica la firma e la installa. Il controllo si può spegnere.
- **Impostazioni → Segnalare un problema:** **Copia le informazioni diagnostiche** prepara un
  testo da allegare a una segnalazione, senza chiavi, conversazioni né nome utente.
- I dati stanno in `%APPDATA%\dev.personaily.hub`.

## Tastiera e accessibilità

- `Ctrl+K` apre la tavolozza dei comandi: pagine, nuova conversazione, esportazione, tema,
  ricerca nelle conversazioni.
- `Tab` all'apertura porta al collegamento **Vai al contenuto**.
- Nella lista delle conversazioni `F2` rinomina quella selezionata.
- Nelle richieste di conferma i pulsanti si raggiungono con `Tab`; `Esc` chiude tavolozza e
  configurazione guidata.
- I lettori di schermo annunciano la risposta del modello quando è completa, gli errori e le
  richieste di permesso. Il tema chiaro e quello scuro rispettano i contrasti WCAG AA.

## Problemi frequenti

**Il modello non entra in memoria.** Riduci il contesto, scegli la cache KV `q8_0` (già
predefinita), una quantizzazione più piccola o la collocazione in RAM / ibrida.

**Il modello è lento.** Se è in RAM è normale; in VRAM, controlla l'avviso "sconfina nella
RAM": vuol dire che la VRAM non basta e il driver sta usando la memoria di sistema.

**Un agente locale si incastra.** Con modelli piccoli succede che ripeta lo stesso percorso
per minuti: il limite di token per passo lo ferma. Aiuta anche dare all'agente una sola
cartella e un compito preciso.

**SmartScreen avvisa a ogni installazione.** Gli eseguibili non sono ancora firmati con un
certificato Authenticode; la firma degli aggiornamenti invece è sempre controllata.
