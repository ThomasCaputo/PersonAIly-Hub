// Estensione VS Code di PersonAIly Hub (Tappa 5): pannello chat, contesto
// dell'editor e modifiche proposte come diff. Tutta la logica sui modelli sta
// nel nucleo dell'app, raggiunto tramite l'API locale (ADR 0004 e 0010).

import * as vscode from "vscode";
import { ChatView } from "./chatView";
import { CoreClient, defaultDataDir } from "./core";
import { EDIT_SYSTEM_PROMPT, editPrompt, extractCode, openAiModelName } from "./edits";
import { currentEditor, editorContext } from "./editor";
import { type ModelChoice, pickModel } from "./models";
import { PROPOSAL_SCHEME, Proposals } from "./proposals";

const MODEL_KEY = "personailyHub.model";

let client: CoreClient | undefined;

export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel("PersonAIly Hub");
  const config = () => vscode.workspace.getConfiguration("personailyHub");
  const core = new CoreClient({
    dataDir: () => config().get<string>("dataDir")?.trim() || defaultDataDir(),
    serverPath: () => config().get<string>("serverPath") ?? "",
    log: (line) => output.appendLine(line),
  });
  client = core;

  const proposals = new Proposals();
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  status.command = "personailyHub.chooseModel";
  status.tooltip = "PersonAIly Hub: scegli il modello";

  const model = () => context.globalState.get<ModelChoice>(MODEL_KEY);
  const showModel = () => {
    status.text = `$(sparkle) ${model()?.label ?? "Scegli il modello"}`;
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
    vscode.workspace.registerTextDocumentContentProvider(PROPOSAL_SCHEME, proposals),
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
        const list = await core.request<{ id: number; title: string; updatedAt: string }[]>(
          "GET",
          "/api/conversations",
        );
        const picked = await vscode.window.showQuickPick(
          list.map((c) => ({ label: c.title, description: c.updatedAt.replace("T", " ").slice(0, 16), id: c.id })),
          { title: "PersonAIly Hub: apri una conversazione", matchOnDescription: true },
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
    vscode.commands.registerCommand("personailyHub.startAgent", guarded(() => startAgent(core))),
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
    vscode.window.showWarningMessage("Apri il file da modificare.");
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
      ? `righe ${context.target.startLine}-${context.target.endLine}`
      : "tutto il file";
  const instruction = await vscode.window.showInputBox({
    title: `PersonAIly Hub · ${model.label}`,
    prompt: `Che modifica vuoi su ${where}?`,
    placeHolder: "Per esempio: aggiungi la gestione degli errori",
    ignoreFocusOut: true,
  });
  if (!instruction?.trim()) return;

  let reply = "";
  const finished = await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: `${model.label} sta scrivendo la modifica`,
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
              { role: "system", content: EDIT_SYSTEM_PROMPT },
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
              progress.report({ message: `${reply.length} caratteri` });
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
    vscode.window.showWarningMessage("Il modello non ha proposto codice.");
    return;
  }
  const document = await vscode.workspace.openTextDocument(context.uri);
  await proposals.show(document, context.range, code);
}

/** Avvia un agente dell'app su un compito; lo si segue nella pagina Agenti dell'app. */
async function startAgent(core: CoreClient): Promise<void> {
  const agents = await core.request<{ id: number; name: string; description: string }[]>("GET", "/api/agents");
  if (agents.length === 0) {
    vscode.window.showInformationMessage("Nessun agente: crealo nella pagina Agenti dell'app.");
    return;
  }
  const agent = await vscode.window.showQuickPick(
    agents.map((a) => ({ label: a.name, description: a.description, id: a.id })),
    { title: "PersonAIly Hub: scegli l'agente" },
  );
  if (!agent) return;
  const task = await vscode.window.showInputBox({
    title: `PersonAIly Hub · ${agent.label}`,
    prompt: "Che cosa deve fare?",
    ignoreFocusOut: true,
  });
  if (!task?.trim()) return;
  await core.request("POST", `/api/agents/${agent.id}/sessions`, { task: task.trim() });
  vscode.window.showInformationMessage(
    `${agent.label} è al lavoro. Le richieste di conferma e l'avanzamento sono nella pagina Agenti dell'app.`,
  );
}
