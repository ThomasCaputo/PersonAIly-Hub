// Contrasti dei due temi (accessibilità): i colori si leggono da App.css e i
// testi devono restare leggibili, almeno 4,5:1 come chiede WCAG 2.1 (livello
// AA) per il testo normale, 3:1 per bordi e indicatori di fuoco.

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("./App.css", import.meta.url), "utf8");

/** Le variabili `--md-*` di un blocco CSS che inizia con `selector`. */
function palette(selector: string): Record<string, string> {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`manca ${selector}`);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  return Object.fromEntries([...body.matchAll(/--(md-[\w-]+):\s*(#[0-9a-f]{6})/gi)].map((m) => [m[1], m[2]]));
}

const light = palette(":root {");
const dark = { ...light, ...palette(':root[data-theme="dark"]') };

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Testo su sfondo: le combinazioni usate davvero nell'interfaccia. */
const TEXT_PAIRS: [string, string][] = [
  ["md-on-surface", "md-surface"],
  ["md-on-surface", "md-surface-container"],
  ["md-on-surface", "md-surface-container-high"],
  ["md-on-surface-variant", "md-surface"],
  ["md-on-surface-variant", "md-surface-container"],
  ["md-on-surface-variant", "md-surface-container-high"],
  ["md-primary", "md-surface"],
  ["md-primary", "md-surface-container"],
  ["md-on-primary", "md-primary"],
  ["md-on-primary-container", "md-primary-container"],
  ["md-on-secondary-container", "md-secondary-container"],
  ["md-on-tertiary-container", "md-tertiary-container"],
  ["md-error", "md-surface-container"],
  ["md-on-error-container", "md-error-container"],
  ["md-success", "md-surface-container"],
  ["md-code-text", "md-code-bg"],
];

describe.each([
  ["chiaro", light],
  ["scuro", dark],
])("tema %s", (_, colors) => {
  it.each(TEXT_PAIRS)("%s su %s si legge (≥ 4,5:1)", (fg, bg) => {
    expect(colors[fg], fg).toBeDefined();
    expect(colors[bg], bg).toBeDefined();
    expect(contrast(colors[fg], colors[bg])).toBeGreaterThanOrEqual(4.5);
  });

  it("l'indicatore del fuoco si vede sugli sfondi (≥ 3:1)", () => {
    for (const bg of ["md-surface", "md-surface-container", "md-surface-container-high"]) {
      expect(contrast(colors["md-primary"], colors[bg])).toBeGreaterThanOrEqual(3);
    }
  });
});
