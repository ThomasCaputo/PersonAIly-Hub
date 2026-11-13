// Scelta del modello tra quelli dell'app: locale (anche da caricare) o cloud.

import * as vscode from "vscode";
import { CoreClient, CoreError } from "./core";
import type { ModelRef } from "./edits";

export interface ModelChoice {
  ref: ModelRef;
  label: string;
}

interface ApiModels {
  local: {
    loaded: { modelPath: string; modelName: string } | null;
    loading: boolean;
    library: { path: string; name: string; sizeBytes: number; quantization: string | null }[];
  };
  cloud: {
    id: string;
    name: string;
    hasKey: boolean;
    models: { id: string; name: string }[];
  }[];
}

type Item = vscode.QuickPickItem & { choice?: ModelChoice; loadPath?: string };

const gb = (bytes: number) => (bytes / 1024 ** 3).toLocaleString("it-IT", { maximumFractionDigits: 1 });

/** Mostra l'elenco dei modelli; un modello locale non caricato viene caricato. */
export async function pickModel(client: CoreClient): Promise<ModelChoice | undefined> {
  const models = await client.request<ApiModels>("GET", "/api/models");
  const items: Item[] = [];
  const loaded = models.local.loaded;

  items.push({ label: "Locale", kind: vscode.QuickPickItemKind.Separator });
  if (loaded) {
    items.push({
      label: `$(chip) ${loaded.modelName}`,
      description: "caricato",
      choice: { ref: { kind: "local" }, label: loaded.modelName },
    });
  }
  for (const model of models.local.library) {
    if (model.path === loaded?.modelPath) continue;
    items.push({
      label: `$(cloud-download) ${model.name}`,
      description: `da caricare · ${gb(model.sizeBytes)} GB${model.quantization ? ` · ${model.quantization}` : ""}`,
      detail: loaded ? "Sostituisce il modello caricato ora." : undefined,
      loadPath: model.path,
      choice: { ref: { kind: "local" }, label: model.name },
    });
  }
  for (const provider of models.cloud.filter((p) => p.hasKey && p.models.length > 0)) {
    items.push({ label: provider.name, kind: vscode.QuickPickItemKind.Separator });
    for (const model of provider.models) {
      items.push({
        label: `$(cloud) ${model.name || model.id}`,
        description: model.name ? model.id : undefined,
        choice: {
          ref: { kind: "cloud", providerId: provider.id, model: model.id },
          label: `${model.name || model.id} (${provider.name})`,
        },
      });
    }
  }
  if (!items.some((item) => item.choice)) {
    vscode.window.showWarningMessage(
      "Nessun modello disponibile: aggiungi una cartella di modelli o un provider cloud nella pagina Modelli dell'app.",
    );
    return undefined;
  }

  const picked = await vscode.window.showQuickPick(items, {
    title: "Progetto AI: scegli il modello",
    matchOnDescription: true,
  });
  if (!picked?.choice) return undefined;
  if (picked.loadPath && !(await load(client, picked.loadPath, picked.choice.label))) return undefined;
  return picked.choice;
}

async function load(client: CoreClient, path: string, name: string): Promise<boolean> {
  const attempt = (force: boolean) =>
    vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `Caricamento di ${name}…` },
      () => client.request("POST", "/api/local/load", { path, force }),
    );
  try {
    await attempt(false);
    return true;
  } catch (e) {
    if (!(e instanceof CoreError) || !e.message.includes("non entra")) throw e;
    const answer = await vscode.window.showWarningMessage(
      `${e.message} Caricarlo comunque? Potrebbe rallentare molto il computer.`,
      { modal: true },
      "Carica comunque",
    );
    if (answer !== "Carica comunque") return false;
    await attempt(true);
    return true;
  }
}
