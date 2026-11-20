// Completamento del codice in linea (VS-6): mentre si scrive, il modello
// locale propone il seguito in grigio; Tab lo accetta. Il nucleo usa
// l'endpoint "fill-in-the-middle" di llama-server: serve un modello per il
// codice (per esempio Qwen2.5-Coder). Spento di base: si accende con
// l'impostazione "personailyHub.inlineCompletion".

import * as vscode from "vscode";
import type { InfillBody, InfillReply } from "../../../packages/api-client";
import { CoreError, type CoreClient } from "./core";
import { t, tc } from "./i18n";
import { cleanCompletion, contextAround } from "./infill";

/** Attesa prima di chiedere: si risparmia il modello mentre si scrive veloce. */
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export class InlineCompletion implements vscode.InlineCompletionItemProvider {
  /** L'ultimo errore mostrato: lo stesso non si ripete a ogni tasto. */
  private shownError: string | null = null;

  constructor(
    private readonly core: CoreClient,
    private readonly config: () => vscode.WorkspaceConfiguration,
    private readonly log: (line: string) => void,
  ) {}

  async provideInlineCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
    _context: vscode.InlineCompletionContext,
    token: vscode.CancellationToken,
  ): Promise<vscode.InlineCompletionItem[]> {
    if (!this.config().get<boolean>("inlineCompletion")) return [];
    await sleep(this.config().get<number>("inlineCompletionDelay") ?? 300);
    if (token.isCancellationRequested) return [];

    const { prefix, suffix } = contextAround(document.getText(), document.offsetAt(position));
    if (!prefix.trim()) return [];
    // Scrivendo ancora, VS Code annulla la richiesta: la si chiude, e il
    // nucleo smette di generare.
    const abort = new AbortController();
    const subscription = token.onCancellationRequested(() => abort.abort());
    try {
      const body: InfillBody = { prefix, suffix, maxTokens: 128 };
      const reply = await this.core.request<InfillReply>("POST", "/api/infill", body, abort.signal);
      this.shownError = null;
      const text = cleanCompletion(reply.text, suffix);
      if (!text || token.isCancellationRequested) return [];
      return [new vscode.InlineCompletionItem(text, new vscode.Range(position, position))];
    } catch (e) {
      if ((e as Error).name === "AbortError") return [];
      const message = (e as Error).message;
      this.log(t("Completamento in linea: {error}", { error: message }));
      // Senza modello caricato si tace; un modello inadatto lo si dice una volta.
      if (e instanceof CoreError && e.status === 400 && message !== this.shownError) {
        this.shownError = message;
        const quiet = [tc("Nessun modello locale caricato."), tc("Il modello locale si sta caricando: aspetta.")];
        if (!quiet.includes(message)) {
          vscode.window.showWarningMessage(`PersonAIly Hub: ${message}`);
        }
      }
      return [];
    } finally {
      subscription.dispose();
    }
  }
}
