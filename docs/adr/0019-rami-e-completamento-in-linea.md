# 0019 — Rami della conversazione e completamento del codice in linea

**Stato:** accettata e realizzata (20-11-2026). Rami provati nell'app; completamento in
linea provato solo sull'API (con Qwen2.5 7B Instruct, che non è un modello FIM: la
risposta arriva, ma il testo non serve). Manca una prova in VS Code con un modello per il
codice.

## Rami della conversazione

- Ogni messaggio ricorda il messaggio a cui risponde (`parent_id`); `active` segna il
  ramo che si vede e che va al modello. Lungo un ramo gli id crescono sempre (un messaggio
  nasce dopo il suo genitore), quindi tutte le letture esistenti restano `ORDER BY id`
  più `active = 1`.
- **Modificare** un messaggio (CH-8) non cancella più ciò che segue: lo mette da parte e
  il messaggio nuovo diventa un'altra versione di quello vecchio. **Rigenerare** tiene il
  messaggio e mette da parte solo la risposta.
- Le frecce "‹ 2 di 3 ›" sotto un messaggio passano da una versione all'altra; scendendo
  si segue ogni volta la versione più recente.
- L'esportazione in **JSON** contiene tutti i rami (`parentId`, `active`); quella in
  Markdown il ramo che si vede. Il costo di una conversazione per il limite di spesa
  conta anche i rami messi da parte: sono soldi spesi.

## Completamento in linea (VS-6)

- Il nucleo espone `POST /api/infill` (`prefix`, `suffix`, `maxTokens`): lo passa
  all'endpoint `/infill` di `llama-server` con il modello locale caricato. Serve un
  modello addestrato al "fill-in-the-middle" (Qwen2.5-Coder e simili); se il modello non
  ha i token FIM l'errore lo dice.
- L'estensione VS Code registra un provider di completamenti in linea, **spento di
  base** (`personailyHub.inlineCompletion`): aspetta 300 ms dopo l'ultimo tasto
  (`inlineCompletionDelay`), manda 6000 caratteri prima e 2000 dopo il cursore, toglie
  dalla proposta ciò che c'è già dopo il cursore. Continuando a scrivere la richiesta si
  chiude e `llama-server` smette di generare.
- Solo modelli locali: con un modello cloud ogni tasto costerebbe una richiesta a pagamento.
