// Modifiche proposte dal modello, mostrate come diff da accettare o rifiutare (VS-4).

import * as vscode from "vscode";
import { fitReplacement, replaceRange } from "./edits";

export const PROPOSAL_SCHEME = "personaily-proposta";

interface Proposal {
  id: string;
  /** Il documento da modificare. */
  target: vscode.Uri;
  /** Intervallo sostituito e testo che c'era al momento della proposta. */
  range: vscode.Range;
  originalText: string;
  newText: string;
  /** Il documento intero con la modifica applicata: il lato destro del diff. */
  preview: vscode.Uri;
}

export class Proposals implements vscode.TextDocumentContentProvider, vscode.Disposable {
  private readonly proposals = new Map<string, Proposal>();
  private readonly previews = new Map<string, string>();
  private readonly changed = new vscode.EventEmitter<vscode.Uri>();
  readonly onDidChange = this.changed.event;
  private counter = 0;
  /** Pulsanti nella barra di stato, visibili mentre si guarda una proposta. */
  private readonly acceptItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 1000);
  private readonly rejectItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 999);
  private readonly listeners: vscode.Disposable[];

  constructor() {
    this.acceptItem.text = "$(check) Accetta la modifica";
    this.acceptItem.command = "personailyHub.acceptProposal";
    this.acceptItem.backgroundColor = new vscode.ThemeColor("statusBarItem.warningBackground");
    this.rejectItem.text = "$(close) Rifiuta";
    this.rejectItem.command = "personailyHub.rejectProposal";
    this.listeners = [
      vscode.window.onDidChangeActiveTextEditor(() => this.updateButtons()),
      vscode.window.tabGroups.onDidChangeTabs(() => this.updateButtons()),
    ];
  }

  dispose(): void {
    this.acceptItem.dispose();
    this.rejectItem.dispose();
    this.listeners.forEach((l) => l.dispose());
  }

  private updateButtons(): void {
    const id = this.activeId();
    const visible = id !== undefined && this.proposals.has(id);
    for (const item of [this.acceptItem, this.rejectItem]) {
      if (visible) item.show();
      else item.hide();
    }
  }

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.previews.get(uri.toString()) ?? "";
  }

  /** Mostra il diff tra il documento e la proposta, con i comandi per accettarla. */
  async show(document: vscode.TextDocument, range: vscode.Range, proposedCode: string): Promise<void> {
    const originalText = document.getText(range);
    const eol = document.eol === vscode.EndOfLine.CRLF ? "\r\n" : "\n";
    const newText = fitReplacement(proposedCode, originalText, eol);
    if (newText === originalText) {
      vscode.window.showInformationMessage("Il modello non ha proposto cambiamenti.");
      return;
    }
    const id = String(++this.counter);
    const name = document.uri.path.split("/").pop() || "documento";
    // Il nome del file resta in fondo al percorso: VS Code ne ricava il linguaggio.
    const preview = vscode.Uri.from({ scheme: PROPOSAL_SCHEME, path: `/${id}/${name}` });
    const full = document.getText();
    const start = document.offsetAt(range.start);
    const end = document.offsetAt(range.end);
    this.previews.set(preview.toString(), replaceRange(full, start, end, newText));
    this.proposals.set(id, { id, target: document.uri, range, originalText, newText, preview });

    await vscode.commands.executeCommand("vscode.diff", document.uri, preview, `${name} ↔ proposta di PersonAIly Hub`, {
      preview: false,
    });
    this.updateButtons();
  }

  /** La proposta mostrata nell'editor attivo. */
  activeId(): string | undefined {
    const uri = vscode.window.activeTextEditor?.document.uri;
    if (uri?.scheme === PROPOSAL_SCHEME) return uri.path.split("/")[1];
    const tab = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
    if (tab instanceof vscode.TabInputTextDiff && tab.modified.scheme === PROPOSAL_SCHEME) {
      return tab.modified.path.split("/")[1];
    }
    return undefined;
  }

  async accept(id = this.activeId()): Promise<void> {
    const proposal = id ? this.proposals.get(id) : undefined;
    if (!proposal) return;
    const document = await vscode.workspace.openTextDocument(proposal.target);
    // Se nel frattempo il testo è cambiato proprio lì, non si sovrascrive nulla.
    if (document.getText(proposal.range) !== proposal.originalText) {
      vscode.window.showWarningMessage(
        "Il file è cambiato nel punto da modificare dopo la proposta: chiedi di nuovo la modifica.",
      );
      return;
    }
    const edit = new vscode.WorkspaceEdit();
    edit.replace(proposal.target, proposal.range, proposal.newText);
    if (!(await vscode.workspace.applyEdit(edit))) {
      vscode.window.showErrorMessage("Non è stato possibile applicare la modifica.");
      return;
    }
    await this.close(proposal);
    await vscode.window.showTextDocument(document, { preview: false });
  }

  async reject(id = this.activeId()): Promise<void> {
    const proposal = id ? this.proposals.get(id) : undefined;
    if (proposal) await this.close(proposal);
  }

  private async close(proposal: Proposal): Promise<void> {
    this.proposals.delete(proposal.id);
    const tabs = vscode.window.tabGroups.all
      .flatMap((group) => group.tabs)
      .filter((tab) => {
        const input = tab.input;
        return (
          (input instanceof vscode.TabInputTextDiff && input.modified.toString() === proposal.preview.toString()) ||
          (input instanceof vscode.TabInputText && input.uri.toString() === proposal.preview.toString())
        );
      });
    await vscode.window.tabGroups.close(tabs);
    this.previews.delete(proposal.preview.toString());
    this.updateButtons();
  }
}
