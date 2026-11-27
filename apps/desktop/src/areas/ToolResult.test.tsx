// @vitest-environment happy-dom
//
// La riga di un risultato mostra anche la chiamata: il pensiero di
// sequentialthinking (che il server non ripete) e gli argomenti delle altre.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Message, ToolCall } from "../api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: async () => null, Channel: class {} }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: async () => null, save: async () => null }));

const { ToolResult, callsById } = await import("./ChatArea");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const message = (over: Partial<Message>): Message =>
  ({
    id: 1,
    role: "tool",
    content: "",
    toolCalls: [],
    toolCallId: "c1",
    toolName: "mcp__x__sequentialthinking",
    isError: false,
    attachments: [],
    parentId: null,
    siblings: [],
    ...over,
  }) as Message;

let root: Root | null = null;
let host: HTMLElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
});

function render(result: Message, call?: ToolCall) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root!.render(<ToolResult message={result} call={call} />));
  return host;
}

describe("ToolResult", () => {
  it("mostra il pensiero al posto dei contatori", () => {
    const counters = '{\n  "thoughtNumber": 1,\n  "totalThoughts": 3\n}';
    const call: ToolCall = { id: "c1", name: "mcp__x__sequentialthinking", arguments: { thought: "Apro la scatola «Misto».", thoughtNumber: 1 } };
    const el = render(message({ content: counters }), call);
    expect(el.querySelector("summary")!.textContent).toContain("Apro la scatola «Misto».");
    expect(el.querySelector(".tool-thought")!.textContent).toBe("Apro la scatola «Misto».");
    expect(el.textContent).toContain('"thoughtNumber": 1');
  });

  it("mostra gli argomenti delle altre chiamate e una riga sola di anteprima", () => {
    const call: ToolCall = { id: "c1", name: "mcp__x__create_entities", arguments: { entities: [{ name: "Utente" }] } };
    const el = render(message({ content: '[\n  {\n    "name": "Utente"\n  }\n]', toolName: "mcp__x__create_entities" }), call);
    expect(el.querySelector("summary")!.textContent).toContain('[ { "name": "Utente" } ]');
    expect(el.querySelector(".tool-thought")).toBeNull();
    expect(el.querySelectorAll("pre")).toHaveLength(2);
  });

  it("senza chiamata mostra solo il risultato", () => {
    const el = render(message({ content: "Risultati per \"x\":" }));
    expect(el.querySelectorAll("pre")).toHaveLength(1);
  });

  it("ritrova le chiamate per id", () => {
    const calls = callsById([message({ role: "assistant", toolCalls: [{ id: "a", name: "n", arguments: {} }] })]);
    expect(calls.get("a")?.name).toBe("n");
    expect(calls.get("b")).toBeUndefined();
  });
});
