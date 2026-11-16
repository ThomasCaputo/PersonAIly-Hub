# 0013 — Nucleo nell'area di notifica, interfaccia che si chiude

**Stato:** accettata e realizzata (16-11-2026). Provata in sviluppo: con la finestra
chiusa la pagina web sparisce, `llama-server` resta acceso, l'API locale risponde
(anche `/v1/chat/completions`); una seconda copia dell'app riapre la finestra della
prima e si chiude.

## Contesto

Fino alla Tappa 5 il nucleo viveva dentro il processo dell'app desktop: chiudere la
finestra chiudeva tutto, modello compreso. Chi usa l'estensione VS Code o lascia
lavorare un agente deve però poter tenere caricato il modello senza tenere aperta
l'interfaccia, che con WebView2 occupa da sola qualche centinaio di MB di RAM. In
futuro altre funzioni (agenti pianificati, indicizzazione dei documenti) avranno lo
stesso bisogno.

## Decisione

**Il nucleo vive quanto il processo, l'interfaccia quanto la finestra.**

- Chiudendo la finestra questa si **distrugge** (non si nasconde): la WebView e la sua
  memoria se ne vanno. Il processo resta, con nucleo, modello, agenti e API locale.
- Un'**icona nell'area di notifica** riapre la finestra (clic) o chiude tutto (menu →
  Esci). Il menu segue la lingua dell'interfaccia.
- La finestra si ricrea dalla stessa configurazione di `tauri.conf.json`, da un thread
  a parte: su Windows crearla dentro un callback del thread principale non funziona.
- **Una sola copia dell'app** (`tauri-plugin-single-instance`): riaprirla dal menu Start
  riapre la finestra di quella già accesa invece di avviare un secondo nucleo.
- La preferenza "Resta attiva nell'area di notifica" (Impostazioni → Chiusura della
  finestra, accesa di base) riporta al comportamento di prima. Se l'icona non si può
  creare la finestra resta l'unico modo di usare l'app, e chiuderla chiude tutto.
- Con l'app in sottofondo il modello non deve restare in memoria per sempre senza che
  nessuno lo usi: lo **scaricamento dopo un periodo di inattività** (LO-8, Modelli →
  Motore) vale anche lì ([ADR 0014](0014-modelli-locali-varianti-visione-download.md)).

Una risposta in chat avviata dalla finestra continua anche se la finestra si chiude:
finisce nell'archivio e si ritrova riaprendo l'app.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Interfaccia come client HTTP di `app-server` (due processi) | Più pulita sulla carta, ma andava riscritto tutto il collegamento dell'interfaccia (circa 1200 righe di comandi Tauri) e si perdevano i canali diretti per lo streaming. Lo stesso risultato per l'utente si ottiene con molto meno |
| Nascondere la finestra invece di distruggerla | La WebView resta in memoria: proprio ciò che si voleva liberare |
| Lasciare il lavoro in sottofondo solo ad `app-server` | L'utente dovrebbe sapere quale dei due è attivo; e `app-server` si chiude da solo dopo 30 minuti senza richieste |

## Conseguenze

- `app-server` resta per l'estensione quando l'app non è mai stata aperta; se l'app parte,
  gli chiede di chiudersi come prima (ADR 0010).
- Chiudere la finestra non libera più la memoria del modello: lo dice la spiegazione
  nelle Impostazioni, e c'è lo scaricamento automatico.
- Il codice dell'interfaccia non può più contare sul fatto che "finestra chiusa = app
  chiusa": tutto ciò che deve sopravvivere sta nel nucleo, come già per agenti e download.
