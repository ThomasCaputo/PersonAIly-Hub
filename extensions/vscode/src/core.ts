// Collegamento al nucleo di Progetto AI tramite l'API locale.
// Non usa `vscode`: si può provare con Node da solo (vedi test/).

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

/** Contenuto di `core.lock`, scritto da chi ha avviato il nucleo. */
export interface LockInfo {
  port: number;
  token: string;
  pid: number;
  kind: "app" | "server";
  version: string;
}

/** Errore restituito dall'API, con il suo codice HTTP (0 = nucleo irraggiungibile). */
export class CoreError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

/** Deve restare uguale a `APP_IDENTIFIER` in `crates/core/src/instance.rs`. */
export const APP_IDENTIFIER = "dev.progettoai.desktop";

/** La cartella dati dell'app, la stessa usata dal nucleo. */
export function defaultDataDir(env: NodeJS.ProcessEnv = process.env, platform = process.platform): string {
  if (platform === "win32") return join(env.APPDATA ?? "", APP_IDENTIFIER);
  if (platform === "darwin") return join(env.HOME ?? "", "Library", "Application Support", APP_IDENTIFIER);
  return join(env.XDG_DATA_HOME ?? join(env.HOME ?? "", ".local", "share"), APP_IDENTIFIER);
}

export function readLock(dataDir: string): LockInfo | null {
  try {
    const lock = JSON.parse(readFileSync(join(dataDir, "core.lock"), "utf8"));
    return typeof lock.port === "number" && typeof lock.token === "string" ? lock : null;
  } catch {
    return null;
  }
}

/** `true` se il processo esiste ancora. */
export function processAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    // EPERM: esiste, ma appartiene a un altro utente.
    return (e as NodeJS.ErrnoException).code === "EPERM";
  }
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export interface CoreOptions {
  dataDir: () => string;
  /** Percorso di `app-server`; vuoto se non configurato. */
  serverPath: () => string;
  log: (line: string) => void;
}

/** Un evento di un flusso SSE: nome (`message` se assente) e dati. */
export type SseHandler = (event: string, data: string) => void;

/** Legge un flusso Server-Sent Events e chiama `onEvent` per ogni evento completo. */
export async function readSse(body: ReadableStream<Uint8Array>, onEvent: SseHandler): Promise<void> {
  const decoder = new TextDecoder();
  let buffer = "";
  const flush = (block: string) => {
    let name = "message";
    const data: string[] = [];
    for (const line of block.split(/\r?\n/)) {
      if (line.startsWith(":")) continue; // commento (keep-alive)
      if (line.startsWith("event:")) name = line.slice(6).trim();
      else if (line.startsWith("data:")) data.push(line.slice(5).replace(/^ /, ""));
    }
    if (data.length > 0) onEvent(name, data.join("\n"));
  };
  const reader = body.getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let end: number;
    while ((end = buffer.search(/\r?\n\r?\n/)) >= 0) {
      const block = buffer.slice(0, end);
      buffer = buffer.slice(end).replace(/^\r?\n\r?\n/, "");
      flush(block);
    }
  }
  if (buffer.trim()) flush(buffer);
}

/** Client dell'API locale: trova il nucleo (o lo avvia) e gli manda le richieste. */
export class CoreClient {
  private lock: LockInfo | null = null;
  private starting: Promise<LockInfo> | null = null;
  /** `true` se il nucleo senza interfaccia l'ha avviato questa estensione. */
  spawnedByUs = false;

  constructor(private readonly options: CoreOptions) {}

  /** Il nucleo in esecuzione; se non c'è, avvia `app-server` e aspetta che sia pronto. */
  async ensure(): Promise<LockInfo> {
    if (this.lock && processAlive(this.lock.pid)) return this.lock;
    this.starting ??= this.connect().finally(() => (this.starting = null));
    return this.starting;
  }

  private async connect(): Promise<LockInfo> {
    const dataDir = this.options.dataDir();
    const existing = readLock(dataDir);
    if (existing && processAlive(existing.pid)) {
      this.lock = existing;
      return existing;
    }
    const serverPath = this.options.serverPath().trim();
    if (!serverPath) {
      throw new CoreError(
        "Il nucleo di Progetto AI non è attivo. Apri l'app desktop, oppure indica nell'impostazione " +
          "“progettoAi.serverPath” il percorso di app-server.exe per avviarlo senza app.",
        0,
      );
    }
    this.options.log(`Avvio del nucleo senza interfaccia: ${serverPath}`);
    try {
      const child = spawn(serverPath, ["--data-dir", dataDir], {
        detached: true,
        stdio: "ignore",
        windowsHide: true,
      });
      child.on("error", (e) => this.options.log(`Avvio non riuscito: ${e.message}`));
      child.unref();
    } catch (e) {
      throw new CoreError(`Impossibile avviare ${serverPath}: ${(e as Error).message}`, 0);
    }
    this.spawnedByUs = true;
    for (let i = 0; i < 100; i++) {
      await sleep(200);
      const lock = readLock(dataDir);
      if (lock && processAlive(lock.pid)) {
        this.lock = lock;
        return lock;
      }
    }
    throw new CoreError("Il nucleo senza interfaccia non è partito entro 20 secondi.", 0);
  }

  private async fetch(path: string, init: RequestInit, retry = true): Promise<Response> {
    const lock = await this.ensure();
    const headers = {
      Authorization: `Bearer ${lock.token}`,
      "Content-Type": "application/json",
      ...(init.headers as Record<string, string> | undefined),
    };
    let response: Response;
    try {
      response = await fetch(`http://127.0.0.1:${lock.port}${path}`, { ...init, headers });
    } catch (e) {
      if ((e as Error).name === "AbortError") throw e;
      // Il nucleo può essere cambiato (l'app ha preso il posto di app-server):
      // si rilegge il file di blocco e si riprova una volta.
      this.lock = null;
      if (retry) return this.fetch(path, init, false);
      throw new CoreError(`Il nucleo non risponde: ${(e as Error).message}`, 0);
    }
    if (response.status === 401 && retry) {
      // Token rigenerato dall'app: è già nel nuovo file di blocco.
      this.lock = null;
      return this.fetch(path, init, false);
    }
    if (!response.ok) {
      const text = await response.text();
      let message = text;
      try {
        message = JSON.parse(text).error?.message ?? text;
      } catch {
        // Non è JSON: si mostra il testo così com'è.
      }
      throw new CoreError(message || `Errore ${response.status}`, response.status);
    }
    return response;
  }

  async request<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
    const response = await this.fetch(path, {
      method,
      body: body === undefined ? undefined : JSON.stringify(body),
      signal,
    });
    if (response.status === 204 || response.status === 202) return undefined as T;
    return (await response.json()) as T;
  }

  /** Una richiesta che risponde con un flusso SSE. */
  async stream(path: string, body: unknown, onEvent: SseHandler, signal?: AbortSignal): Promise<void> {
    const response = await this.fetch(path, { method: "POST", body: JSON.stringify(body), signal });
    if (!response.body) throw new CoreError("Risposta vuota dal nucleo.", 0);
    await readSse(response.body, onEvent);
  }

  /** Chiude il nucleo senza interfaccia, se l'ha avviato questa estensione. */
  async shutdownIfOurs(): Promise<void> {
    if (!this.spawnedByUs || !this.lock || this.lock.kind !== "server") return;
    try {
      await this.request("POST", "/api/shutdown");
    } catch {
      // Già chiuso o sostituito dall'app: niente da fare.
    }
  }
}
