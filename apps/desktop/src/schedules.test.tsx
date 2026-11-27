// @vitest-environment happy-dom
//
// Prove della scheda Pianificazioni (ADR 0028): il modulo avvisa di ciò che
// verrà rifiutato, chiede il modello locale e salva; l'elenco mostra la
// ricorrenza e fa partire "Esegui ora". Al posto del nucleo c'è un finto
// `invoke` di Tauri.

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
    return cmd.startsWith("list_") ? [] : null;
  },
  Channel: class {},
}));

const { SchedulesTab, recurrenceLabel, refusedActions } = await import("./components/SchedulesTab");
import type { Agent, Schedule } from "./api";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const AGENT: Agent = {
  id: 7,
  name: "Notturno",
  description: "",
  instructions: "",
  model: { kind: "local" },
  folders: ["C:\\lavoro"],
  tools: { files: true, web: false, terminal: true, permission: "ask" },
  mcpServers: [],
  maxSteps: 50,
  maxStepTokens: 8192,
  restorePoints: true,
  delegates: [],
};
const MODEL = "C:\\modelli\\qwen.gguf";

let root: Root;
let container: HTMLElement;

async function render(element: React.ReactElement) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => root.render(element));
  await settle();
}
async function settle(ms = 50) {
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
const calls = (cmd: string) => backend.calls.filter((c) => c.cmd === cmd);

beforeEach(() => {
  backend.handlers = {};
  backend.calls = [];
});
afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
});

describe("pianificazioni", () => {
  it("la ricorrenza in parole", () => {
    expect(recurrenceLabel({ kind: "daily", time: "08:00", weekdays: [] })).toBe("Ogni giorno alle 08:00");
    expect(recurrenceLabel({ kind: "everyHours", hours: 4, start: "06:00", weekdays: [] })).toBe("Ogni 4 ore dalle 06:00");
    expect(recurrenceLabel({ kind: "daily", time: "09:30", weekdays: [1, 5] })).toMatch(/alle 09:30$/);
  });

  it("dice cosa verrà rifiutato con i permessi dell'agente", () => {
    expect(refusedActions(AGENT)).toEqual(["creare o modificare file", "comandi nel terminale"]);
    expect(refusedActions({ ...AGENT, tools: { ...AGENT.tools, permission: "edits" } })).toEqual(["comandi nel terminale"]);
    expect(refusedActions({ ...AGENT, tools: { ...AGENT.tools, permission: "all" } })).toEqual([]);
  });

  it("il modulo vuole il modello locale e salva la pianificazione", async () => {
    backend.handlers.list_models = () => [{ path: MODEL, name: "Qwen 7B" }];
    backend.handlers.save_schedule = ({ schedule }) => ({ ...(schedule as Schedule), id: 1 });
    await render(<SchedulesTab agents={[AGENT]} newForAgent={7} onEditorClosed={() => {}} onOpenSession={() => {}} />);

    expect(text()).toContain("Nuova pianificazione");
    expect(text()).toContain("verranno rifiutate: creare o modificare file, comandi nel terminale");
    const save = button("Salva");
    expect(save.disabled).toBe(true);

    const task = container.querySelector<HTMLTextAreaElement>("#schedule-task")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(task, "Riassumi i file nuovi");
      task.dispatchEvent(new Event("input", { bubbles: true }));
    });
    // Senza modello non si salva ancora.
    expect(button("Salva").disabled).toBe(true);
    const model = container.querySelector<HTMLSelectElement>("#schedule-model")!;
    await act(async () => {
      model.value = MODEL;
      model.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await click(button("Salva"));

    const [saved] = calls("save_schedule");
    expect(saved.args.schedule).toMatchObject({
      agentId: 7,
      task: "Riassumi i file nuovi",
      localModel: MODEL,
      recurrence: { kind: "daily", time: "08:00", weekdays: [] },
      onConflict: "wait",
      onMissed: "catchUp",
      enabled: true,
    });
    // Salvato, si torna all'elenco.
    expect(text()).toContain("Avvia con Windows");
  });

  it("l'elenco mostra quando parte e fa partire subito il compito", async () => {
    const schedule: Schedule = {
      id: 3,
      agentId: 7,
      task: "Controlla le dipendenze",
      recurrence: { kind: "everyHours", hours: 6, start: "00:00", weekdays: [] },
      localModel: MODEL,
      onConflict: "wait",
      onMissed: "catchUp",
      notify: "always",
      maxMinutes: 60,
      enabled: true,
      nextRun: 1_795_831_200,
      lastRun: 1_795_800_000,
      lastOutcome: "skipped",
      lastNote: "Esecuzione persa: all'ora stabilita l'app era chiusa o il PC spento.",
      running: false,
    };
    backend.handlers.list_schedules = () => [schedule];
    backend.handlers.autostart_enabled = () => false;
    backend.handlers.runs_in_tray = () => true;
    backend.handlers.preferences = () => ({ keepRunning: false });
    await render(<SchedulesTab agents={[AGENT]} newForAgent={null} onEditorClosed={() => {}} onOpenSession={() => {}} />);

    expect(text()).toContain("Ogni 6 ore dalle 00:00");
    expect(text()).toContain("Prossima esecuzione:");
    expect(text()).toContain("Saltata");
    expect(text()).toContain("qwen.gguf");
    // Chiudere la finestra chiude tutto: l'avviso c'è.
    expect(text()).toContain("le pianificazioni si fermano");

    await click(button("Esegui ora"));
    expect(calls("run_schedule_now")[0].args).toEqual({ id: 3 });

    await act(async () => container.querySelector<HTMLInputElement>(".check input")!.click());
    await settle();
    expect(calls("set_autostart")[0].args).toEqual({ enabled: true });
  });
});
