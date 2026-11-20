// @vitest-environment happy-dom
//
// Prove dei percorsi principali dell'interfaccia: chat con risposta in
// streaming e richiesta di permesso, caricamento di un modello locale, agente
// che aspetta una conferma. Al posto del nucleo c'è un finto `invoke` di
// Tauri che risponde come farebbe l'app vera; si controllano i comandi
// mandati e ciò che compare sullo schermo.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Handler = (args: Record<string, unknown>) => unknown;

const backend = vi.hoisted(() => ({
  handlers: {} as Record<string, Handler>,
  calls: [] as { cmd: string; args: Record<string, unknown> }[],
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
    backend.calls.push({ cmd, args });
    const handler = backend.handlers[cmd];
    if (handler) return handler(args);
    // Comandi che non contano per la prova: elenchi vuoti, nient'altro.
    return cmd.startsWith("list_") ? [] : null;
  },
  Channel: class {
    onmessage: (event: unknown) => void = () => {};
  },
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

const { ChatArea } = await import("./areas/ChatArea");
const { ModelsArea } = await import("./areas/ModelsArea");
const { AgentsArea } = await import("./areas/AgentsArea");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// ── Dati finti ──────────────────────────────────────────────────────────

const SAMPLING = { temperature: 0.7, topP: 0.9, topK: 40, minP: 0.05, repeatPenalty: 1.1, maxTokens: null, seed: null, stop: [] };
const LOAD = { placement: "vram", contextLength: 4096, kvCacheType: "q8_0", gpuLayers: null, threads: null, batchSize: null, vision: false };
const PRESET = { sampling: SAMPLING, load: LOAD, systemPrompt: "" };
const TOOLS = { files: true, web: false, terminal: false, documents: false, permission: "ask" };
const STATUS = {
  loading: false,
  loaded: { modelPath: "C:\\modelli\\prova.gguf", modelName: "Prova 7B", params: LOAD, port: 1, pid: 1, engine: "b1", vision: false },
  memory: { ramTotalBytes: 16 * 2 ** 30, ramAvailableBytes: 8 * 2 ** 30, gpu: null },
  processRamBytes: null,
  processGpu: null,
  vramSpillBytes: null,
};

let nextId = 100;
function message(conversationId: number, role: string, content: string, extra: object = {}) {
  return {
    id: nextId++,
    conversationId,
    role,
    content,
    model: role === "assistant" ? "Prova 7B" : null,
    createdAt: "2026-11-20 10:00:00",
    promptTokens: null,
    completionTokens: null,
    costUsd: null,
    toolCalls: [],
    toolCallId: null,
    toolName: null,
    isError: false,
    attachments: [],
    parentId: null,
    siblings: [],
    ...extra,
  };
}

// ── Montaggio ───────────────────────────────────────────────────────────

let root: Root;
let container: HTMLElement;

async function render(element: React.ReactElement) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => root.render(element));
  await settle();
}

/** Lascia finire promesse e timer brevi (lo streaming si mostra ogni 60 ms). */
async function settle(ms = 100) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

const text = () => container.textContent ?? "";
const button = (label: string) => {
  const found = [...container.querySelectorAll("button")].find((b) => b.textContent?.trim() === label);
  if (!found) throw new Error(`manca il pulsante «${label}»; c'è: ${text()}`);
  return found;
};
async function click(element: HTMLElement) {
  await act(async () => element.click());
  await settle();
}
async function type(element: HTMLTextAreaElement | HTMLInputElement, value: string) {
  // React segue il valore impostato con il setter nativo e l'evento input.
  const proto = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(element, value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const calls = (cmd: string) => backend.calls.filter((c) => c.cmd === cmd);

beforeEach(() => {
  backend.handlers = {};
  backend.calls = [];
});
afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
});

// ── Chat ────────────────────────────────────────────────────────────────

