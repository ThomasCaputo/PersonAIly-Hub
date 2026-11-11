# Decisioni di architettura (ADR)

Ogni file registra **una** decisione tecnica: il contesto, la scelta, le alternative
scartate e le conseguenze. Una decisione non si riscrive: se cambia, si aggiunge un nuovo
ADR che sostituisce il precedente.

| N. | Decisione | Stato |
|---|---|---|
| [0001](0001-stack-tauri-typescript.md) | App desktop con Tauri, React e TypeScript | Accettata |
| [0002](0002-motore-locale-llama-server.md) | Modelli locali tramite `llama-server` come processo separato | Accettata |
| [0003](0003-collocazione-memoria.md) | Modello e cache KV in una sola memoria; KV a `q8_0` | Accettata, da verificare con misure |
| [0004](0004-nucleo-condiviso-api-locale.md) | Nucleo unico con API locale per app e VS Code | Accettata |
