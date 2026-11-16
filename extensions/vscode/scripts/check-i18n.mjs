// Controlla che ogni testo dell'estensione abbia la traduzione inglese, come
// `apps/desktop/scripts/check-i18n.mjs` per l'app: i testi passati a t() in
// `src/` e quelli del pannello chat (VIEW_TEXTS in src/i18n.ts) contro le
// chiavi di `src/locales/en.ts`; poi che package.nls.json e
// package.nls.it.json abbiano le stesse chiavi usate in package.json.
// Uso: npm run check:i18n   (esce con errore se manca qualcosa)

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const base = join(dirname(fileURLToPath(import.meta.url)), "..");
const root = join(base, "src");

function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === "locales" || name === "generated" ? [] : files(path);
    return name.endsWith(".ts") ? [path] : [];
  });
}

const STRING = /"(?:[^"\\\n]|\\.)*"/g;
const CALL = /\bt\(\s*("(?:[^"\\\n]|\\.)*")/g;

const used = new Map();
for (const file of files(root)) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(CALL)) {
    const key = JSON.parse(match[1]);
    if (!used.has(key)) used.set(key, file.slice(root.length + 1));
  }
  const view = text.match(/VIEW_TEXTS = \[([\s\S]*?)\] as const/);
  if (view) for (const literal of view[1].match(STRING) ?? []) used.set(JSON.parse(literal), "media/chat.js");
}

const dictionary = readFileSync(join(root, "locales", "en.ts"), "utf8");
const KEY = /^\s*("(?:[^"\\\n]|\\.)*")\s*:/gm;
const known = new Set([...dictionary.matchAll(KEY)].map((m) => JSON.parse(m[1])));

const missing = [...used.keys()].filter((key) => !known.has(key));
const unused = [...known].filter((key) => !used.has(key));
for (const key of missing) console.log(`manca la traduzione (${used.get(key)}): ${key}`);
for (const key of unused) console.log(`traduzione non usata: ${key}`);

// Testi di package.json (`%chiave%`): in inglese in package.nls.json, in italiano in package.nls.it.json.
const manifest = readFileSync(join(base, "package.json"), "utf8");
const nlsKeys = new Set([...manifest.matchAll(/"%([\w.]+)%"/g)].map((m) => m[1]));
let nlsProblems = 0;
for (const name of ["package.nls.json", "package.nls.it.json"]) {
  const nls = JSON.parse(readFileSync(join(base, name), "utf8"));
  for (const key of nlsKeys) {
    if (!(key in nls)) {
      console.log(`${name}: manca ${key}`);
      nlsProblems++;
    }
  }
  for (const key of Object.keys(nls)) {
    if (!nlsKeys.has(key)) {
      console.log(`${name}: ${key} non è usata in package.json`);
      nlsProblems++;
    }
  }
}

console.log(
  `${used.size} testi, ${missing.length} senza traduzione, ${unused.length} traduzioni non usate, ` +
    `${nlsKeys.size} testi di package.json (${nlsProblems} problemi).`,
);
process.exit(missing.length > 0 || nlsProblems > 0 ? 1 : 0);
