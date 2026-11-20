// Controlla che ogni testo dell'interfaccia abbia la traduzione inglese.
//
// Cerca in `src/` i testi passati a t(), tn() e msg() e li confronta con le
// chiavi di `src/locales/en.ts`. Elenca quelli che mancano (escono in
// italiano anche con l'inglese scelto) e quelli in più (non usati da nessuno).
// Uso: npm run check:i18n        (esce con errore se manca qualcosa)
//      npm run check:i18n -- --list   (stampa i testi mancanti in JSON)

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "src");

function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === "locales" ? [] : files(path);
    return /\.(ts|tsx)$/.test(name) ? [path] : [];
  });
}

/** Il valore di una stringa JavaScript tra virgolette semplici o doppie. */
function unquote(literal) {
  if (literal.startsWith('"')) return JSON.parse(literal);
  const body = literal.slice(1, -1).replace(/\\'/g, "'").replace(/"/g, '\\"');
  return JSON.parse(`"${body}"`);
}

const CALL = /\b(?:t|tn|msg)\(\s*("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*')/g;

const used = new Map();
for (const file of files(root)) {
  // Le prove usano apposta testi senza traduzione.
  if (file.endsWith("i18n.ts") || /\.test\.tsx?$/.test(file)) continue;
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(CALL)) {
    const key = unquote(match[1]);
    if (!used.has(key)) used.set(key, file.slice(root.length + 1));
  }
}

const dictionary = readFileSync(join(root, "locales", "en.ts"), "utf8");
const KEY = /^\s*("(?:[^"\\\n]|\\.)*")\s*:/gm;
const known = new Set([...dictionary.matchAll(KEY)].map((m) => JSON.parse(m[1])));

const missing = [...used.keys()].filter((key) => !known.has(key));
const unused = [...known].filter((key) => !used.has(key));

if (process.argv.includes("--list")) {
  console.log(JSON.stringify(missing, null, 2));
  process.exit(0);
}

for (const key of missing) console.log(`manca la traduzione (${used.get(key)}): ${key}`);
for (const key of unused) console.log(`traduzione non usata: ${key}`);
console.log(`${used.size} testi, ${missing.length} senza traduzione, ${unused.length} traduzioni non usate.`);
process.exit(missing.length > 0 ? 1 : 0);
