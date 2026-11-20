import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { beforeAll, describe, expect, it } from "vitest";
import { cloudPresetKey, gb, mcpToolLabel, size, usd } from "./api";
import { applyLanguage, t, tc } from "./i18n";
import { CORE_EN, CORE_PATTERNS_EN } from "./locales/core-en";
import { placementLabel } from "./placement";

// `applyLanguage` imposta la lingua della pagina: qui basta un documento finto.
beforeAll(() => {
  (globalThis as { document?: unknown }).document ??= { documentElement: {} };
});

describe("testi dell'interfaccia", () => {
  it("in italiano escono come sono, con le parti variabili", () => {
    applyLanguage("it");
    // In italiano le migliaia si separano da cinque cifre in su.
    expect(t("Contesto occupato: {used} / {total} token", { used: 1234, total: 32768 })).toBe(
      "Contesto occupato: 1234 / 32.768 token",
    );
  });

  it("in inglese si traducono, numeri compresi", () => {
    applyLanguage("en");
    expect(t("Contesto occupato: {used} / {total} token", { used: 1234, total: 8192 })).toBe(
      "Context used: 1,234 / 8,192 tokens",
    );
    // Un testo senza traduzione resta in italiano invece di sparire.
    expect(t("Testo che non esiste")).toBe("Testo che non esiste");
  });

  it("una parte variabile assente resta visibile", () => {
    applyLanguage("it");
    expect(t("Ciao {nome}")).toBe("Ciao {nome}");
  });
});

describe("testi del nucleo", () => {
  it("in italiano non cambiano", () => {
    applyLanguage("it");
    expect(tc("Nessun modello locale caricato.")).toBe("Nessun modello locale caricato.");
  });

  it("testi esatti ed espressioni", () => {
    applyLanguage("en");
    expect(tc("Nessun modello locale caricato.")).toBe("No local model loaded.");
    expect(tc("Leggere il file C:\\progetto\\a.txt")).toBe("Read the file C:\\progetto\\a.txt");
    expect(tc("Un testo sconosciuto.")).toBe("Un testo sconosciuto.");
  });

  it("$T traduce a sua volta la parte variabile", () => {
    applyLanguage("en");
    expect(tc("file del modello non leggibile: versione GGUF 4 non supportata")).toBe(
      "model file not readable: GGUF version 4 not supported",
    );
  });

  it("$$ diventa il simbolo del dollaro", () => {
    applyLanguage("en");
    expect(
      tc(
        "Limite di spesa mensile raggiunto: spesi circa 12.30 $ su 10.00 $. Puoi alzarlo in Impostazioni → Spesa cloud, o usare un modello locale.",
      ),
    ).toBe(
      "Monthly spending limit reached: about $12.30 spent of $10.00. You can raise it in Settings → Cloud spending, or use a local model.",
    );
  });

  // ADR 0012: se il nucleo cambia un testo, la sua traduzione smette di
  // funzionare senza che nessuno se ne accorga. Qui si controlla che ogni
  // testo tradotto esista ancora nel codice Rust.
  it("ogni testo tradotto esiste ancora nel nucleo", () => {
    const root = join(__dirname, "..", "..", "..");
    const rust = rustSources([join(root, "crates"), join(root, "apps", "desktop", "src-tauri", "src")])
      // Le stringhe Rust spezzate con `\` a fine riga, e le virgolette con escape.
      .replace(/\\\r?\n\s*/g, "")
      .replace(/\\"/g, '"');
    const missing = Object.keys(CORE_EN).filter((text) => !rust.includes(text));
    const stalePatterns = CORE_PATTERNS_EN.map(([pattern]) => literalStart(pattern))
      .filter((start) => start.length > 4)
      .filter((start) => !rust.includes(start));
    expect(missing).toEqual([]);
    expect(stalePatterns).toEqual([]);
  });
});

describe("formati", () => {
  it("dimensioni e importi", () => {
    applyLanguage("it");
    expect(size(512 * 1024 ** 2)).toBe("512 MB");
    expect(size(4.4 * 1024 ** 3)).toBe("4,4 GB");
    expect(gb(1024 ** 3)).toBe("1,0");
    applyLanguage("en");
    expect(usd(12.3)).toBe("$12.30");
    expect(usd(0.0123)).toBe("$0.012");
    expect(usd(0.00012)).toBe("$0.0001");
  });

  it("nomi e chiavi", () => {
    expect(mcpToolLabel("mcp__file_system__read")).toBe("MCP file system: read");
    expect(mcpToolLabel("read_file")).toBeNull();
    expect(cloudPresetKey("p1", "claude")).toBe("cloud:p1:claude");
  });

  it("collocazione del modello", () => {
    applyLanguage("en");
    const base = { contextLength: 8192, kvCacheType: "q8_0", vision: true } as const;
    expect(placementLabel({ ...base, placement: "vram" } as never)).toBe("VRAM only");
    expect(placementLabel({ ...base, placement: "hybrid", gpuLayers: 20 } as never)).toBe(
      "Hybrid: 20 layers on the GPU",
    );
  });
});

/** Tutto il codice Rust delle cartelle indicate, `target` escluso. */
function rustSources(dirs: string[]): string {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name === "target") continue;
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.name.endsWith(".rs")) out.push(readFileSync(path, "utf8"));
    }
  };
  dirs.forEach(walk);
  return out.join("\n");
}

/** Il pezzo fisso con cui inizia un'espressione, prima della prima parte variabile. */
function literalStart(pattern: RegExp): string {
  const source = pattern.source.replace(/^\^/, "").replace(/^\\n\?/, "");
  // Si ferma alla prima parte variabile, a un a capo o alla fine del testo.
  return source.split(/(?<!\\)\(|\\n|\$$/)[0].replace(/\\(.)/g, "$1");
}