describe("chat", () => {
  it("invia un messaggio, mostra la risposta in arrivo e chiede il permesso per uno strumento", async () => {
    const saved: ReturnType<typeof message>[] = [];
    let approve: (approval: string) => void = () => {};
    Object.assign(backend.handlers, {
      list_conversations: () => [{ id: 1, title: "Prova", createdAt: "", updatedAt: "" }],
      get_messages: () => saved,
      granted_folders: () => ["C:\\dati"],
      conversation_tools: () => TOOLS,
      tool_defaults: () => TOOLS,
      answer_tool_request: ({ approval }: Record<string, unknown>) => approve(String(approval)),
      send_message: async ({ text: sent, onEvent }: Record<string, unknown>) => {
        const channel = onEvent as { onmessage: (e: unknown) => void };
        saved.push(message(1, "user", String(sent)));
        channel.onmessage({ type: "text", text: "Leggo il file " });
        channel.onmessage({ type: "text", text: "spesa.txt…" });
        await new Promise((resolve) => setTimeout(resolve, 150));
        // Il modello chiede di scrivere un file: si aspetta la risposta dell'utente.
        const answered = new Promise<string>((resolve) => (approve = resolve));
        channel.onmessage({
          type: "toolRequest",
          id: "c1:call_1",
          name: "write_file",
          summary: "Scrivere il file spesa.txt (5 caratteri)",
          arguments: { path: "spesa.txt", content: "latte" },
        });
        expect(await answered).toBe("once");
        saved.push(message(1, "assistant", "Fatto: ho scritto spesa.txt."));
        return {
          conversation: { id: 1, title: "Prova", createdAt: "", updatedAt: "" },
          assistantMessage: saved[saved.length - 1],
          stats: { promptTokens: 10, completionTokens: 5, tokensPerSecond: 20, cacheReadTokens: null, cacheWriteTokens: null },
          cancelled: false,
          notice: null,
        };
      },
    });

    await render(
      <ChatArea
        status={STATUS as never}
        providers={[]}
        activeModel={{ kind: "local" }}
        onModelChange={() => {}}
        preset={PRESET as never}
        onPresetChange={() => {}}
        onOpenModels={() => {}}
        onOpenSettings={() => {}}
      />,
    );
    expect(text()).toContain("Scrivi un messaggio per iniziare.");

    const box = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Messaggio"]')!;
    await type(box, "Aggiungi il latte alla lista");
    // La risposta si conclude solo dopo la conferma: non si aspetta il clic.
    act(() => button("Invia").click());
    await settle(250);

    // Il messaggio dell'utente e il testo in arrivo sono già sullo schermo.
    expect(text()).toContain("Aggiungi il latte alla lista");
    expect(text()).toContain("Leggo il file spesa.txt…");
    const request = container.querySelector('[role="alertdialog"]');
    expect(request?.textContent).toContain("Scrivere il file spesa.txt");
    expect(request?.textContent).toContain("latte");
    expect(button("Interrompi")).toBeTruthy();

    await click(button("Consenti"));
    await settle(200);
    expect(calls("answer_tool_request")[0].args).toEqual({ id: "c1:call_1", approval: "once" });
    expect(container.querySelector('[role="alertdialog"]')).toBeNull();
    expect(text()).toContain("Fatto: ho scritto spesa.txt.");
    expect(button("Invia")).toBeTruthy();

    const sent = calls("send_message")[0].args;
    expect(sent).toMatchObject({ conversationId: 1, text: "Aggiungi il latte alla lista", model: { kind: "local" } });
    expect(container.querySelector('[role="log"]')?.getAttribute("aria-busy")).toBe("false");
  }, 30_000);

  it("senza modello caricato non lascia scrivere e manda ai modelli", async () => {
    const openModels = vi.fn();
    await render(
      <ChatArea
        status={{ ...STATUS, loaded: null } as never}
        providers={[]}
        activeModel={{ kind: "local" }}
        onModelChange={() => {}}
        preset={null}
        onPresetChange={() => {}}
        onOpenModels={openModels}
        onOpenSettings={() => {}}
      />,
    );
    expect(container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Messaggio"]')!.disabled).toBe(true);
    await click(button("Vai ai modelli"));
    expect(openModels).toHaveBeenCalled();
  });
});

// ── Modelli ─────────────────────────────────────────────────────────────

describe("modelli locali", () => {
  it("apre un modello, mostra la stima della memoria e lo carica", async () => {
    let loaded = false;
    Object.assign(backend.handlers, {
      model_folders: () => ["C:\\modelli"],
      engine_status: () => ({
        installed: "b1-cuda",
        engines: [{ dirName: "b1-cuda", variant: "cuda", exe: "llama-server.exe" }],
        selected: "cuda",
        build: "b1",
        recommended: "cuda",
        downloadBytes: 1,
        variants: [],
      }),
      preferences: () => ({ theme: "system", keepRunning: true, idleUnloadMinutes: 0, checkUpdates: true, onboarded: true }),
      list_models: () => [
        {
          path: "C:\\modelli\\prova.gguf",
          name: "Prova 7B",
          sizeBytes: 4 * 2 ** 30,
          sizeLabel: "7B",
          quantization: "Q4_K_M",
          architecture: "qwen2",
          contextLength: 32768,
          blockCount: 28,
          parts: 1,
          mmproj: null,
          error: null,
        },
      ],
      get_preset: () => PRESET,
      check_load: () => ({
        estimate: { modelBytes: 4 * 2 ** 30, kvCacheBytes: 2 ** 28, overheadBytes: 2 ** 28, totalBytes: 4.5 * 2 ** 30, vramBytes: 0, ramBytes: 0 },
        availableBytes: 11 * 2 ** 30,
        ramAvailableBytes: null,
        fits: true,
      }),
      load_model: () => {
        loaded = true;
        return STATUS.loaded;
      },
    });
    const onStatusChange = vi.fn();
    await render(
      <ModelsArea status={{ ...STATUS, loaded: null } as never} onStatusChange={onStatusChange} providers={[]} onProvidersChange={() => {}} />,
    );

    const card = [...container.querySelectorAll<HTMLButtonElement>("button.card-head")].find((b) =>
      b.textContent?.includes("Prova 7B"),
    )!;
    expect(card.getAttribute("aria-expanded")).toBe("false");
    await click(card);
    expect(card.getAttribute("aria-expanded")).toBe("true");
    expect(calls("check_load").length).toBeGreaterThan(0);

    await click(button("Carica"));
    expect(loaded).toBe(true);
    expect(calls("save_preset")[0].args).toMatchObject({ modelPath: "C:\\modelli\\prova.gguf" });
    expect(calls("load_model")[0].args).toMatchObject({ modelPath: "C:\\modelli\\prova.gguf", force: false });
    expect(onStatusChange).toHaveBeenCalled();
  });
});

// ── Agenti ──────────────────────────────────────────────────────────────

describe("agenti", () => {
  it("una sessione in attesa mostra la richiesta e la conferma arriva al nucleo", async () => {
    const session = {
      id: 7,
      agentId: 1,
      agentName: "Scrittore",
      conversationId: 70,
      task: "Scrivi le note",
      status: "waiting",
      note: null,
      createdAt: "2026-11-20 10:00:00",
      updatedAt: "2026-11-20 10:01:00",
      definition: null,
      restorePoints: [],
      parentSessionId: null,
    };
    let waiting = true;
    Object.assign(backend.handlers, {
      list_agent_sessions: () => [session],
      get_agent_session: () => ({ ...session, status: waiting ? "waiting" : "running" }),
      agent_session_live: () => ({
        status: waiting ? "waiting" : "running",
        streaming: "",
        reasoning: "",
        revision: 1,
        request: waiting
          ? { id: "s7:call_9", name: "run_command", summary: "Eseguire: git status", arguments: { command: "git status" } }
          : null,
      }),
      get_messages: () => [message(70, "user", "Scrivi le note")],
      answer_tool_request: () => {
        waiting = false;
      },
    });

    await render(<AgentsArea status={STATUS as never} providers={[]} onOpenModels={() => {}} />);
    await settle(200);
    const request = container.querySelector('[role="alertdialog"]');
    expect(request?.textContent).toContain("L'agente chiede il permesso di:");
    expect(request?.textContent).toContain("Eseguire: git status");

    await click(button("Consenti sempre in questa sessione"));
    expect(calls("answer_tool_request")[0].args).toEqual({ id: "s7:call_9", approval: "always" });
    await settle(200);
    expect(container.querySelector('[role="alertdialog"]')).toBeNull();
  });
});
