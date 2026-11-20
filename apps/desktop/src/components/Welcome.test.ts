import { describe, expect, it, vi } from "vitest";

// Il modulo importa le API di Tauri: qui bastano dei sostituti vuoti.
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock("@tauri-apps/api/path", () => ({ documentDir: vi.fn(), join: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { pickQ4, suggestModel } = await import("./Welcome");

const GIB = 1024 ** 3;
const memory = (vramGb: number | null, ramGb: number) => ({
  ramTotalBytes: ramGb * GIB,
  ramAvailableBytes: ramGb * GIB,
  gpu:
    vramGb === null
      ? null
      : { name: "GPU", totalBytes: vramGb * GIB, usedBytes: 0, freeBytes: vramGb * GIB },
});

describe("primo avvio: modello consigliato", () => {
  it("con una GPU NVIDIA sceglie in base alla VRAM", () => {
    expect(suggestModel(memory(12, 16))).toMatchObject({ inVram: true, suggestion: { label: "Qwen2.5 14B Instruct" } });
    expect(suggestModel(memory(8, 16)).suggestion.label).toBe("Qwen2.5 7B Instruct");
    expect(suggestModel(memory(4, 16)).suggestion.label).toBe("Qwen2.5 3B Instruct");
  });

  it("senza GPU (o con poca VRAM) usa la RAM, lasciando spazio al sistema", () => {
    // (16 - 6) / 2 = 5 GB: il 3B.
    expect(suggestModel(memory(null, 16))).toMatchObject({ inVram: false, suggestion: { label: "Qwen2.5 3B Instruct" } });
    expect(suggestModel(memory(2, 32)).suggestion.label).toBe("Qwen2.5 14B Instruct");
    expect(suggestModel(memory(null, 8)).suggestion.label).toBe("Qwen2.5 1.5B Instruct");
  });

  it("prende il file Q4_K_M, non il componente visivo", () => {
    const file = (path: string, kind: "model" | "vision" = "model") => ({
      path,
      sizeBytes: 1,
      sha256: null,
      parts: [],
      kind,
    });
    const files = [file("mmproj-Q4_K_M.gguf", "vision"), file("m-Q8_0.gguf"), file("m-Q4_K_M.gguf")];
    expect(pickQ4(files)?.path).toBe("m-Q4_K_M.gguf");
    expect(pickQ4([file("m-Q8_0.gguf")])).toBeNull();
  });
});
