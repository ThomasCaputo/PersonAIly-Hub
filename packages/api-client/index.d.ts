// Tipi dell'API locale di PersonAIly Hub, generati dai tipi Rust del nucleo
// (crates/server/src/bindings.rs). Non modificare a mano: dopo aver cambiato
// un tipo, rigenera con `UPDATE_BINDINGS=1 cargo test -p app-server bindings`.

export type Info = { version: string, kind: InstanceKind, pid: number, };

export type InstanceKind = "app" | "server";

export type LockInfo = { port: number, token: string, pid: number, kind: InstanceKind, version: string, };

export type InstallInfo = { serverPath: string, version: string, };

export type Models = { local: LocalModels, cloud: Array<ProviderInfo>, };

export type LocalModels = { loaded: LoadedModel | null, loading: boolean, library: Array<ModelInfo>, };

export type LoadBody = { path: string, 
/**
 * Carica anche se la stima dice che non entra in memoria.
 */
force?: boolean, };

export type InfillBody = { 
/**
 * Il testo prima del cursore.
 */
prefix: string, 
/**
 * Il testo dopo il cursore.
 */
suffix: string, 
/**
 * Token massimi da generare (predefinito 128, al massimo 512).
 */
maxTokens?: number, };

export type InfillReply = { 
/**
 * Il testo da inserire al cursore.
 */
text: string, };

export type ModelInfo = { path: string, fileName: string, 
/**
 * Nome leggibile: quello scritto nel file, altrimenti il nome del file.
 */
name: string, sizeBytes: number, architecture: string | null, sizeLabel: string | null, quantization: string | null, contextLength: number | null, blockCount: number | null, embeddingLength: number | null, headCount: number | null, headCountKv: number | null, 
/**
 * Componente visivo (`mmproj-….gguf`) nella stessa cartella: con questo
 * il modello può leggere le immagini.
 */
mmproj: string | null, mmprojBytes: number | null, 
/**
 * In quanti file è diviso il modello (`-00001-of-00003.gguf`); 1 se è
 * in un file solo. `path` è sempre la prima parte.
 */
parts: number, 
/**
 * Presente se il file non si è potuto leggere.
 */
error: string | null, 
/**
 * Se e come si controlla il ragionamento ("thinking") del modello: con
 * `switch` si accende e spegne, con `effort` (gpt-oss) si riduce.
 */
thinking: ThinkingControl, };

export type ThinkingControl = "none" | "switch" | "effort";

export type LoadedModel = { modelPath: string, modelName: string, params: LoadParams, port: number, pid: number, 
/**
 * Cartella della versione di llama.cpp che lo esegue (`b12110-cuda`).
 */
engine: string, 
/**
 * Caricato con il suo componente visivo (`mmproj`): legge le immagini.
 */
vision: boolean, };

export type LoadParams = { placement: Placement, contextLength: number, kvCacheType: KvCacheType, 
/**
 * Thread della CPU; `None` lascia decidere a llama.cpp.
 */
threads: number | null, batchSize: number | null, 
/**
 * Modalità di lettura del file (`--load-mode`). `None` usa quella
 * predefinita per la collocazione scelta. Solo per prove e misure.
 */
loadMode: string | null, 
/**
 * Carica anche il componente visivo (`mmproj`) se accanto al modello ce
 * n'è uno: il modello legge le immagini, al costo di un po' di memoria.
 */
vision: boolean, 
/**
 * Solo per [`Placement::Hybrid`]: quanti layer vanno sulla GPU.
 */
gpuLayers: number | null, 
/**
 * Con `false` il modello risponde subito, senza ragionare prima
 * (`--reasoning off`), o ragiona il meno possibile se non sa farne a
 * meno (gpt-oss). Conta solo per i modelli che sanno ragionare.
 */
thinking: boolean, };

export type Placement = "vram" | "ram" | "hybrid";

export type KvCacheType = "f16" | "q8_0" | "q4_0";

export type SamplingParams = { temperature: number, topP: number, topK: number, minP: number, repeatPenalty: number, 
/**
 * `None` = casuale a ogni risposta.
 */
seed: number | null, 
/**
 * `None` = nessun limite oltre al contesto.
 */
maxTokens: number | null, stop: Array<string>, };

export type RuntimeStatus = { 
/**
 * `true` mentre un modello è in fase di caricamento.
 */
loading: boolean, loaded: LoadedModel | null, memory: MemorySnapshot, 
/**
 * RAM occupata dal processo del modello.
 */
processRamBytes: number | null, 
/**
 * Memoria grafica dedicata e condivisa del processo del modello.
 */
processGpu: ProcessGpuMemory | null, 
/**
 * Presente se un modello caricato in VRAM sta sconfinando in RAM:
 * quanta RAM il driver sta usando al posto della VRAM.
 */
vramSpillBytes: number | null, };

