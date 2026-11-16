# Decisioni di architettura (ADR)

Ogni file registra **una** decisione tecnica: il contesto, la scelta, le alternative
scartate e le conseguenze. Una decisione non si riscrive: se cambia, si aggiunge un nuovo
ADR che sostituisce il precedente.

| N. | Decisione | Stato |
|---|---|---|
| [0001](0001-stack-tauri-typescript.md) | App desktop con Tauri, React e TypeScript | Accettata |
| [0002](0002-motore-locale-llama-server.md) | Modelli locali tramite `llama-server` come processo separato | Accettata |
| [0003](0003-collocazione-memoria.md) | Modello e cache KV in una sola memoria; KV a `q8_0` | Accettata, misurata e realizzata |
| [0004](0004-nucleo-condiviso-api-locale.md) | Nucleo unico con API locale per app e VS Code | Accettata e realizzata |
| [0005](0005-provider-cloud.md) | Provider cloud: due famiglie di API, chiavi nel portachiavi | Accettata; compatibile OpenAI provato con una chiave vera, Anthropic ancora da provare |
| [0006](0006-download-modelli.md) | Download dei modelli da Hugging Face: file `.part`, ripresa, impronta | Accettata e provata |
| [0007](0007-strumenti-e-permessi.md) | Strumenti e permessi: il nucleo esegue, l'utente conferma | Accettata e provata; Anthropic solo con test automatici; integrata dalla 0011 |
| [0008](0008-agenti-e-terminale.md) | Agenti: sessioni nel nucleo, terminale con permessi, registro delle azioni | Accettata e provata con un modello locale |
| [0009](0009-mcp.md) | MCP: server stdio avviati con la sessione, strumenti in classe "esecuzione" | Accettata e provata con un server di prova |
| [0010](0010-api-locale-e-vscode.md) | API locale con axum, file di blocco, estensione VS Code con diff | Accettata e provata con un modello locale e VS Code |
| [0011](0011-revisione-sicurezza-e-prestazioni.md) | Revisione di sicurezza e prestazioni; protezioni che l'utente può spegnere | Accettata e realizzata |
| [0012](0012-lingue-interfaccia.md) | Lingue dell'interfaccia: italiano e inglese, testo italiano come chiave | Accettata e realizzata nell'app e nell'estensione VS Code |
| [0013](0013-nucleo-in-area-di-notifica.md) | Nucleo nell'area di notifica: l'interfaccia si chiude, il modello resta | Accettata e provata in sviluppo |
| [0014](0014-modelli-locali-varianti-visione-download.md) | Modelli locali: varianti di llama.cpp, immagini (mmproj), modelli divisi, più download, scaricamento per inattività | Accettata; immagini e modelli divisi solo con test automatici |
