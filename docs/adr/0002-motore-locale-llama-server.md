# 0002 — Modelli locali tramite `llama-server` come processo separato

**Stato:** accettata

## Contesto

I modelli locali sono file GGUF, il formato di llama.cpp. llama.cpp si può usare in due
modi: collegandolo come libreria dentro il nostro programma, oppure avviando il suo
eseguibile `llama-server`, che espone un'API HTTP compatibile con OpenAI.

## Decisione

Il nucleo avvia `llama-server` come **processo figlio**, uno per modello caricato, e gli
parla via HTTP su `127.0.0.1` con una porta scelta al momento. I binari ufficiali di
llama.cpp vengono scaricati dall'app, nella variante adatta all'hardware (CUDA, Vulkan,
solo CPU).

## Motivi

- **Liberazione certa della memoria:** terminando il processo, VRAM e RAM tornano al
  sistema senza residui. È ciò che serve per l'ADR 0003.
- **Isolamento:** se llama.cpp va in crash (modello corrotto, memoria esaurita), l'app
  resta in piedi e può mostrare l'errore.
- **Aggiornamenti semplici:** llama.cpp esce molto spesso; basta sostituire i binari,
  senza ricompilare l'app.
- **Nessuna compilazione C++/CUDA** nel nostro progetto: build più semplice e più rapida.
- **Stessa forma dei provider cloud:** il provider locale è quasi identico a quello
  "compatibile OpenAI".

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| llama.cpp collegato come libreria (binding Rust) | Controllo più fine, ma build complessa (CUDA), crash che abbattono l'app, aggiornamenti legati ai nostri rilasci |
| Appoggiarsi a Ollama | Aggiunge un programma da installare e nasconde proprio i parametri che vogliamo esporre (collocazione della memoria, tipo di cache KV) |

## Conseguenze

- Possiamo controllare solo ciò che `llama-server` espone con argomenti e API. Oggi copre
  tutto quanto richiesto.
- Nomi e comportamento degli argomenti possono cambiare tra versioni: l'app fissa una
  versione provata di llama.cpp e la costruzione degli argomenti sta in un unico modulo
  (`local`), coperto da test.
- Il primo avvio richiede di scaricare i binari (alcune centinaia di MB per la variante CUDA).
- Inoltre `llama-server` viene legato alla vita dell'app con un "job" di Windows: se l'app
  va in crash o viene terminata, il sistema chiude anche lui e la memoria si libera.

## Versione in uso

Build **b12110**, che corrisponde alla versione stabile **v0.9.0** di llama.cpp. Da
`github.com/ggml-org/llama.cpp/releases/tag/b12110` servono due archivi, estratti nella
stessa cartella `bin\llama.cpp\b12110-cuda\` dentro la cartella dati dell'app:

- `llama-b12110-bin-win-cuda-13.4-x64.zip` — il programma;
- `cudart-llama-bin-win-cuda-13.4-x64.zip` — le librerie CUDA di NVIDIA.

Attenzione allo schema delle release: la release "latest" (`v0.9.0`) contiene solo un
file di testo con il nome della build; i binari stanno nelle release `bNNNNN`.

Per ora il download si fa a mano; quello dall'app è previsto (requisito LO-9).
