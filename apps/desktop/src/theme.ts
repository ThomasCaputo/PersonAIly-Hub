// Tema chiaro e scuro (GE-6). Di base segue il sistema; la scelta fatta in
// Impostazioni → Aspetto si salva nel nucleo, insieme alle altre preferenze.
//
// I colori sono token CSS in App.css: l'attributo `data-theme` sulla radice
// sceglie il gruppo, senza attributo decide `prefers-color-scheme`.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type Theme } from "./api";

/** Copia locale della scelta: evita un lampo del tema sbagliato all'apertura. */
const THEME_KEY = "theme";

export const isTheme = (value: unknown): value is Theme =>
  value === "system" || value === "light" || value === "dark";

export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // Senza memoria del browser la scelta arriva comunque dal nucleo.
  }
  // Anche la barra del titolo di Windows segue il tema.
  getCurrentWindow()
    .setTheme(theme === "system" ? null : theme)
    .catch(() => {});
}

/** All'avvio: subito la copia locale, poi la scelta salvata nel nucleo. */
export function initTheme() {
  let cached: unknown = null;
  try {
    cached = localStorage.getItem(THEME_KEY);
  } catch {
    // Memoria del browser non disponibile.
  }
  if (isTheme(cached) && cached !== "system") document.documentElement.dataset.theme = cached;
  api
    .preferences()
    .then((p) => applyTheme(p.theme))
    .catch(() => {});
}
