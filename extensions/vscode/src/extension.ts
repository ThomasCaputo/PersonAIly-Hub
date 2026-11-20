// Estensione VS Code di PersonAIly Hub (Tappa 5): pannello chat, contesto
// dell'editor e modifiche proposte come diff. Tutta la logica sui modelli sta
// nel nucleo dell'app, raggiunto tramite l'API locale (ADR 0004 e 0010).

import * as vscode from "vscode";
import type { Conversation } from "../../../packages/api-client";
import { AgentSessions } from "./agents";
import { ChatView } from "./chatView";
import { CoreClient, defaultDataDir } from "./core";
import { editPrompt, editSystemPrompt, extractCode, openAiModelName } from "./edits";
import { currentEditor, editorContext } from "./editor";
import { languageFor, setLanguage, t } from "./i18n";
import { InlineCompletion } from "./inline";
import { type ModelChoice, pickModel } from "./models";
import { PROPOSAL_SCHEME, Proposals } from "./proposals";

const MODEL_KEY = "personailyHub.model";

let client: CoreClient | undefined;

export function activate(context: vscode.ExtensionContext): void {
  // L'estensione parla la lingua di VS Code.
  setLanguage(languageFor(vscode.env.language));
  const output = vscode.window.createOutputChannel("PersonAIly Hub");
  const config = () => vscode.workspace.getConfiguration("personailyHub");
  const core = new CoreClient({
    dataDir: () => config().get<string>("dataDir")?.trim() || defaultDataDir(),
    serverPath: () => config().get<string>("serverPath") ?? "",
    log: (line) => output.appendLine(line),
  });
  client = core;

  const proposals = new Proposals();
  const sessions = new AgentSessions(core);
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  status.command = "personailyHub.chooseModel";
  status.tooltip = t("PersonAIly Hub: scegli il modello");

  const model = () => context.globalState.get<ModelChoice>(MODEL_KEY);
  const showModel = () => {
    status.text = `$(sparkle) ${model()?.label ?? t("Scegli il modello")}`;
    status.show();
  };
  showModel();

  const chooseModel = async (): Promise<ModelChoice | undefined> => {
    const choice = await pickModel(core);
    if (choice) {
      await context.globalState.update(MODEL_KEY, choice);
      showModel();
      await chat.refresh().catch(() => undefined);
    }
    return choice;
  };

  const chat = new ChatView(
    context.extensionUri,
    core,
    context.workspaceState,
    proposals,
    model,
    chooseModel,
    () => config().get<number>("maxContextChars") ?? 60000,
  );

  /** Esegue un comando mostrando gli errori invece di lasciarli cadere. */
  const guarded = (f: () => Promise<unknown>) => async () => {
      try {
        await f();
      } catch (e) {
        vscode.window.showErrorMessage(`PersonAIly Hub: ${(e as Error).message}`);
      }
    };

  context.subscriptions.push(
    output,
    status,
    proposals,
    sessions,
    vscode.workspace.registerTextDocumentContentProvider(PROPOSAL_SCHEME, proposals),
    // Completamento in linea (VS-6): il provider tace se l'impostazione è spenta.
    vscode.languages.registerInlineCompletionItemProvider(
      { pattern: "**" },
      new InlineCompletion(core, config, (line) => output.appendLine(line)),
    ),
    vscode.window.registerWebviewViewProvider("personailyHub.chat", chat, {
      webviewOptions: { retainContextWhenHidden: true },
    }),
    vscode.commands.registerCommand("personailyHub.chooseModel", guarded(chooseModel)),
    vscode.commands.registerCommand(
      "personailyHub.newChat",
      guarded(() => chat.newConversation()),
    ),
    vscode.commands.registerCommand(
      "personailyHub.history",
      guarded(async () => {
        const list = await core.request<Conversation[]>(
          "GET",
          "/api/conversations",
        );
        const picked = await vscode.window.showQuickPick(
          list.map((c) => ({ label: c.title, description: c.updatedAt.replace("T", " ").slice(0, 16), id: c.id })),
          { title: t("PersonAIly Hub: apri una conversazione"), matchOnDescription: true },
        );
        if (picked) await chat.openConversation(picked.id);
      }),
    ),
    vscode.commands.registerCommand(
      "personailyHub.edit",
      guarded(async () => requestEdit(core, proposals, model() ?? (await chooseModel()), config)),
    ),
    vscode.commands.registerCommand(
      "personailyHub.acceptProposal",
      guarded(() => proposals.accept()),
    ),
    vscode.commands.registerCommand(
      "personailyHub.rejectProposal",
      guarded(() => proposals.reject()),
    ),
    vscode.commands.registerCommand(
      "personailyHub.startAgent",
      guarded(() => sessions.start()),
    ),
    vscode.commands.registerCommand(
      "personailyHub.sessions",
      guarded(() => sessions.pick()),
    ),
  );
}

export async function deactivate(): Promise<void> {
  // Il nucleo avviato da questa finestra non deve restare acceso con il
  // modello in memoria. Se lo usano altre finestre, lo riavvieranno.
  await client?.shutdownIfOurs();
}

/** Chiede una modifica al file aperto (o alla selezione) e la propone come diff. */
async function requestEdit(
  core: CoreClient,
  proposals: Proposals,
  model: ModelChoice | undefined,
  config: () => vscode.WorkspaceConfiguration,
): Promise<void> {
  const editor = currentEditor();
  if (!editor) {
    vscode.window.showWarningMessage(t("Apri il file da modificare."));
    return;
  }
  if (!model) return;
  const context = editorContext(editor, config().get<number>("maxContextChars") ?? 60000);
  if (typeof context === "string") {
    vscode.window.showWarningMessage(context);
    return;
  }
  const where =
    context.target.startLine !== undefined
      ? t("righe {start}-{end}", { start: String(context.target.startLine), end: String(context.target.endLine) })
      : t("tutto il file");
  const instruction = await vscode.window.showInputBox({
    title: `PersonAIly Hub · ${model.label}`,
    prompt: t("Che modifica vuoi su {where}?", { where }),
    placeHolder: t("Per esempio: aggiungi la gestione degli errori"),
    ignoreFocusOut: true,
  });
  if (!instruction?.trim()) return;

  let reply = "";
  const finished = await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: t("{model} sta scrivendo la modifica", { model: model.label }),
      cancellable: true,
    },
    async (progress, token) => {
      const abort = new AbortController();
      token.onCancellationRequested(() => abort.abort());
      try {
        await core.stream(
          "/v1/chat/completions",
          {
            model: openAiModelName(model.ref),
            stream: true,
            messages: [
              { role: "system", content: editSystemPrompt() },
              { role: "user", content: editPrompt(context.target, instruction.trim()) },
            ],
          },
          (_event, data) => {
            if (data === "[DONE]") return;
            const chunk = JSON.parse(data);
            if (chunk.error) throw new Error(chunk.error.message);
            const text = chunk.choices?.[0]?.delta?.content;
            if (text) {
              reply += text;
              progress.report({ message: t("{n} caratteri", { n: reply.length }) });
            }
          },
          abort.signal,
        );
        return true;
      } catch (e) {
        if ((e as Error).name === "AbortError") return false;
        throw e;
      }
    },
  );
  if (!finished) return;
  const code = extractCode(reply);
  if (!code) {
    vscode.window.showWarningMessage(t("Il modello non ha proposto codice."));
    return;
  }
  const document = await vscode.workspace.openTextDocument(context.uri);
  await proposals.show(document, context.range, code);
}
