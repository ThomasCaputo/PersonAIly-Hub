# PersonAIly Hub per VS Code

Usa da VS Code i modelli di PersonAIly Hub, locali (`.gguf`) e cloud.

- **Pannello chat** nella barra laterale (icona a fumetto). Le conversazioni sono quelle
  dell'app: le ritrovi anche nella sua pagina Chat.
- **Contesto dell'editor**: con la casella “Includi il file aperto o la selezione” il
  modello vede la selezione (estesa a righe intere) oppure il file intero.
- **Modifiche come diff**: `Ctrl+Alt+I` (o tasto destro → *Chiedi una modifica…*)
  chiede una modifica alla selezione o al file e la mostra come diff. Nella chat, il
  pulsante *Applica come diff* sopra un blocco di codice fa lo stesso. Si accetta o
  rifiuta con i pulsanti in basso a sinistra nella barra di stato, oppure con ✓ e ✗ in
  alto nel diff. Nulla cambia nel file finché non accetti.
- **Modello**: si sceglie dalla barra di stato in basso a destra. Un modello locale non
  ancora caricato viene caricato con i parametri salvati nell'app.
- **Agenti**: *PersonAIly Hub: Avvia un agente* fa partire un agente dell'app su un compito.
  L'avanzamento compare nella barra di stato e le richieste di conferma arrivano come
  notifiche (*Consenti*, *Consenti sempre in questa sessione*, *Nega*); alla fine si può
  aprire il resoconto. *Segui gli agenti al lavoro* fa lo stesso con un agente avviato
  dall'app.
- **Lingua**: l'estensione parla la lingua di VS Code (italiano, altrimenti inglese),
  anche nei messaggi d'errore che arrivano dall'app.

## Come si collega

L'estensione non contiene modelli né logica propria: parla con il nucleo dell'app tramite
l'API locale su `127.0.0.1`, protetta da un token. Porta e token sono nel file `core.lock`
della cartella dati dell'app (`%APPDATA%\dev.personaily.hub`).

- **App aperta** (anche solo nell'area di notifica): l'estensione usa il nucleo dell'app.
- **App chiusa**: l'estensione avvia `app-server.exe`, il nucleo senza interfaccia, che
  l'installer mette accanto all'app: lo trova da sola. Con l'exe portatile (che non lo
  contiene) va indicato il percorso nell'impostazione `personailyHub.serverPath`. Si chiude da solo
  dopo 30 minuti senza richieste, o quando si chiude la finestra di VS Code che l'ha avviato.
  Se nel frattempo apri l'app, prende lei il suo posto e l'estensione si ricollega da sola.

## Impostazioni

| Impostazione | Significato |
|---|---|
| `personailyHub.serverPath` | Percorso di `app-server.exe`; vuoto: quello installato con l'app |
| `personailyHub.dataDir` | Cartella dati dell'app, se non è quella predefinita |
| `personailyHub.maxContextChars` | Caratteri massimi del file inviati come contesto (60 000) |

## Sviluppo

```
npm run build -w personaily-hub-vscode     # compila in out/
npm test -w personaily-hub-vscode          # prove senza VS Code e controllo delle traduzioni
npm run package -w personaily-hub-vscode   # crea personaily-hub.vsix
```

Per provarla senza installarla: apri la cartella `extensions/vscode` in VS Code e premi F5.
Per installarla: `code --install-extension personaily-hub.vsix`.
