// Prepara `app-server` (il nucleo senza interfaccia) per l'installer: lo
// compila in versione release e lo copia dove Tauri cerca i programmi da
// installare accanto all'app (`bundle.externalBin` in
// apps/desktop/src-tauri/tauri.bundle.conf.json). L'installer lo mette nella
// cartella dell'app e l'app, a ogni avvio, annota dove si trova: così
// l'estensione VS Code lo avvia da sola anche ad app chiusa.
//
// Uso (lo fa `npm run build`): node scripts/build-server.mjs
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: ["ignore", "pipe", "inherit"] }).toString();

// Tauri vuole il nome del programma seguito dalla piattaforma di destinazione.
const host = run("rustc", ["-vV"]).match(/^host: (.+)$/m)?.[1]?.trim();
if (!host) throw new Error("rustc -vV non indica la piattaforma");

execFileSync("cargo", ["build", "--release", "-p", "app-server"], { cwd: root, stdio: "inherit" });

const exe = process.platform === "win32" ? ".exe" : "";
const target = join(root, "apps", "desktop", "src-tauri", "binaries", `app-server-${host}${exe}`);
mkdirSync(dirname(target), { recursive: true });
copyFileSync(join(root, "target", "release", `app-server${exe}`), target);
console.log(`app-server pronto per l'installer: ${target}`);
