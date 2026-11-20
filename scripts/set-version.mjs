// Cambia la versione dell'app in tutti i posti in cui è scritta, poi
// aggiorna Cargo.lock e package-lock.json.
//
// Uso, dalla cartella del repository:  node scripts/set-version.mjs 1.0.0
//
// Non crea il tag e non tocca CHANGELOG.md: la sezione della nuova versione
// va scritta a mano (scripts/release.ps1 la usa come note di rilascio).
import { execFileSync, execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version ?? "")) {
  console.error("Uso: node scripts/set-version.mjs <versione>, per esempio 1.0.0");
  process.exit(1);
}

/** Sostituisce `pattern` (che deve comparire una volta sola) nel file. */
function replaceOnce(file, pattern, replacement) {
  const path = join(root, file);
  const text = readFileSync(path, "utf8");
  const matches = text.match(new RegExp(pattern.source, pattern.flags.replace("g", "") + "g"));
  if (matches?.length !== 1) throw new Error(`${file}: ${matches?.length ?? 0} versioni trovate invece di una`);
  writeFileSync(path, text.replace(pattern, replacement));
  console.log(`${file} → ${version}`);
}

// Cargo: la versione del workspace vale per nucleo, server e app.
replaceOnce("Cargo.toml", /^version = "[^"]+"$/m, `version = "${version}"`);
// Tauri: nome dei file dell'installer e versione confrontata dall'aggiornamento.
replaceOnce("apps/desktop/src-tauri/tauri.conf.json", /^  "version": "[^"]+",$/m, `  "version": "${version}",`);
for (const file of [
  "package.json",
  "apps/desktop/package.json",
  "extensions/vscode/package.json",
  "packages/api-client/package.json",
]) {
  replaceOnce(file, /^  "version": "[^"]+",$/m, `  "version": "${version}",`);
}

execFileSync("cargo", ["update", "--workspace", "--offline"], { cwd: root, stdio: "inherit" });
// Su Windows npm è un .cmd: serve la shell. Il comando è fisso, niente da quotare.
execSync("npm install --package-lock-only --ignore-scripts", { cwd: root, stdio: "inherit" });
console.log(`\nFatto. Ora: controlla CHANGELOG.md, esegui npm run check, poi crea il tag v${version}.`);