export type MemorySnapshot = { ramTotalBytes: number, ramAvailableBytes: number, 
/**
 * `None` se non c'è una GPU NVIDIA o `nvidia-smi` non risponde.
 */
gpu: GpuMemory | null, };

export type GpuMemory = { name: string, totalBytes: number, usedBytes: number, freeBytes: number, };

export type ProcessGpuMemory = { 
/**
 * Memoria sulla scheda video (VRAM vera).
 */
dedicatedBytes: number, 
/**
 * Memoria "condivisa": è RAM di sistema usata come se fosse VRAM. Un valore
 * alto significa che il driver sta riversando la VRAM in RAM.
 */
sharedBytes: number, };

export type ProviderInfo = { 
/**
 * La chiave API è nel portachiavi. La chiave stessa non esce mai dal nucleo.
 */
hasKey: boolean, supportedParams: Array<string>, id: string, name: string, kind: ProviderKind, 
/**
 * Indirizzo dell'API, senza barra finale. Per i servizi compatibili con
 * OpenAI comprende il `/v1`.
 */
baseUrl: string, 
/**
 * Elenco dei modelli, come scaricato l'ultima volta.
 */
models: Array<CloudModel>, };

export type Provider = { id: string, name: string, kind: ProviderKind, 
/**
 * Indirizzo dell'API, senza barra finale. Per i servizi compatibili con
 * OpenAI comprende il `/v1`.
 */
baseUrl: string, 
/**
 * Elenco dei modelli, come scaricato l'ultima volta.
 */
models: Array<CloudModel>, };

export type ProviderKind = "anthropic" | "openai_compatible";

export type CloudModel = { id: string, 
/**
 * Nome leggibile, se il provider lo fornisce.
 */
name: string, contextLength: number | null, maxOutputTokens: number | null, 
/**
 * Prezzo in dollari per milione di token letti, se noto.
 */
inputPrice: number | null, 
/**
 * Prezzo in dollari per milione di token generati, se noto.
 */
outputPrice: number | null, };

export type ModelRef = { "kind": "local" } | { "kind": "cloud", providerId: string, model: string, };

export type Conversation = { id: number, title: string, createdAt: string, updatedAt: string, };

export type RenameBody = { title: string, };

export type Message = { id: number, conversationId: number, role: string, content: string, model: string | null, createdAt: string, 
/**
 * Token letti dal modello per produrre questa risposta (tutta la conversazione fin lì).
 */
promptTokens: number | null, 
/**
 * Token generati in questa risposta.
 */
completionTokens: number | null, 
/**
 * Costo stimato della risposta in dollari; assente per i modelli locali
 * o quando il prezzo non è noto.
 */
costUsd: number | null, 
/**
 * Strumenti chiesti dal modello in questa risposta.
 */
toolCalls: Array<ToolCall>, 
/**
 * Per un messaggio `tool`: la chiamata di cui è il risultato.
 */
toolCallId: string | null, toolName: string | null, 
/**
 * Per un messaggio `tool`: lo strumento è fallito o è stato negato.
 */
isError: boolean, 
/**
 * Nome e tipo degli allegati, per l'interfaccia.
 */
attachments: Array<AttachmentInfo>, 
/**
 * Il messaggio a cui risponde; `None` per il primo.
 */
parentId: number | null, 
/**
 * Le versioni di questo messaggio nei vari rami (lui compreso), dalla più
 * vecchia. Vuoto se ce n'è una sola. Solo nei messaggi da mostrare.
 */
siblings: Array<number>, };

export type AttachmentInfo = { name: string, kind: AttachmentKind, };

export type AttachmentKind = "text" | "image";

export type ToolCall = { id: string, name: string, 
/**
 * Un oggetto JSON. Se il modello ha scritto parametri illeggibili, qui
 * c'è il testo grezzo come stringa e la chiamata fallisce senza eseguire nulla.
 */
arguments: JsonValue, };

export type SendBody = { text: string, 
/**
 * Percorsi di file da allegare.
 */
attachments?: Array<string>, model: ModelRef, 
/**
 * Assenti: si usano quelli del preset del modello.
 */
sampling?: SamplingParams, systemPrompt?: string, };

export type BranchBody = { 
/**
 * Un messaggio del ramo da mostrare.
 */
messageId: number, };

export type TextEvent = { text: string, };

export type ToolRequestEvent = { id: string, name: string, summary: string, arguments: JsonValue, };

export type ErrorEvent = { message: string, };

