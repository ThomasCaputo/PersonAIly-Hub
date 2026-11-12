# 0006 — Download dei modelli da Hugging Face: file `.part`, ripresa, impronta

**Stato:** accettata e provata (file da 172 MB: download intero, interruzione al 30 % e
ripresa, rifiuto del doppione)

## Contesto

I modelli GGUF pesano da centinaia di megabyte a decine di gigabyte: un download può
durare ore e interrompersi. Un file incompleto o rovinato non deve mai finire in libreria
come se fosse un modello buono (problema rimasto aperto nella Tappa 1).

## Decisione

- **Fonte:** l'API pubblica di Hugging Face, senza account. La ricerca mostra solo i
  repository con file GGUF, dai più scaricati.
- **File `.part`:** il download scrive in `<nome>.gguf.part` accanto alla destinazione. La
  libreria guarda solo i `.gguf`, quindi un download a metà non vi compare.
- **Ripresa:** se un `.part` esiste già, si chiede al sito solo il resto (`Range`). Se il
  sito non riprende e rimanda tutto, si ricomincia da capo.
- **Impronta:** a download finito si confronta lo SHA-256 dell'intero file con quello
  pubblicato dal sito. Solo se coincide il file prende il nome definitivo; altrimenti viene
  eliminato e l'utente è avvisato.
- **Destinazione:** si scrive solo in una delle cartelle che l'utente ha già aggiunto alla
  libreria. Nomi di repository e di file vengono controllati, perché finiscono in un
  indirizzo e in un percorso su disco.
- **Niente doppioni:** se il file è già nella cartella, il download viene rifiutato.
- **In sottofondo:** il download vive nel nucleo, non nella pagina: prosegue cambiando
  pagina e l'interfaccia ne rilegge lo stato una volta al secondo.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Scrivere direttamente nel file `.gguf` | Un download interrotto comparirebbe in libreria come modello normale |
| Fidarsi della sola dimensione | Non rileva un file rovinato della lunghezza giusta |
| Usare il programma `huggingface-cli` | Richiederebbe Python sul computer dell'utente |

## Conseguenze

- Un download per volta.
- I modelli divisi in più file (`-00001-of-00003`) e i componenti visivi (`mmproj`) non
  vengono proposti: l'app non sa ancora ricomporli.
- I repository che richiedono l'accesso (modelli "gated") non sono scaricabili: servirebbe
  un token dell'utente.
- Riprendendo, la parte già scaricata viene riletta per ricalcolare l'impronta: su file
  molto grandi la ripresa parte dopo qualche secondo.
