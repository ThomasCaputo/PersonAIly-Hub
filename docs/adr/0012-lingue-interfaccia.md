# 0012 — Lingue dell'interfaccia: italiano e inglese

**Stato:** accettata e realizzata nell'app desktop (14-11-2026). L'estensione VS Code e i
messaggi d'errore meno comuni del nucleo restano in italiano (vedi Conseguenze).

## Contesto

GE-5 chiede l'interfaccia in italiano e inglese. Tutti i testi erano scritti in italiano
direttamente nei componenti React; anche il nucleo scrive in italiano i testi che
l'interfaccia mostra (richieste di conferma, avvisi, errori).

## Decisione

**Il testo italiano è la chiave.** Nei componenti i testi restano in italiano, passati a
`t()` (`apps/desktop/src/i18n.ts`):

```tsx
t("Nessun risultato.")
t("Manca la chiave API di {name}.", { name })          // parti variabili tra graffe
tn("Per eseguire i modelli serve {engine}.", { engine: <strong>llama.cpp</strong> })
const LABEL = msg("Al lavoro");  // costanti: si segnano con msg() e si traducono con t() quando si mostrano
```

In italiano `t()` restituisce il testo così com'è; in inglese lo cerca nel dizionario
`src/locales/en.ts`. I numeri passati come variabili si formattano nella lingua scelta
(`1.234` / `1,234`); lo stesso vale per date, gigabyte e importi (`locale()`).

**Testi del nucleo.** Il nucleo continua a scrivere in italiano: i testi sono anche quelli
che legge il modello e che finiscono nel registro. L'interfaccia li traduce con `tc()`,
che cerca prima il testo esatto e poi una serie di espressioni regolari (richieste di
conferma come "Leggere il file …", avvisi, errori frequenti) in `src/locales/core-en.ts`.
Un testo sconosciuto resta in italiano.

**Scelta e memoria.** La lingua si sceglie in Impostazioni → Lingua e si salva nel
nucleo (impostazione `language`, comandi `language` / `set_language`). Finché l'utente non
sceglie, si segue la lingua del sistema (italiano se è italiano, altrimenti inglese).
`LanguageProvider` tiene la lingua; `App` la legge con `useLanguage()`, quindi un cambio
ridisegna tutta l'interfaccia senza riavviare e senza perdere la pagina aperta.

**Controllo.** `npm run check:i18n -w desktop` (anche dentro `npm run check`) elenca i testi
passati a `t()`, `tn()` e `msg()` senza traduzione e le traduzioni non più usate, ed esce
con errore se ne manca qualcuna.

## Alternative scartate

| Alternativa | Motivo |
|---|---|
| Libreria (i18next, FormatJS) | Una dipendenza in più per due lingue senza plurali complicati; il meccanismo serve in meno di 100 righe |
| Chiavi astratte (`chat.empty`) | Il codice diventa meno leggibile e ogni testo va scritto due volte anche in italiano |
| Lingua passata al nucleo, che traduce da sé | Andrebbero toccati centinaia di messaggi del nucleo, e il modello leggerebbe testi in lingue diverse a seconda dell'utente |
| Lingua nella memoria del browser | Non la vedrebbero il nucleo e, in futuro, l'estensione VS Code |

## Conseguenze

- Cambiando un testo italiano nel codice, la sua traduzione va aggiornata (lo segnala
  `check:i18n`). Se il nucleo cambia un testo tradotto in `core-en.ts`, quel testo torna in
  italiano finché non si aggiorna il file: non c'è un controllo automatico.
- Rimandato: estensione VS Code in inglese (potrebbe seguire la lingua di VS Code o quella
  salvata nel nucleo); traduzione dei messaggi d'errore rari del nucleo.
- Una terza lingua richiede un dizionario in più e una voce in `LANGUAGES`.
