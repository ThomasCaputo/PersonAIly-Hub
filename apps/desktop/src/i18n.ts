// Lingue dell'interfaccia (GE-5; docs/adr/0012-lingue-interfaccia.md).
//
// I testi si scrivono in italiano direttamente nel codice e passano da `t()`:
// in italiano escono così come sono, in inglese si cercano nel dizionario
// `locales/en.ts`. Un testo che manca dal dizionario esce in italiano, e
// `npm run check:i18n` lo segnala. Le parti variabili si scrivono `{nome}`.

import { Fragment, createElement, type ReactNode } from "react";
import { CORE_EN, CORE_PATTERNS_EN } from "./locales/core-en";
import { EN } from "./locales/en";

export type Lang = "it" | "en";

export const LANGUAGES: { id: Lang; label: string }[] = [
  { id: "it", label: "Italiano" },
  { id: "en", label: "English" },
];

let current: Lang = "it";

export function currentLanguage(): Lang {
  return current;
}

export function applyLanguage(lang: Lang) {
  current = lang;
  document.documentElement.lang = lang;
}

/** La lingua del sistema, finché l'utente non ne sceglie una. */
export function systemLanguage(): Lang {
  return (navigator.language ?? "").toLowerCase().startsWith("it") ? "it" : "en";
}

export const isLang = (value: unknown): value is Lang => value === "it" || value === "en";

type Vars = Record<string, string | number>;

function lookup(text: string): string {
  return current === "en" ? (EN[text] ?? text) : text;
}

function fill(text: string, vars?: Vars): string {
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? String(typeof vars[name] === "number" ? num(vars[name] as number) : vars[name]) : whole,
  );
}

/** Un testo dell'interfaccia nella lingua scelta. */
export function t(text: string, vars?: Vars): string {
  return fill(lookup(text), vars);
}

/** Come `t`, ma le parti variabili possono essere elementi (grassetto, codice, pulsanti). */
export function tn(text: string, parts: Record<string, ReactNode>): ReactNode {
  const pieces = lookup(text).split(/\{(\w+)\}/g);
  return createElement(
    Fragment,
    null,
    ...pieces.map((piece, index) =>
      index % 2 === 1 ? createElement(Fragment, { key: index }, parts[piece] ?? `{${piece}}`) : piece,
    ),
  );
}

/**
 * Segna un testo da tradurre più tardi, al momento di mostrarlo, con `t()`.
 * Serve per le costanti: si leggono prima che l'utente scelga la lingua.
 */
export const msg = (text: string) => text;

/**
 * Un testo che arriva dal nucleo (avvisi, richieste di conferma, errori
 * frequenti). Il nucleo scrive in italiano: in inglese si traducono i testi
 * noti, gli altri restano com'erano.
 */
export function tc(text: string): string {
  if (current !== "en") return text;
  const exact = CORE_EN[text.trim()];
  if (exact) return exact;
  for (const [pattern, replacement] of CORE_PATTERNS_EN) {
    if (pattern.test(text)) return text.replace(pattern, replacement);
  }
  return text;
}

/** Il formato di numeri e date della lingua scelta. */
export function locale(): string {
  return current === "en" ? "en-US" : "it-IT";
}

export function num(value: number, options?: Intl.NumberFormatOptions): string {
  return value.toLocaleString(locale(), options);
}
