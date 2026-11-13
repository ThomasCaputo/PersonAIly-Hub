// Pannello chat nella barra laterale (VS-2): usa le conversazioni dell'app,
// che restano visibili anche nella sua pagina Chat.

import * as vscode from "vscode";
import { CoreClient, CoreError } from "./core";
import { contextBlock, extractCode } from "./edits";
import { currentEditor, editorContext, type EditorContext } from "./editor";
import type { ModelChoice } from "./models";
import type { Proposals } from "./proposals";

const CONVERSATION_KEY = "progettoAi.conversationId";

/** Un messaggio dell'archivio, come lo restituisce l'API. */
interface ApiMessage {
  id: number;
  role: string;
  content: string;
  model: string | null;
  toolCalls: { id: string; name: string }[];
  toolName: string | null;
  isError: boolean;
}

/** Messaggi dal pannello all'estensione. */
type FromView =
  | { type: "ready" }
  | { type: "send"; text: string; includeContext: boolean }
  | { type: "stop" }
  | { type: "approve"; id: string; approval: "once" | "always" | "deny" }
  | { type: "apply"; code: string }
  | { type: "chooseModel" };

/** Titolo dalla prima riga del testo, come fa il nucleo. */
function titleFrom(text: string): string {
  const line = text.split(/\r?\n/).find((l) => l.trim())?.trim() ?? "Nuova conversazione";
  return line.length <= 48 ? line : `${line.slice(0, 48).trimEnd()}…`;
}

export class ChatView implements vscode.WebviewViewProvider {
  private view?: vscode.WebviewView;
  private abort?: AbortController;
  /** Il contesto inviato con l'ultimo messaggio: lì vanno le proposte della chat. */
  private lastContext?: EditorContext;

  constructor(
    private readonly extensionUri: vscode.Uri,
    private readonly client: CoreClient,
    private readonly state: vscode.Memento,
    private readonly proposals: Proposals,
    private readonly model: () => ModelChoice | undefined,
    private readonly chooseModel: () => Promise<ModelChoice | undefined>,
    private readonly maxContextChars: () => number,
  ) {}

  resolveWebviewView(view: vscode.WebviewView): void {
    this.view = view;
    const media = vscode.Uri.joinPath(this.extensionUri, "media");
    view.webview.options = { enableScripts: true, localResourceRoots: [media] };
    const nonce = Array.from({ length: 32 }, () => Math.floor(Math.random() * 16).toString(16)).join("");
    const script = view.webview.asWebviewUri(vscode.Uri.joinPath(media, "chat.js"));
    const style = view.webview.asWebviewUri(vscode.Uri.joinPath(media, "chat.css"));
    view.webview.html = `<!DOCTYPE html>
<html lang="it">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${view.webview.cspSource}; script-src 'nonce-${nonce}';">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="stylesheet" href="${style}">
</head>
<body>
<div id="top"><button id="model" class="link" title="Scegli il modello"></button><span id="title"></span></div>
<div id="messages"></div>
<div id="composer">
  <label class="context"><input type="checkbox" id="includeContext" checked> Includi il file aperto o la selezione</label>
  <textarea id="input" rows="3" placeholder="Scrivi un messaggio… (Invio per inviare, Maiusc+Invio per andare a capo)"></textarea>
  <div class="row"><span id="status"></span><button id="send">Invia</button></div>
</div>
<script nonce="${nonce}" src="${script}"></script>
</body>
</html>`;
    view.webview.onDidReceiveMessage((message: FromView) => this.handle(message));
  }

  private post(message: unknown): void {
    void this.view?.webview.postMessage(message);
  }

  private get conversationId(): number | undefined {
    return this.state.get<number>(CONVERSATION_KEY);
  }

  private async setConversation(id: number | undefined): Promise<void> {
    await this.state.update(CONVERSATION_KEY, id);
  }

  private async handle(message: FromView): Promise<void> {
    try {
      switch (message.type) {
        case "ready":
          await this.refresh();
          break;
        case "send":
          await this.send(message.text, message.includeContext);
          break;
        case "stop":
          this.abort?.abort();
          if (this.conversationId !== undefined) {
            await this.client.request("POST", `/api/conversations/${this.conversationId}/stop`);
          }
          break;
        case "approve":
          await this.client.request("POST", "/api/tool-requests", { id: message.id, approval: message.approval });
          break;
        case "apply":
          await this.apply(message.code);
          break;
        case "chooseModel":
          await this.chooseModel();
          break;
      }
    } catch (e) {
      this.post({ type: "error", message: (e as Error).message });
    }
  }

