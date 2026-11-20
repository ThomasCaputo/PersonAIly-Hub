// Prove delle parti dell'estensione che non dipendono da VS Code.
// Si eseguono con `npm test` (compila e poi `node --test`).
const test = require("node:test");
const assert = require("node:assert/strict");
const http = require("node:http");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { CoreClient, readSse, defaultDataDir, readLock, findServer } = require("../out/core.js");
const i18n = require("../out/i18n.js");
const { extractCode, fitReplacement, fence, editPrompt, contextBlock, openAiModelName } = require("../out/edits.js");

test("estrae il blocco di codice più lungo", () => {
  const reply = "Ecco:\n```ts\nconst a = 1;\nconst b = 2;\n```\nE anche:\n```\nx\n```";
  assert.equal(extractCode(reply), "const a = 1;\nconst b = 2;");
});

test("senza blocchi usa tutto il testo; un blocco non chiuso vale comunque", () => {
  assert.equal(extractCode("  return 1;  "), "return 1;");
  assert.equal(extractCode("```py\nprint(1)\nprint(2)"), "print(1)\nprint(2)");
  assert.equal(extractCode("   "), null);
});

test("adatta a capo e chiusura al testo sostituito", () => {
  assert.equal(fitReplacement("a\nb\n\n", "x\r\ny", "\r\n"), "a\r\nb");
  assert.equal(fitReplacement("a\nb", "x\n", "\n"), "a\nb\n");
});

test("rimette il rientro che il modello ha tolto, e solo quello", () => {
  const original = "  let s = 0;\n  for (;;) s++;";
  assert.equal(fitReplacement("let s = f(\n  1);", original, "\n"), "  let s = f(\n    1);");
  // Codice già rientrato: non si tocca.
  assert.equal(fitReplacement("    x();", original, "\n"), "    x();");
  // File intero senza rientro: non si tocca.
  assert.equal(fitReplacement("y();", "x();\n", "\n"), "y();\n");
});

test("il blocco di contesto non si chiude sul codice che contiene apici", () => {
  const code = "const s = `\n```\n`;";
  const block = fence(code, "ts");
  assert.ok(block.startsWith("````ts\n"));
  assert.equal(extractCode(block), code);
  const prompt = editPrompt({ fileName: "a.ts", languageId: "ts", code: "x", startLine: 2, endLine: 4 }, "rinomina");
  assert.match(prompt, /^Righe 2-4 del file a\.ts\./);
  assert.match(contextBlock({ fileName: "a.ts", languageId: "ts", code: "x" }), /^\[Contesto da VS Code: a\.ts, file intero\]/);
});

test("nomi dei modelli per l'endpoint OpenAI", () => {
  assert.equal(openAiModelName({ kind: "local" }), "local");
  assert.equal(openAiModelName({ kind: "cloud", providerId: "p-1", model: "a/b" }), "p-1/a/b");
});

test("cartella dati come quella dell'app", () => {
  assert.equal(
    defaultDataDir({ APPDATA: "C:\\Utente\\AppData\\Roaming" }, "win32"),
    path.join("C:\\Utente\\AppData\\Roaming", "dev.personaily.hub"),
  );
});

test("legge eventi SSE spezzati a metà, commenti compresi", async () => {
  const text = ": keep-alive\n\nevent: text\ndata: {\"text\":\"Ci\"}\n\nevent: te";
  const rest = "xt\ndata: {\"text\":\"ao\"}\r\n\r\ndata: [DONE]\n\n";
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(text));
      controller.enqueue(new TextEncoder().encode(rest));
      controller.close();
    },
  });
  const events = [];
  await readSse(stream, (name, data) => events.push([name, data]));
  assert.deepEqual(events, [
    ["text", '{"text":"Ci"}'],
    ["text", '{"text":"ao"}'],
    ["message", "[DONE]"],
  ]);
});

