// Tipi e chiamate verso il nucleo. I tipi rispecchiano quelli Rust di `app-core`.

import { Channel, invoke } from "@tauri-apps/api/core";

export type Placement = "vram" | "ram";
export type KvCacheType = "f16" | "q8_0" | "q4_0";

export type LoadParams = {
  placement: Placement;
  contextLength: number;
  kvCacheType: KvCacheType;
  threads: number | null;
  batchSize: number | null;
  loadMode: string | null;
};

export type SamplingParams = {
  temperature: number;
  topP: number;
  topK: number;
  minP: number;
  repeatPenalty: number;
  seed: number | null;
  maxTokens: number | null;
  stop: string[];
};

export type Preset = {
  load: LoadParams;
  sampling: SamplingParams;
  systemPrompt: string;
};

export type ModelInfo = {
  path: string;
  fileName: string;
  name: string;
  sizeBytes: number;
  architecture: string | null;
  sizeLabel: string | null;
  quantization: string | null;
  contextLength: number | null;
  error: string | null;
};

export type LoadCheck = {
  estimate: {
    weightsBytes: number;
    kvCacheBytes: number;
    overheadBytes: number;
    totalBytes: number;
    kvCacheKnown: boolean;
  };
  availableBytes: number | null;
  fits: boolean;
};

export type LoadedModel = {
  modelPath: string;
  modelName: string;
  params: LoadParams;
  port: number;
  pid: number;
};

export type RuntimeStatus = {
  loading: boolean;
  loaded: LoadedModel | null;
  memory: {
    ramTotalBytes: number;
    ramAvailableBytes: number;
    gpu: { name: string; totalBytes: number; usedBytes: number; freeBytes: number } | null;
  };
  processRamBytes: number | null;
};

export type Conversation = { id: number; title: string; createdAt: string; updatedAt: string };

export type Message = {
  id: number;
  conversationId: number;
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  model: string | null;
  createdAt: string;
};

export type GenerationStats = {
  promptTokens: number | null;
  completionTokens: number | null;
  tokensPerSecond: number | null;
};

export type SendOutcome = {
  conversation: Conversation;
  assistantMessage: Message | null;
  stats: GenerationStats;
  cancelled: boolean;
};

export type StreamEvent = { type: "text"; text: string } | { type: "reasoning"; text: string };

export type CoreInfo = { version: string; schemaVersion: number };

export const api = {
  coreInfo: () => invoke<CoreInfo>("core_info"),

  modelFolders: () => invoke<string[]>("model_folders"),
  setModelFolders: (folders: string[]) => invoke<void>("set_model_folders", { folders }),
  listModels: () => invoke<ModelInfo[]>("list_models"),
  getPreset: (modelPath: string) => invoke<Preset>("get_preset", { modelPath }),
  savePreset: (modelPath: string, preset: Preset) =>
    invoke<void>("save_preset", { modelPath, preset }),
  checkLoad: (modelPath: string, params: LoadParams) =>
    invoke<LoadCheck>("check_load", { modelPath, params }),
  loadModel: (modelPath: string, params: LoadParams, force: boolean) =>
    invoke<LoadedModel>("load_model", { modelPath, params, force }),
  unloadModel: () => invoke<void>("unload_model"),
  runtimeStatus: () => invoke<RuntimeStatus>("runtime_status"),

  listConversations: () => invoke<Conversation[]>("list_conversations"),
  createConversation: () => invoke<Conversation>("create_conversation"),
  renameConversation: (id: number, title: string) =>
    invoke<void>("rename_conversation", { id, title }),
  deleteConversation: (id: number) => invoke<void>("delete_conversation", { id }),
  getMessages: (conversationId: number) => invoke<Message[]>("get_messages", { conversationId }),

  sendMessage: (
    conversationId: number,
    text: string,
    sampling: SamplingParams,
    systemPrompt: string,
    onEvent: (event: StreamEvent) => void,
  ) => {
    const channel = new Channel<StreamEvent>();
    channel.onmessage = onEvent;
    return invoke<SendOutcome>("send_message", {
      conversationId,
      text,
      sampling,
      systemPrompt,
      onEvent: channel,
    });
  },
  stopGeneration: () => invoke<void>("stop_generation"),
};

const GIB = 1024 ** 3;

/** Byte in gigabyte con una cifra decimale, alla maniera italiana. */
export function gb(bytes: number): string {
  return (bytes / GIB).toLocaleString("it-IT", {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });
}
