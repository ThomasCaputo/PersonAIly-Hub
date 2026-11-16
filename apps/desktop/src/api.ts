// Tipi e chiamate verso il nucleo. I tipi rispecchiano quelli Rust di `app-core`.

import { Channel, invoke } from "@tauri-apps/api/core";
import { locale } from "./i18n";

export type Placement = "vram" | "ram";

/** Stato dell'API locale avviata dall'app. */
export type ApiInfo = {
  running: boolean;
  port: number | null;
  token: string | null;
  error: string | null;
};
export type KvCacheType = "f16" | "q8_0" | "q4_0";

export type LoadParams = {
  placement: Placement;
  contextLength: number;
  kvCacheType: KvCacheType;
  threads: number | null;
  batchSize: number | null;
  loadMode: string | null;
  /** Carica anche il componente visivo (mmproj), se accanto al modello ce n'è uno. */
  vision: boolean;
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
  /** Componente visivo nella stessa cartella: il modello può leggere le immagini. */
  mmproj: string | null;
  mmprojBytes: number | null;
  /** In quanti file è diviso il modello (1 = un file solo). */
  parts: number;
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
  /** Versione di llama.cpp che lo esegue (`b12110-cuda`). */
  engine: string;
  /** Caricato con il componente visivo: legge le immagini. */
  vision: boolean;
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
  processGpu: { dedicatedBytes: number; sharedBytes: number } | null;
  /** Presente se il modello in VRAM sta sconfinando in RAM. */
  vramSpillBytes: number | null;
};

export type Conversation = { id: number; title: string; createdAt: string; updatedAt: string };

export type Message = {
  id: number;
  conversationId: number;
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  model: string | null;
  createdAt: string;
  /** Token letti dal modello per questa risposta (tutta la conversazione fin lì). */
  promptTokens: number | null;
  completionTokens: number | null;
  /** Costo stimato in dollari; assente per i modelli locali o senza prezzo noto. */
  costUsd: number | null;
  /** Strumenti chiesti dal modello in questa risposta. */
  toolCalls: ToolCall[];
  /** Per un messaggio `tool`: la chiamata di cui è il risultato. */
  toolCallId: string | null;
  toolName: string | null;
  /** Per un messaggio `tool`: lo strumento è fallito o è stato negato. */
  isError: boolean;
  attachments: AttachmentInfo[];
};

export type ToolCall = { id: string; name: string; arguments: unknown };
export type AttachmentInfo = { name: string; kind: "text" | "image" };

/** Quanto chiedere prima di lasciar agire il modello. */
export type PermissionLevel = "ask" | "edits" | "all";

/** Strumenti e permessi di una conversazione o di un agente. Il terminale vale solo per gli agenti. */
export type ToolSettings = {
  files: boolean;
  web: boolean;
  terminal: boolean;
  permission: PermissionLevel;
  /** Strumenti MCP con "Consenti sempre". */
  allowedTools?: string[];
};

/** Un server MCP configurato. `id` vuoto per uno non ancora salvato. */
export type McpServer = {
  id: string;
  name: string;
  command: string;
  args: string[];
  /** Coppie [nome, valore]. */
  env: [string, string][];
};

export type McpToolInfo = { name: string; description: string };

/** Nome leggibile di uno strumento MCP (`mcp__server__strumento`), oppure null. */
export function mcpToolLabel(name: string): string | null {
  if (!name.startsWith("mcp__")) return null;
  const [server, ...tool] = name.slice(5).split("__");
  return `MCP ${server.replace(/_/g, " ")}: ${tool.join("__")}`;
}

/** La definizione di un agente. `id` 0 per uno non ancora salvato. */
export type Agent = {
  id: number;
  name: string;
  description: string;
  instructions: string;
  model: ModelRef;
  folders: string[];
  tools: ToolSettings;
  /** Server MCP (identificatori) avviati con ogni sessione. */
  mcpServers: string[];
  maxSteps: number;
};

export type SessionStatus = "running" | "paused" | "waiting" | "done" | "stopped" | "failed" | "interrupted";

export type AgentSession = {
  id: number;
  agentId: number | null;
  agentName: string;
  conversationId: number;
  task: string;
  status: SessionStatus;
  /** Errore o avviso con cui la sessione si è fermata. */
  note: string | null;
  createdAt: string;
  updatedAt: string;
};

