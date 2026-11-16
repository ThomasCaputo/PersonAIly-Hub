# 0014 — Modelli locali: varianti di llama.cpp, immagini, modelli divisi, download

**Stato:** accettata e realizzata (16-11-2026). Provati dal vivo la scelta della variante,
lo scaricamento per inattività e l'elenco dei file di Hugging Face (modelli divisi e
componenti visivi riconosciuti su repository veri). Le immagini con un modello locale e
il download di un modello diviso sono provati solo dai test automatici.

## Contesto

Restavano dalle Tappe 1–3: scegliere a mano la variante di llama.cpp (LO-9), far leggere
le immagini ai modelli locali, scaricare i modelli divisi in più file e più modelli
insieme, scaricare il modello dopo un periodo di inattività (LO-8).

## Decisione

**Varianti.** Si possono installare più varianti (CUDA, Vulkan, solo processore), ognuna
nella sua cartella `bin\llama.cpp\<build>-<variante>`. La variante scelta è
un'impostazione del nucleo (`engine_variant`); installarne una la sceglie. Il prossimo
caricamento usa la build più recente della variante scelta, e in mancanza CUDA, poi
Vulkan, poi solo processore. Una variante in uso dal modello caricato non si elimina.
Il modello caricato annota quale versione lo esegue (`engine`).

**Immagini (componente visivo).** Un file `mmproj-….gguf` nella cartella del modello è
il suo componente visivo; con più candidati si prende quello col nome più simile al
modello. Si carica con `--mmproj` (con `--no-mmproj-offload` in Solo RAM: anche lui resta
nella memoria scelta, ADR 0003) ed entra nella stima della memoria. Si può spegnere per
modello ("Lettura delle immagini", parametro di caricamento `vision`, acceso di base).
Un modello caricato con il componente visivo riceve le immagini come i modelli cloud.

**Download in sottocartelle.** Ogni repository si salva in `<cartella>\<autore>\<nome>`:
i componenti visivi hanno spesso lo stesso nome in repository diversi (`mmproj-F16.gguf`)
e devono stare accanto al loro modello. I modelli scaricati prima restano dove sono.

**Modelli divisi.** Le parti `-00001-of-0000N.gguf` diventano una voce sola, se ci sono
tutte, con la dimensione totale; si scaricano una dopo l'altra, ognuna con il suo
`.part`, la sua impronta e la ripresa. Una parte già completa non si riscarica. In
libreria compare solo la prima parte (llama.cpp trova le altre), con la dimensione di
tutte; se ne manca una il modello è segnalato come non leggibile.

**Più download insieme.** Ogni download ha un identificatore (`<repo>/<file>`) e il suo
segnale di interruzione; lo stesso file non si scarica due volte insieme. I download
conclusi restano in elenco finché l'utente non li toglie.

**Scaricamento per inattività (LO-8).** Preferenza `idle_unload_minutes` (0 = mai, di
base). Il nucleo annota l'ultima volta in cui il modello locale ha lavorato; un compito
di sottofondo (`Core::maintenance`, avviato dall'app e da `app-server`) controlla ogni
30 secondi e scarica il modello se è inattivo da troppo, non si sta caricando e nessuna
risposta o agente è al lavoro. Cambiare la preferenza fa ripartire il conteggio.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Scegliere il componente visivo a mano | Un passo in più per un caso quasi sempre ovvio; resta la casella per spegnerlo |
| Unire le parti di un modello diviso in un file solo | Raddoppia lo spazio durante l'unione; llama.cpp le legge già divise |
| Una coda di download, uno alla volta | La banda raramente è il limite di un singolo download; l'utente decide |

## Conseguenze

- `ModelInfo` ha `mmproj`, `mmprojBytes` e `parts`; `LoadedModel` ha `engine` e
  `vision`; `HubFile` ha `parts` e `kind`.
- Il motore non è più "uno solo": l'interfaccia mostra le versioni installate e quella
  in uso.
