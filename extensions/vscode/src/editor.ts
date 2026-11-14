// Il contesto dell'editor da passare al modello: file aperto e selezione (VS-3).

import * as vscode from "vscode";
import type { EditTarget } from "./edits";

export interface EditorContext {
  uri: vscode.Uri;
  /** Ciò che una proposta sostituirà: le righe selezionate o il file intero. */
  range: vscode.Range;
  target: EditTarget;
}

/** L'editor di testo su cui lavorare, anche se il focus è nel pannello della chat. */
export function currentEditor(): vscode.TextEditor | undefined {
  const editor = vscode.window.activeTextEditor ?? vscode.window.visibleTextEditors[0];
  return editor?.document.uri.scheme === "personaily-proposta" ? undefined : editor;
}

/**
 * Il contesto dell'editor: la selezione estesa a righe intere, oppure il file
 * intero se non c'è selezione. Errore (stringa) se il file è troppo lungo.
 */
export function editorContext(editor: vscode.TextEditor, maxChars: number): EditorContext | string {
  const document = editor.document;
  const selection = editor.selection;
  const fileName = vscode.workspace.asRelativePath(document.uri, false);

  if (!selection.isEmpty) {
    // Una selezione che finisce all'inizio di una riga non la comprende.
    const lastLine =
      selection.end.character === 0 && selection.end.line > selection.start.line
        ? selection.end.line - 1
        : selection.end.line;
    const range = new vscode.Range(selection.start.line, 0, lastLine, document.lineAt(lastLine).text.length);
    const code = document.getText(range);
    if (code.length > maxChars) {
      return `La selezione è troppo lunga (${code.length} caratteri, massimo ${maxChars}).`;
    }
    return {
      uri: document.uri,
      range,
      target: {
        fileName,
        languageId: document.languageId,
        code,
        startLine: range.start.line + 1,
        endLine: range.end.line + 1,
      },
    };
  }

  const end = document.lineAt(document.lineCount - 1).range.end;
  const range = new vscode.Range(0, 0, end.line, end.character);
  const code = document.getText();
  if (code.length > maxChars) {
    return `Il file è troppo lungo (${code.length} caratteri, massimo ${maxChars}): seleziona la parte che ti interessa.`;
  }
  return { uri: document.uri, range, target: { fileName, languageId: document.languageId, code } };
}