/** Ciò che una sessione al lavoro sta facendo in questo momento. */
export type LiveState = {
  status: SessionStatus | null;
  streaming: string;
  reasoning: string;
  request: ToolRequest | null;
  /** Cambia a ogni nuovo messaggio salvato. */
  revision: number;
};

/** Una voce del registro delle azioni. */
export type ActionRecord = {
  id: number;
  createdAt: string;
  conversationId: number | null;
  sessionId: number | null;
  kind: "chat" | "agent";
  source: string;
  tool: string;
  summary: string;
  approval: "auto" | "user" | "denied" | "refused" | "interrupted";
  isError: boolean;
  result: string;
};

/** Le sessioni che stanno ancora lavorando (o aspettano di poterlo fare). */
export const isActive = (status: SessionStatus) => status === "running" || status === "paused" || status === "waiting";

/** Risposta dell'utente a una richiesta di usare uno strumento. */
export type Approval = "deny" | "once" | "always";

export type ToolRequest = { id: string; name: string; summary: string; arguments: unknown };

/** Parametri di generazione e prompt di sistema salvati con un nome. */
export type NamedPreset = { name: string; sampling: SamplingParams; systemPrompt: string };

export type HubRepo = { id: string; downloads: number; likes: number };
export type HubPart = { path: string; sizeBytes: number; sha256: string | null };
export type HubFile = HubPart & {
  /** Le parti di un modello diviso (vuoto per un file solo). */
  parts: HubPart[];
  /** `vision`: componente visivo (mmproj) che fa leggere le immagini al modello. */
  kind: "model" | "vision";
};

export type DownloadState = {
  /** `<repo>/<file>`: più download possono procedere insieme. */
  id: string;
  repo: string;
  file: string;
  doneBytes: number;
  totalBytes: number;
  phase: "downloading" | "done" | "failed" | "cancelled";
  error: string | null;
  /** Hugging Face non ha pubblicato l'impronta: controllata solo la dimensione. */
  unverified: boolean;
};

export type Theme = "system" | "light" | "dark";

/** Preferenze generali dell'app. */
export type Preferences = {
  theme: Theme;
  /** Chiudendo la finestra il nucleo resta attivo nell'area di notifica. */
  keepRunning: boolean;
  /** Minuti senza richieste dopo cui il modello locale si scarica; 0 = mai. */
  idleUnloadMinutes: number;
};

/** Protezioni accese di base, che l'utente può spegnere nelle Impostazioni. */
export type SecuritySettings = {
  /** Conferma prima che il modello apra un indirizzo inventato mentre vede i file. */
  exfiltrationGuard: boolean;
  /** Conferma prima di aprire nel browser un collegamento di una risposta. */
  confirmLinks: boolean;
};

/** Importo in dollari: più decimali per le cifre piccole, tipiche di una singola risposta. */
export function usd(amount: number): string {
  const digits = amount >= 1 ? 2 : amount >= 0.01 ? 3 : 4;
  return `$${amount.toLocaleString(locale(), { minimumFractionDigits: digits, maximumFractionDigits: digits })}`;
}

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
  /** Avviso da mostrare sotto la risposta (troncata, rifiutata…). */
  notice: string | null;
};

export type ProviderKind = "anthropic" | "openai_compatible";

export type CloudModel = {
  id: string;
  name: string;
  contextLength: number | null;
  maxOutputTokens: number | null;
  /** Dollari per milione di token letti / generati, se noti. */
  inputPrice: number | null;
  outputPrice: number | null;
};

/** Dati di un provider che l'utente può modificare. */
export type Provider = {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  models: CloudModel[];
};

export type ProviderInfo = Provider & {
  hasKey: boolean;
  /** Parametri di generazione accettati da questo provider. */
  supportedParams: string[];
};

/** Il modello a cui inviare un messaggio. */
export type ModelRef = { kind: "local" } | { kind: "cloud"; providerId: string; model: string };

/** Chiave sotto cui sono salvati i parametri di un modello cloud. */
export function cloudPresetKey(providerId: string, model: string): string {
  return `cloud:${providerId}:${model}`;
}

export type StreamEvent =
  | { type: "text"; text: string }
  | { type: "reasoning"; text: string }
  /** Nuovi messaggi in archivio (risposta parziale, risultato di uno strumento). */
  | { type: "saved" }
  | ({ type: "toolRequest" } & ToolRequest);

export type CoreInfo = { version: string; schemaVersion: number };

export type EngineVariant = "cuda" | "vulkan" | "cpu";