export type SendOutcome = { conversation: Conversation, 
/**
 * Assente se il modello non ha prodotto testo.
 */
assistantMessage: Message | null, stats: GenerationStats, cancelled: boolean, 
/**
 * Avviso da mostrare sotto la risposta (troncata, rifiutata…).
 */
notice: string | null, };

export type GenerationStats = { 
/**
 * Tutti i token letti, compresi quelli serviti dalla cache del servizio.
 */
promptTokens: number | null, completionTokens: number | null, tokensPerSecond: number | null, 
/**
 * Dei token letti, quanti sono arrivati dalla cache del servizio
 * (costano circa un decimo). Solo con Anthropic.
 */
cacheReadTokens: number | null, 
/**
 * Dei token letti, quanti sono stati scritti nella cache (costano un
 * quarto in più). Solo con Anthropic.
 */
cacheWriteTokens: number | null, };

export type AnswerBody = { id: string, approval: Approval, };

export type Approval = "deny" | "once" | "always";

export type ToolSettings = { 
/**
 * Accesso ai file delle cartelle concesse.
 */
files: boolean, 
/**
 * Ricerca web e lettura di pagine: manda all'esterno ciò che si cerca.
 */
web: boolean, 
/**
 * Comandi nel terminale. Lo usano gli agenti; in chat resta spento.
 */
terminal: boolean, 
/**
 * Ricerca nei documenti consultabili (RAG locale, ADR 0017).
 */
documents: boolean, permission: PermissionLevel, 
/**
 * Strumenti MCP per cui l'utente ha scelto "Consenti sempre".
 */
allowedTools: Array<string>, };

export type PermissionLevel = "ask" | "edits" | "all";

export type ActionRecord = { id: number, createdAt: string, conversationId: number | null, sessionId: number | null, 
/**
 * `chat` o `agent`: resta anche quando conversazione o sessione non ci sono più.
 */
kind: string, 
/**
 * Titolo della conversazione o nome dell'agente, com'era al momento dell'azione.
 */
source: string, tool: string, summary: string, 
/**
 * `auto` (consentita dal livello di permesso), `user` (confermata),
 * `denied` (negata), `refused` (fuori confine), `interrupted`.
 */
approval: string, isError: boolean, 
/**
 * Inizio del risultato.
 */
result: string, };

export type Agent = { 
/**
 * 0 per un agente non ancora salvato.
 */
id: number, name: string, description: string, 
/**
 * Istruzioni dell'utente, aggiunte al prompt di sistema.
 */
instructions: string, model: ModelRef, 
/**
 * Le cartelle su cui l'agente lavora: sono le sue cartelle concesse.
 */
folders: Array<string>, tools: ToolSettings, 
/**
 * Server MCP (identificatori) avviati con ogni sessione.
 */
mcpServers: Array<string>, 
/**
 * Passi massimi prima di fermarsi e aspettare l'utente.
 */
maxSteps: number, 
/**
 * Token massimi per passo (vedi [`DEFAULT_MAX_STEP_TOKENS`]).
 */
maxStepTokens: number, 
/**
 * Prima di ogni sessione fotografa le cartelle, per poter annullare
 * tutto ciò che l'agente ha cambiato (vedi [`crate::snapshots`]).
 */
restorePoints: boolean, 
/**
 * Gli agenti (id) a cui questo può affidare parte del lavoro (AG-8).
 */
delegates: Array<number>, };

export type AgentSession = { id: number, 
/**
 * `None` se l'agente è stato eliminato nel frattempo.
 */
agentId: number | null, agentName: string, conversationId: number, task: string, status: SessionStatus, 
/**
 * Errore o avviso con cui la sessione si è fermata.
 */
note: string | null, createdAt: string, updatedAt: string, 
/**
 * Le cartelle fotografate prima che l'agente lavorasse; vuoto se non ce
 * ne sono (opzione spenta, git mancante o foto non riuscita).
 */
restorePoints: Array<RestorePoint>, 
/**
 * La sessione che ha affidato questo lavoro, se l'ha avviata un altro agente.
 */
parentSessionId: number | null, };

export type RestorePoint = { folder: string, 
/**
 * Il commit dell'istantanea nel repository separato.
 */
commit: string, 
/**
 * File fotografati.
 */
files: number, };

export type SessionStatus = "running" | "paused" | "waiting" | "done" | "stopped" | "failed" | "interrupted";

export type TaskBody = { task: string, };

export type ReplyBody = { text: string, };

export type LiveState = { status: SessionStatus | null, streaming: string, reasoning: string, request: PendingRequest | null, 
/**
 * Cresce a ogni nuovo messaggio salvato: se cambia, l'interfaccia rilegge.
 */
revision: number, };

export type PendingRequest = { id: string, name: string, summary: string, arguments: JsonValue, };
