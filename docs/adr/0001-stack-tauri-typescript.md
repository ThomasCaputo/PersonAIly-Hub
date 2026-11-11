# 0001 — App desktop con Tauri, React e TypeScript

**Stato:** accettata

## Contesto

Serve un'app desktop per Windows, da portare poi su Linux e macOS, con un'interfaccia
ricca (chat in streaming, Markdown, diff, pannelli). L'app gira accanto a modelli che
occupano molta memoria, su macchine che possono avere solo 16 GB di RAM: quello che
consuma l'app è tolto ai modelli. Deve inoltre integrarsi con VS Code, le cui estensioni
si scrivono in TypeScript.

## Decisione

- **Tauri 2** come guscio dell'app.
- **React + TypeScript** per l'interfaccia.
- **Rust** per il nucleo.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Electron + TypeScript | Un solo linguaggio, ma ogni finestra porta con sé un browser intero: centinaia di MB di RAM in più |
| C# / WinUI 3 | Ottima su Windows, ma l'interfaccia andrebbe riscritta per le altre piattaforme |
| Python + interfaccia web | Comodo per la parte AI, ma distribuzione, installer e avvio sono più pesanti e fragili |

## Conseguenze

- App leggera: usa il WebView2 già presente in Windows.
- Multipiattaforma senza riscrivere l'interfaccia.
- TypeScript è condiviso tra interfaccia ed estensione VS Code.
- **Costo:** Rust ha una curva di apprendimento ripida. Per contenerla il nucleo è diviso
  in moduli piccoli con confini chiari, e la parte più difficile (l'inferenza) resta fuori
  dal nostro codice (vedi ADR 0002).
- WebView diverso per piattaforma (WebView2, WebKit): l'interfaccia andrà provata su
  ciascuna quando arriveranno Linux e macOS.
