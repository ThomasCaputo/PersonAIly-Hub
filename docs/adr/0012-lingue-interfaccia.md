# 0012 — Lingue dell'interfaccia: italiano e inglese

**Stato:** accettata e realizzata nell'app desktop (14-11-2026) e, il 16-11-2026,
nell'estensione VS Code e per i messaggi d'errore meno comuni del nucleo (vedi
Aggiornamento del 16-11-2026).

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
- Una terza lingua richiede un dizionario in più e una voce in `LANGUAGES`.

## Aggiornamento del 16-11-2026

- **Estensione VS Code:** segue la lingua di VS Code (italiano se è italiano, altrimenti
  inglese), con lo stesso schema: testo italiano come chiave, `src/locales/en.ts`,
  `npm run check:i18n -w personaily-hub-vscode` (anche dentro `npm test`). I testi di
  `package.json` (comandi, impostazioni) stanno in `package.nls.json` (inglese) e
  `package.nls.it.json`. Anche i prompt con cui l'estensione chiede una modifica sono
  nella lingua scelta: un prompt italiano spingeva il modello a rispondere in italiano.
- **Testi del nucleo:** `core-en.ts` copre ora anche gli errori meno comuni. Una
  sostituzione può tradurre a sua volta la parte variabile (`$T1`), per gli errori che
  ne avvolgono un altro. L'estensione usa lo stesso dizionario, copiato in
  `src/generated/` a ogni build (`scripts/sync-core-texts.mjs`). Restano in italiano i
  testi che legge solo il modello (descrizioni degli strumenti, preambolo).
- **Lingua delle risposte:** il prompt di sistema di chat e agenti chiede al modello di
  rispondere nella lingua in cui scrive l'utente e, se non si capisce, in quella scelta
  per l'interfaccia o, in mancanza, in quella del sistema (`sys-locale`).
