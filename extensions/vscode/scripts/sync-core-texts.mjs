// Copia nell'estensione la traduzione inglese dei testi del nucleo, che sta
// nell'app desktop (apps/desktop/src/locales/core-en.ts): un solo dizionario
// per entrambi. La copia (src/generated/) non va nel repository: la rifà ogni
// build, prova o pacchetto.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const source = join(here, "..", "..", "..", "apps", "desktop", "src", "locales", "core-en.ts");
const target = join(here, "..", "src", "generated", "core-en.ts");

const text = readFileSync(source, "utf8");
mkdirSync(dirname(target), { recursive: true });
writeFileSync(
  target,
  "// Copia generata da scripts/sync-core-texts.mjs: non modificarla, cambia\n" +
    "// apps/desktop/src/locales/core-en.ts.\n\n" +
    text,
);