export type InstalledEngine = { dirName: string; variant: EngineVariant | null };

export type EngineStatus = {
  /** Versione che userà il prossimo caricamento. */
  installed: string | null;
  engines: InstalledEngine[];
  /** Variante scelta; null finché non se ne installa una. */
  selected: EngineVariant | null;
  build: string;
  recommended: EngineVariant;
  downloadBytes: number;
  variants: { variant: EngineVariant; downloadBytes: number }[];
};

export type InstallProgress = {
  phase: "download" | "extract";
  doneBytes: number;
  totalBytes: number;
};

export const api = {
  coreInfo: () => invoke<CoreInfo>("core_info"),

  engineStatus: () => invoke<EngineStatus>("engine_status"),
  setEngineVariant: (variant: EngineVariant) => invoke<void>("set_engine_variant", { variant }),
  removeEngine: (dirName: string) => invoke<InstalledEngine[]>("remove_engine", { dirName }),
  installEngine: (variant: EngineVariant, onProgress: (progress: InstallProgress) => void) => {
    const channel = new Channel<InstallProgress>();
    channel.onmessage = onProgress;
    return invoke<void>("install_engine", { variant, onProgress: channel });
  },

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

  listConversations: (query = "") => invoke<Conversation[]>("list_conversations", { query }),
  createConversation: () => invoke<Conversation>("create_conversation"),
  renameConversation: (id: number, title: string) =>
    invoke<void>("rename_conversation", { id, title }),
  deleteConversation: (id: number) => invoke<void>("delete_conversation", { id }),
  getMessages: (conversationId: number) => invoke<Message[]>("get_messages", { conversationId }),

  sendMessage: (
    conversationId: number,
    text: string,
    attachments: string[],
    sampling: SamplingParams,
    systemPrompt: string,
    model: ModelRef,
    onEvent: (event: StreamEvent) => void,
  ) => {
    const channel = new Channel<StreamEvent>();
    channel.onmessage = onEvent;
    return invoke<SendOutcome>("send_message", {
      conversationId,
      text,
      attachments,
      sampling,
      systemPrompt,
      model,
      onEvent: channel,
    });
  },
  /** Modifica (con `text`) o ripete un tuo messaggio e rigenera la risposta; i messaggi successivi si cancellano. */
  resendMessage: (
    conversationId: number,
    messageId: number,
    text: string | null,
    sampling: SamplingParams,
    systemPrompt: string,
    model: ModelRef,
    onEvent: (event: StreamEvent) => void,
  ) => {
    const channel = new Channel<StreamEvent>();
    channel.onmessage = onEvent;
    return invoke<SendOutcome>("resend_message", {
      conversationId,
      messageId,
      text,
      sampling,
      systemPrompt,
      model,
      onEvent: channel,
    });
  },
  exportConversation: (conversationId: number, path: string, lang: string) =>
    invoke<void>("export_conversation", { conversationId, path, lang }),
  stopGeneration: (conversationId: number | null = null) => invoke<void>("stop_generation", { conversationId }),
  apiInfo: () => invoke<ApiInfo>("api_info"),
  regenerateApiToken: () => invoke<string>("regenerate_api_token"),
  answerToolRequest: (id: string, approval: Approval) => invoke<void>("answer_tool_request", { id, approval }),

  grantedFolders: () => invoke<string[]>("granted_folders"),
  setGrantedFolders: (folders: string[]) => invoke<void>("set_granted_folders", { folders }),
  toolDefaults: () => invoke<ToolSettings>("tool_defaults"),
  setToolDefaults: (settings: ToolSettings) => invoke<void>("set_tool_defaults", { settings }),
  conversationTools: (conversationId: number) => invoke<ToolSettings>("conversation_tools", { conversationId }),
  setConversationTools: (conversationId: number, settings: ToolSettings) =>
    invoke<void>("set_conversation_tools", { conversationId, settings }),

  listActions: (limit: number, sessionId: number | null = null) =>
    invoke<ActionRecord[]>("list_actions", { limit, sessionId }),
  /** Per quanti giorni si tengono le voci del registro; 0 = per sempre. */
  actionLogDays: () => invoke<number>("action_log_days"),
  /** Restituisce quante voci troppo vecchie sono state eliminate. */
  setActionLogDays: (days: number) => invoke<number>("set_action_log_days", { days }),
  clearActions: () => invoke<number>("clear_actions"),

  listMcpServers: () => invoke<McpServer[]>("list_mcp_servers"),
  saveMcpServer: (server: McpServer) => invoke<McpServer>("save_mcp_server", { server }),
  deleteMcpServer: (id: string) => invoke<void>("delete_mcp_server", { id }),
  probeMcpServer: (server: McpServer) => invoke<McpToolInfo[]>("probe_mcp_server", { server }),

  listAgents: () => invoke<Agent[]>("list_agents"),
  saveAgent: (agent: Agent) => invoke<Agent>("save_agent", { agent }),
  duplicateAgent: (id: number) => invoke<Agent>("duplicate_agent", { id }),
  deleteAgent: (id: number) => invoke<void>("delete_agent", { id }),
  startAgentSession: (agentId: number, task: string) =>
    invoke<AgentSession>("start_agent_session", { agentId, task }),
  replyAgentSession: (id: number, text: string) => invoke<AgentSession>("reply_agent_session", { id, text }),
  listAgentSessions: () => invoke<AgentSession[]>("list_agent_sessions"),
  getAgentSession: (id: number) => invoke<AgentSession>("get_agent_session", { id }),
  agentSessionLive: (id: number) => invoke<LiveState | null>("agent_session_live", { id }),
  pauseAgentSession: (id: number) => invoke<void>("pause_agent_session", { id }),
  resumeAgentSession: (id: number) => invoke<void>("resume_agent_session", { id }),
  stopAgentSession: (id: number) => invoke<void>("stop_agent_session", { id }),
  deleteAgentSession: (id: number) => invoke<void>("delete_agent_session", { id }),

  listNamedPresets: () => invoke<NamedPreset[]>("list_named_presets"),
  saveNamedPreset: (preset: NamedPreset) => invoke<NamedPreset>("save_named_preset", { preset }),
  deleteNamedPreset: (name: string) => invoke<void>("delete_named_preset", { name }),
  exportNamedPreset: (name: string, path: string) => invoke<void>("export_named_preset", { name, path }),
  importNamedPreset: (path: string) => invoke<NamedPreset>("import_named_preset", { path }),

  searchHub: (query: string) => invoke<HubRepo[]>("search_hub", { query }),
  hubFiles: (repo: string) => invoke<HubFile[]>("hub_files", { repo }),
  startModelDownload: (repo: string, file: HubFile, folder: string) =>
    invoke<void>("start_model_download", { repo, file, folder }),
  listDownloads: () => invoke<DownloadState[]>("list_downloads"),
  cancelDownload: (id: string) => invoke<void>("cancel_download", { id }),
  dismissDownload: (id: string) => invoke<void>("dismiss_download", { id }),

  listProviders: () => invoke<ProviderInfo[]>("list_providers"),
  /** `apiKey` vuota lascia invariata la chiave già salvata. */
  saveProvider: (provider: Provider, apiKey: string) =>
    invoke<ProviderInfo>("save_provider", { provider, apiKey: apiKey || null }),
  deleteProvider: (id: string) => invoke<void>("delete_provider", { id }),
  refreshProviderModels: (id: string) => invoke<ProviderInfo>("refresh_provider_models", { id }),

  /** La lingua scelta dall'utente; `null` finché non ne sceglie una. */
  language: () => invoke<string | null>("language"),
  setLanguage: (language: string) => invoke<void>("set_language", { language }),
  preferences: () => invoke<Preferences>("preferences"),
  setPreferences: (preferences: Preferences) => invoke<void>("set_preferences", { preferences }),
  /** `true` se chiudendo la finestra il nucleo resta nell'area di notifica. */
  runsInTray: () => invoke<boolean>("runs_in_tray"),
  securitySettings: () => invoke<SecuritySettings>("security_settings"),
  setSecuritySettings: (settings: SecuritySettings) => invoke<void>("set_security_settings", { settings }),
  /** Apre un indirizzo web nel browser di sistema. */
  openExternal: (url: string) => invoke<void>("open_external", { url }),
};


/** Dimensione leggibile: megabyte sotto il gigabyte, gigabyte sopra. */
export function size(bytes: number): string {
  return bytes < GIB ? `${Math.round(bytes / 1024 ** 2)} MB` : `${gb(bytes)} GB`;
}

const GIB = 1024 ** 3;

/** Byte in gigabyte con una cifra decimale, nel formato della lingua scelta. */
export function gb(bytes: number): string {
  return (bytes / GIB).toLocaleString(locale(), {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });
}
