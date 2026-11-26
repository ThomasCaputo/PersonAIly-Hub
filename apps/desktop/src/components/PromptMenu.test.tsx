// @vitest-environment happy-dom
//
// Suggerimenti per gli argomenti dei prompt MCP: si chiedono al server mentre si
// scrive, si scelgono con frecce e Invio (che allora non inserisce il prompt), e
// le risposte vecchie non prendono il posto di quelle nuove.

import { act, createRef } from "react";
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
    return cmd.startsWith("list_") || cmd === "plugin_commands" ? [] : null;
  },
  Channel: class {},
}));

const { PromptMenu } = await import("./PromptMenu");
type Handle = import("./PromptMenu").PromptMenuHandle;

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PROMPT = {
  server: 0,
  serverName: "Remoto",
  name: "revisione",
  description: "Rivedi un testo",
  arguments: [
    { name: "tono", description: "", required: true },
    { name: "lingua", description: "", required: false },
  ],
  completions: true,
};

let container: HTMLDivElement;
let root: Root;
let draft: string;

const wait = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

function render() {
  const ref = createRef<Handle>();
  const draw = () =>
    root.render(
      <PromptMenu
        ref={ref}
        conversationId={1}
        servers={["s1"]}
        draft={draft}
        setDraft={(text) => {
          draft = text;
          draw();
        }}
        onError={() => {}}
      />,
    );
  draw();
  return ref;
}

function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  setter.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function key(target: Element, name: string) {
  target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true }));
}

const options = () => [...container.querySelectorAll(".prompt-suggestions [role=option]")].map((o) => o.textContent);
const completionCalls = () => backend.calls.filter((c) => c.cmd === "conversation_prompt_completion");

async function openForm() {
  const ref = render();
  await wait(0);
  await act(async () => {
    ref.current!.handleKey({ key: "Enter", preventDefault() {} } as never);
  });
  await wait(0);
  return container.querySelector<HTMLInputElement>("#prompt-arg-tono")!;
}

beforeEach(() => {
  backend.calls = [];
  backend.handlers = {
    conversation_prompts: () => [PROMPT],
    conversation_prompt_completion: (args) =>
      args.argument === "tono" ? ["formale", "fortissimo", "informale"].filter((v) => v.includes(String(args.value))) : [],
    conversation_prompt: (args) => `Testo con ${JSON.stringify(args.arguments)}`,
  };
  draft = "/";
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("suggerimenti degli argomenti dei prompt MCP", () => {
  it("si chiedono mentre si scrive e Invio sceglie quello evidenziato senza inserire il prompt", async () => {
    const input = await openForm();
    expect(input).toBeTruthy();
    await wait(250);
    // All'apertura il primo campo ha il fuoco: suggerimenti per il valore vuoto.
    expect(options()).toEqual(["formale", "fortissimo", "informale"]);

    await act(async () => type(input, "for"));
    await wait(250);
    const last = completionCalls().at(-1)!.args;
    expect(last).toMatchObject({ conversationId: 1, server: 0, name: "revisione", argument: "tono", value: "for" });
    expect(options()).toEqual(["formale", "fortissimo", "informale"]);

    await act(async () => key(input, "ArrowDown"));
    await act(async () => key(input, "ArrowDown"));
    expect(input.getAttribute("aria-activedescendant")).toBe("prompt-arg-tono-value-1");
    await act(async () => key(input, "Enter"));
    expect(input.value).toBe("fortissimo");
    expect(options()).toEqual([]);
    // Il prompt non è stato inserito: Invio ha solo scelto il valore.
    expect(backend.calls.some((c) => c.cmd === "conversation_prompt")).toBe(false);
    expect(draft).toBe("/");

    // Senza suggerimento evidenziato, Invio inserisce il prompt come prima.
    await act(async () => key(input, "Enter"));
    await wait(0);
    expect(draft).toContain("fortissimo");
  });

  it("gli altri argomenti compilati vanno al server come contesto", async () => {
    const input = await openForm();
    await act(async () => type(input, "formale"));
    const lingua = container.querySelector<HTMLInputElement>("#prompt-arg-lingua")!;
    await act(async () => lingua.focus());
    await act(async () => type(lingua, "it"));
    await wait(250);
    expect(completionCalls().at(-1)!.args).toMatchObject({ argument: "lingua", value: "it", context: { tono: "formale" } });
  });

  it("si chiede solo dopo una pausa e una risposta vecchia non sostituisce quella nuova", async () => {
    let release: (v: string[]) => void = () => {};
    const input = await openForm();
    await wait(250);
    backend.calls = [];
    backend.handlers.conversation_prompt_completion = (args) =>
      args.value === "f" ? new Promise<string[]>((r) => (release = r)) : ["formale"];

    await act(async () => type(input, "f"));
    await wait(250);
    await act(async () => type(input, "fo"));
    await act(async () => type(input, "for"));
    await wait(250);
    // Il server non ha ancora risposto a "f": "for" aspetta, non si accoda nel nucleo.
    expect(completionCalls().map((c) => c.args.value)).toEqual(["f"]);
    await act(async () => release(["vecchio"]));
    await wait(0);
    // La risposta a "f" si scarta; poi parte "for" ("fo", nella stessa pausa, mai).
    expect(completionCalls().map((c) => c.args.value)).toEqual(["f", "for"]);
    expect(options()).toEqual(["formale"]);
  });

  it("Esc chiude prima l'elenco, poi il modulo", async () => {
    const input = await openForm();
    await wait(250);
    expect(options().length).toBe(3);
    await act(async () => key(input, "Escape"));
    expect(options()).toEqual([]);
    expect(container.querySelector("#prompt-arg-tono")).not.toBeNull();
    await act(async () => key(input, "Escape"));
    expect(container.querySelector("#prompt-arg-tono")).toBeNull();
  });

  it("un server senza suggerimenti non riceve richieste", async () => {
    backend.handlers.conversation_prompts = () => [{ ...PROMPT, completions: false }];
    const input = await openForm();
    await act(async () => type(input, "for"));
    await wait(250);
    expect(completionCalls()).toEqual([]);
    expect(input.getAttribute("role")).toBeNull();
  });
});