test("client: token, nuovo token dopo 401, errori leggibili", async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "personaily-hub-"));
  let token = "primo";
  const server = http.createServer((req, res) => {
    if (req.headers.authorization !== `Bearer ${token}`) {
      res.writeHead(401, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: { message: "token sbagliato" } }));
    } else if (req.url === "/api/info") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ kind: "app" }));
    } else {
      res.writeHead(400, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: { message: "Nessun modello locale caricato." } }));
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = server.address().port;
  const writeLock = () =>
    fs.writeFileSync(
      path.join(dir, "core.lock"),
      JSON.stringify({ port, token, pid: process.pid, kind: "app", version: "0" }),
    );
  writeLock();
  try {
    assert.equal(readLock(dir).port, port);
    const client = new CoreClient({ dataDir: () => dir, serverPath: () => "", log: () => {} });
    assert.deepEqual(await client.request("GET", "/api/info"), { kind: "app" });

    // L'app rigenera il token: il client rilegge il file di blocco da solo.
    token = "secondo";
    writeLock();
    assert.deepEqual(await client.request("GET", "/api/info"), { kind: "app" });

    await assert.rejects(client.request("GET", "/altro"), (e) => e.status === 400 && /Nessun modello/.test(e.message));
  } finally {
    server.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("client: senza nucleo né app-server spiega cosa fare", async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "personaily-hub-"));
  // Un'app installata davvero su questo PC non deve partire durante la prova.
  const saved = process.env.LOCALAPPDATA;
  process.env.LOCALAPPDATA = dir;
  try {
    const client = new CoreClient({ dataDir: () => dir, serverPath: () => "", log: () => {} });
    await assert.rejects(client.request("GET", "/api/info"), (e) => e.status === 0 && /serverPath/.test(e.message));
  } finally {
    process.env.LOCALAPPDATA = saved;
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("trova app-server: impostazione, poi install.json dell'app, poi la cartella dell'installer", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "personaily-hub-"));
  try {
    const exists = (p) => fs.existsSync(p);
    const env = { LOCALAPPDATA: path.join(dir, "local") };
    assert.equal(findServer("  C:\a\app-server.exe ", dir, env, "win32", exists), "C:\a\app-server.exe");
    assert.equal(findServer("", dir, env, "win32", exists), "");

    const installed = path.join(dir, "local", "PersonAIly Hub", "app-server.exe");
    fs.mkdirSync(path.dirname(installed), { recursive: true });
    fs.writeFileSync(installed, "");
    assert.equal(findServer("", dir, env, "win32", exists), installed);

    const recorded = path.join(dir, "altrove", "app-server.exe");
    fs.mkdirSync(path.dirname(recorded), { recursive: true });
    fs.writeFileSync(recorded, "");
    fs.writeFileSync(path.join(dir, "install.json"), JSON.stringify({ serverPath: recorded, version: "0.1.0" }));
    assert.equal(findServer("", dir, env, "win32", exists), recorded);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("lingua di VS Code, testi tradotti e messaggi del nucleo in inglese", () => {
  assert.equal(i18n.languageFor("it"), "it");
  assert.equal(i18n.languageFor("en-us"), "en");
  assert.equal(i18n.languageFor("de"), "en");
  try {
    i18n.setLanguage("en");
    assert.equal(i18n.t("Invia"), "Send");
    assert.equal(i18n.t("{n} caratteri", { n: 1234 }), "1,234 characters");
    assert.equal(i18n.tc("Nessun modello locale caricato."), "No local model loaded.");
    assert.equal(
      i18n.tc("file del modello non leggibile: il modello è diviso in 3 file e ne mancano 1"),
      "model file not readable: the model is split into 3 files and 1 are missing",
    );
    assert.match(contextBlock({ fileName: "a.ts", languageId: "ts", code: "x" }), /^\[Context from VS Code: a\.ts, whole file\]/);
    assert.match(
      editPrompt({ fileName: "a.ts", languageId: "ts", code: "x" }, "rename"),
      /^Whole file: a\.ts\.[\s\S]*Instruction: rename$/,
    );
  } finally {
    i18n.setLanguage("it");
  }
  assert.equal(i18n.t("Invia"), "Invia");
  assert.equal(i18n.tc("Nessun modello locale caricato."), "Nessun modello locale caricato.");
});

test("completamento in linea: contesto tagliato alle righe intere", () => {
  const { contextAround } = require("../out/infill.js");
  const text = "riga uno\nriga due\nriga tre\nriga quattro";
  const offset = text.indexOf("tre");
  const { prefix, suffix } = contextAround(text, offset, 12, 8);
  assert.equal(prefix, "riga ");
  assert.equal(suffix, "tre");
  const all = contextAround(text, offset);
  assert.equal(all.prefix + all.suffix, text);
});

test("completamento in linea: niente doppioni del testo dopo il cursore", () => {
  const { cleanCompletion } = require("../out/infill.js");
  assert.equal(cleanCompletion("a + b;\n}\n", ";\n}\n"), "a + b");
  assert.equal(cleanCompletion("return x;  \n", "\n}"), "return x;");
  assert.equal(cleanCompletion("   \n", ""), "");
});
