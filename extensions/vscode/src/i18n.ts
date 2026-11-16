// Lingua dell'estensione: segue quella di VS Code (italiano se è italiano,
// altrimenti inglese). Come nell'app desktop (ADR 0012) i testi si scrivono in
// italiano nel codice e passano da `t()`; la traduzione inglese sta in
// `locales/en.ts`. Non usa `vscode`: si prova con Node da solo.

import { CORE_EN, CORE_PATTERNS_EN } from "./generated/core-en";
import { EN } from "./locales/en";

export type Lang = "it" | "en";

let current: Lang = "it";

/** La lingua dell'estensione a partire da quella di VS Code (`it`, `en-us`, `de`…). */
export function languageFor(vscodeLanguage: string | undefined): Lang {
  return (vscodeLanguage ?? "").toLowerCase().startsWith("it") ? "it" : "en";
}

export function setLanguage(lang: Lang): void {
  current = lang;
}

export function currentLanguage(): Lang {
  return current;
}

/** Il formato di numeri e date della lingua scelta. */
export function locale(): string {
  return current === "en" ? "en-US" : "it-IT";
}

/** Un testo nella lingua scelta. Le parti variabili si scrivono `{nome}`. */
export function t(text: string, vars?: Record<string, string | number>): string {
  const translated = current === "en" ? (EN[text] ?? text) : text;
  if (!vars) return translated;
  return translated.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? (typeof vars[name] === "number" ? vars[name].toLocaleString(locale()) : String(vars[name])) : whole,
  );
}

/**
 * Un testo che arriva dal nucleo (errori, richieste di conferma): il nucleo
 * scrive in italiano, in inglese si traducono quelli noti con lo stesso
 * dizionario dell'app desktop. `$T1` nelle sostituzioni è la parte variabile
 * tradotta a sua volta.
 */
export function tc(text: string): string {
  return current === "en" ? translateCore(text, 0) : text;
}

function translateCore(text: string, depth: number): string {
  const exact = CORE_EN[text.trim()];
  if (exact) return exact;
  for (const [pattern, replacement] of CORE_PATTERNS_EN) {
    const match = pattern.exec(text);
    if (!match) continue;
    const filled = replacement.replace(/\$(T?)(\d)/g, (_, translated: string, n: string) => {
      const part = match[Number(n)] ?? "";
      return translated && depth < 3 ? translateCore(part, depth + 1) : part;
    });
    return text.slice(0, match.index) + filled + text.slice(match.index + match[0].length);
  }
  return text;
}

/** I testi del pannello chat (media/chat.js), già tradotti, da passargli all'avvio. */
export const VIEW_TEXTS = [
  "Applica come diff",
  "Mostra la modifica nel file e chiedi conferma",
  "Copia",
  "Copiato",
  "strumento",
  "Usa: {tools}",
  "Interrompi",
  "Invia",
  "Sto rispondendo…",
  "Sto ragionando…",
  "Scegli il modello",
  "Fai una domanda sul codice. Con la casella sotto spuntata il modello vede il file aperto, o solo la selezione. Per modificare direttamente il file: Ctrl+Alt+I.",
  "Il modello chiede: {summary}",
  "Consenti",
  "Consenti sempre",
  "Nega",
] as const;

export function viewTexts(): Record<string, string> {
  return Object.fromEntries(VIEW_TEXTS.map((text) => [text, t(text)]));
}
