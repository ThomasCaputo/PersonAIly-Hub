# 0015 — Punti di ripristino per gli agenti

**Stato:** accettata e realizzata (20-11-2026). Provata nell'app con Qwen2.5 7B: l'agente
cambia `a.txt` e crea `nuovo.txt`, "Annulla le modifiche" riporta la cartella com'era,
"Rifai le modifiche" rimette il lavoro dell'agente.

## Contesto

Un agente con il permesso "Consenti tutto" può cambiare molti file in pochi secondi. Il
registro delle azioni dice che cosa ha fatto, ma non permette di tornare indietro. Serve
un "annulla tutto" che funzioni su qualunque cartella, anche fuori da un repository git,
senza toccare il lavoro dell'utente.

## Decisione

- Prima che una sessione lavori per la prima volta, ogni cartella dell'agente viene
  **fotografata con git in un repository separato**, in
  `<cartella dati>/snapshots/<hash della cartella>.git`, usando la cartella come
  `--work-tree`. Il repository git dell'utente (se c'è) non viene toccato: né indice,
  né rami, né configurazione.
- Valgono le regole `.gitignore` della cartella; in più non si fotografano mai
  `node_modules`, `target`, `.venv`, `__pycache__`, `.gradle` e i `.gguf`, né i file oltre
  50 MB.
- Ogni foto è un commit appeso a un riferimento (`refs/sessions/<id>`), così git non la
  cancella facendo pulizia. Le foto della stessa cartella condividono i file uguali.
- **Ripristinare** (`git read-tree -u --reset <foto>`) rimette i file cambiati e quelli
  cancellati e toglie quelli creati dopo; i file ignorati restano come sono. Prima si
  scatta un'altra foto, che diventa il nuovo punto di ripristino: ripetere il comando
  annulla il ripristino. Il ripristino finisce nel registro delle azioni.
- Riprendere una sessione non cambia il punto di ripristino: resta quello di prima che
  l'agente lavorasse. Eliminare una sessione dimentica le sue foto.
- L'opzione è per agente ("Punto di ripristino prima di ogni sessione", accesa di base).
  Senza git installato non c'è: l'editor dell'agente lo dice, e l'agente lavora lo stesso.
  Una cartella che non si riesce a fotografare non ferma l'agente.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Commit nel repository dell'utente | Sporca la sua cronologia e non funziona fuori da git |
| Copia della cartella | Lenta e pesante per cartelle grandi; nessuna condivisione tra foto |
| `git stash` | Tocca l'indice e lo stato del repository dell'utente |

## Conseguenze

- Serve `git` nel `PATH`. Le cartelle con sotto-repository git li fotografano come
  riferimento al commit, non come contenuto.
- La prima foto di una cartella grande occupa spazio quanto i suoi file (compressi).
