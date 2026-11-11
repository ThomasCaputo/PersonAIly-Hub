# 0004 — Nucleo unico con API locale per app e VS Code

**Stato:** accettata

## Contesto

L'app deve essere integrabile in VS Code. Un'estensione VS Code è un programma separato,
in TypeScript, che non può caricare direttamente il codice dell'app desktop. Se la logica
stesse dentro l'interfaccia dell'app, l'estensione dovrebbe duplicarla.

## Decisione

- Tutta la logica (provider, modelli locali, strumenti, permessi, agenti, archivio) sta in
  una libreria Rust, il **nucleo**, senza interfaccia.
- Il nucleo espone un'**API locale** HTTP su `127.0.0.1`, protetta da token, con eventi in
  tempo reale via Server-Sent Events.
- L'app desktop usa il nucleo direttamente; l'estensione VS Code lo usa tramite l'API.
- Esiste **un solo nucleo attivo** per utente. Chi parte per primo (app o eseguibile
  `server`) scrive un file di blocco con porta e token; chi arriva dopo si collega.
- L'API comprende una parte **compatibile con OpenAI**, così programmi di terzi possono
  usare i modelli gestiti dall'app.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Logica dentro l'interfaccia dell'app | L'estensione dovrebbe reimplementare tutto |
| Estensione autonoma con un proprio motore | Due copie del modello in memoria, impostazioni e cronologia separate |
| Comunicazione tramite named pipe | Più sicura per costruzione, ma diversa su ogni piattaforma e non utilizzabile dai client OpenAI esistenti |

## Conseguenze

- Aggiungere un nuovo client (riga di comando, altri editor) non richiede di toccare il nucleo.
- L'API locale è una superficie d'attacco: ascolto solo su `127.0.0.1`, token obbligatorio,
  controllo dell'intestazione `Origin` per bloccare le pagine web che provassero a chiamarla.
- I tipi dell'API vanno tenuti allineati tra Rust e TypeScript: verranno generati
  automaticamente dalle definizioni Rust.