  /** Ridisegna il pannello: modello, titolo e messaggi della conversazione. */
  async refresh(): Promise<void> {
    const model = this.model();
    let title = "Nuova conversazione";
    let messages: ApiMessage[] = [];
    const id = this.conversationId;
    if (id !== undefined) {
      try {
        messages = await this.client.request<ApiMessage[]>("GET", `/api/conversations/${id}/messages`);
        const all = await this.client.request<{ id: number; title: string }[]>("GET", "/api/conversations");
        const found = all.find((c) => c.id === id);
        if (found) title = found.title;
        else await this.setConversation(undefined); // eliminata nell'app
      } catch (e) {
        if (!(e instanceof CoreError) || e.status === 0) throw e;
        await this.setConversation(undefined);
      }
    }
    this.post({
      type: "state",
      model: model?.label ?? null,
      title,
      messages: messages.map((m) => ({
        role: m.role,
        content: m.content,
        tools: m.toolCalls.map((c) => c.name),
        toolName: m.toolName,
        isError: m.isError,
      })),
    });
  }

  async newConversation(): Promise<void> {
    this.abort?.abort();
    await this.setConversation(undefined);
    this.lastContext = undefined;
    await this.refresh();
  }

  async openConversation(id: number): Promise<void> {
    this.abort?.abort();
    await this.setConversation(id);
    await this.refresh();
  }

  /** Invia un messaggio, con il contesto dell'editor se richiesto, e segue la risposta. */
  private async send(text: string, includeContext: boolean): Promise<void> {
    const model = this.model() ?? (await this.chooseModel());
    if (!model) {
      this.post({ type: "done" });
      return;
    }
    let full = text;
    if (includeContext) {
      const editor = currentEditor();
      if (editor) {
        const context = editorContext(editor, this.maxContextChars());
        if (typeof context === "string") {
          this.post({ type: "error", message: context });
          this.post({ type: "done" });
          return;
        }
        this.lastContext = context;
        full = `${contextBlock(context.target)}\n\n${text}`;
      }
    }

    let id = this.conversationId;
    const isNew = id === undefined;
    if (id === undefined) {
      id = (await this.client.request<{ id: number }>("POST", "/api/conversations")).id;
      await this.setConversation(id);
    }
    this.post({ type: "pending", text: full });

    this.abort = new AbortController();
    try {
      await this.client.stream(
        `/api/conversations/${id}/messages`,
        { text: full, model: model.ref },
        (event, data) => {
          const payload = JSON.parse(data);
          switch (event) {
            case "text":
            case "reasoning":
              this.post({ type: event, text: payload.text });
              break;
            case "saved":
              void this.refresh();
              break;
            case "toolRequest":
              this.post({ type: "toolRequest", request: payload });
              break;
            case "done":
              this.post({ type: "notice", text: payload.notice ?? null });
              break;
            case "error":
              this.post({ type: "error", message: payload.message });
              break;
          }
        },
        this.abort.signal,
      );
    } catch (e) {
      if ((e as Error).name !== "AbortError") throw e;
    } finally {
      this.abort = undefined;
      // Il nucleo intitola la conversazione con la prima riga del messaggio,
      // che qui sarebbe l'intestazione del contesto: si usa la domanda.
      if (isNew && full !== text) {
        await this.client
          .request("PATCH", `/api/conversations/${id}`, { title: titleFrom(text) })
          .catch(() => undefined);
      }
      await this.refresh();
      this.post({ type: "done" });
    }
  }

  /** Propone come diff il codice di una risposta, al posto del contesto inviato. */
  private async apply(code: string): Promise<void> {
    const extracted = extractCode(code) ?? code;
    const last = this.lastContext;
    if (last) {
      const document = await vscode.workspace.openTextDocument(last.uri);
      await this.proposals.show(document, last.range, extracted);
      return;
    }
    // Nessun contesto inviato: si sostituisce la selezione o il file aperto.
    const editor = currentEditor();
    if (!editor) {
      vscode.window.showWarningMessage("Apri il file a cui applicare il codice.");
      return;
    }
    const context = editorContext(editor, Number.MAX_SAFE_INTEGER);
    if (typeof context === "string") return;
    await this.proposals.show(editor.document, context.range, extracted);
  }
}
