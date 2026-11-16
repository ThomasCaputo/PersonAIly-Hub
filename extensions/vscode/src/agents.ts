// Agenti dell'app avviati e seguiti da VS Code: avanzamento nella barra di
// stato e richieste di conferma come notifiche, senza dover aprire l'app.

import * as vscode from "vscode";
import type {
  Agent,
  AgentSession,
  AnswerBody,
  LiveState,
  Message,
  SessionStatus,
  TaskBody,
} from "../../../packages/api-client";
import { CoreClient, CoreError } from "./core";
import { t, tc } from "./i18n";

const POLL_MS = 1500;
const ACTIVE: SessionStatus[] = ["running", "paused", "waiting"];

/** Segue le sessioni degli agenti: una voce nella barra di stato per quelle al lavoro. */
export class AgentSessions implements vscode.Disposable {
  private readonly followed = new Map<number, { name: string; shownRequest: string | null }>();
  private readonly item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 99);
  private timer: NodeJS.Timeout | undefined;

  constructor(private readonly core: CoreClient) {
    this.item.command = "personailyHub.sessions";
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
    this.item.dispose();
  }

  /** Sceglie un agente, gli affida un compito e lo segue. */
  async start(): Promise<void> {
    const agents = await this.core.request<Agent[]>("GET", "/api/agents");
    if (agents.length === 0) {
      vscode.window.showInformationMessage(t("Nessun agente: crealo nella pagina Agenti dell'app."));
      return;
    }
    const agent = await vscode.window.showQuickPick(
      agents.map((a) => ({ label: a.name, description: a.description, id: a.id })),
      { title: t("PersonAIly Hub: scegli l'agente") },
    );
    if (!agent) return;
    const task = await vscode.window.showInputBox({
      title: `PersonAIly Hub · ${agent.label}`,
      prompt: t("Che cosa deve fare?"),
      ignoreFocusOut: true,
    });
    if (!task?.trim()) return;
    const session = await this.core.request<AgentSession>("POST", `/api/agents/${agent.id}/sessions`, {
      task: task.trim(),
    } satisfies TaskBody);
    this.follow(session.id, agent.label);
    vscode.window.showInformationMessage(
      t("{agent} è al lavoro. Le richieste di conferma arrivano qui come notifiche.", { agent: agent.label }),
    );
  }

  /** Elenca le sessioni al lavoro per seguirne una (anche avviata dall'app). */
  async pick(): Promise<void> {
    const sessions = await this.core.request<AgentSession[]>("GET", "/api/sessions");
    const active = sessions.filter((s) => ACTIVE.includes(s.status));
    if (active.length === 0) {
      vscode.window.showInformationMessage(t("Nessun agente al lavoro."));
      return;
    }
    const picked = await vscode.window.showQuickPick(
      active.map((s) => ({
        label: s.agentName,
        description: statusLabel(s.status),
        detail: s.task,
        session: s,
      })),
      { title: t("PersonAIly Hub: agenti al lavoro") },
    );
    if (!picked) return;
    this.follow(picked.session.id, picked.session.agentName);
    const action = await vscode.window.showInformationMessage(
      `${picked.session.agentName}: ${statusLabel(picked.session.status)}`,
      t("Ferma"),
    );
    if (action === t("Ferma")) await this.core.request("POST", `/api/sessions/${picked.session.id}/stop`);
  }

  follow(id: number, name: string): void {
    if (!this.followed.has(id)) this.followed.set(id, { name, shownRequest: null });
    this.timer ??= setInterval(() => void this.poll(), POLL_MS);
    void this.poll();
  }

  private async poll(): Promise<void> {
    for (const [id, followed] of [...this.followed]) {
      let live: LiveState | null;
      try {
        live = await this.core.request<LiveState | null>("GET", `/api/sessions/${id}/live`);
      } catch (e) {
        // Nucleo chiuso o sostituito: la sessione è stata interrotta.
        if (e instanceof CoreError && e.status !== 0) this.followed.delete(id);
        continue;
      }
      if (!live) {
        this.followed.delete(id);
        void this.finished(id, followed.name);
        continue;
      }
      const request = live.request;
      if (request && followed.shownRequest !== request.id) {
        followed.shownRequest = request.id;
        void this.ask(followed.name, request);
      }
      if (!request) followed.shownRequest = null;
    }
    this.updateItem();
    if (this.followed.size === 0 && this.timer) {
      clearInterval(this.timer);
      this.timer = undefined;
    }
  }

  private updateItem(): void {
    if (this.followed.size === 0) {
      this.item.hide();
      return;
    }
    const names = [...this.followed.values()].map((f) => f.name);
    this.item.text = `$(sync~spin) ${names.length === 1 ? names[0] : t("{n} agenti", { n: names.length })}`;
    this.item.tooltip = t("Agenti al lavoro: {names}", { names: names.join(", ") });
    this.item.show();
  }

  /** Una richiesta di conferma dell'agente, come notifica con i pulsanti. */
  private async ask(agent: string, request: { id: string; summary: string }): Promise<void> {
    const once = t("Consenti");
    const always = t("Consenti sempre in questa sessione");
    const deny = t("Nega");
    const answer = await vscode.window.showWarningMessage(
      t("{agent} chiede il permesso di: {summary}", { agent, summary: tc(request.summary) }),
      { modal: false },
      once,
      always,
      deny,
    );
    // Chiudere la notifica senza scegliere lascia la decisione all'app.
    if (!answer) return;
    const approval = answer === once ? "once" : answer === always ? "always" : "deny";
    try {
      await this.core.request("POST", "/api/tool-requests", { id: request.id, approval } satisfies AnswerBody);
    } catch (e) {
      vscode.window.showErrorMessage(`PersonAIly Hub: ${(e as Error).message}`);
    }
  }

  /** La sessione ha smesso di lavorare: esito e, a richiesta, il resoconto. */
  private async finished(id: number, name: string): Promise<void> {
    let session: AgentSession;
    try {
      session = await this.core.request<AgentSession>("GET", `/api/sessions/${id}`);
    } catch {
      return;
    }
    const show = t("Mostra il resoconto");
    const text = `${name}: ${statusLabel(session.status)}${session.note ? ` — ${tc(session.note)}` : ""}`;
    const answer =
      session.status === "failed"
        ? await vscode.window.showErrorMessage(text, show)
        : await vscode.window.showInformationMessage(text, show);
    if (answer !== show) return;
    const messages = await this.core.request<Message[]>(
      "GET",
      `/api/conversations/${session.conversationId}/messages`,
    );
    const last = [...messages].reverse().find((m) => m.role === "assistant" && m.content.trim());
    const document = await vscode.workspace.openTextDocument({
      language: "markdown",
      content: `# ${name}\n\n**${t("Compito")}:** ${session.task}\n\n${last?.content ?? t("(nessuna risposta)")}\n`,
    });
    await vscode.window.showTextDocument(document, { preview: true });
  }
}

function statusLabel(status: SessionStatus): string {
  return {
    running: t("al lavoro"),
    paused: t("in pausa"),
    waiting: t("aspetta una conferma"),
    done: t("ha finito"),
    stopped: t("fermato"),
    failed: t("non riuscito"),
    interrupted: t("interrotto"),
  }[status];
}
