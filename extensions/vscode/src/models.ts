// Scelta del modello tra quelli dell'app: locale (anche da caricare) o cloud.

import * as vscode from "vscode";
import { CoreClient, CoreError } from "./core";
import { locale, t } from "./i18n";
import type { LoadBody, Models } from "../../../packages/api-client";
import type { ModelRef } from "./edits";

export interface ModelChoice {
  ref: ModelRef;
  label: string;
}

type Item = vscode.QuickPickItem & { choice?: ModelChoice; loadPath?: string };

const gb = (bytes: number) => (bytes / 1024 ** 3).toLocaleString(locale(), { maximumFractionDigits: 1 });

/** Mostra l'elenco dei modelli; un modello locale non caricato viene caricato. */
export async function pickModel(client: CoreClient): Promise<ModelChoice | undefined> {
  const models = await client.request<Models>("GET", "/api/models");
  const items: Item[] = [];
  const loaded = models.local.loaded;

  items.push({ label: t("Locale"), kind: vscode.QuickPickItemKind.Separator });
  if (loaded) {
    items.push({
      label: `$(chip) ${loaded.modelName}`,
      description: t("caricato"),
      choice: { ref: { kind: "local" }, label: loaded.modelName },
    });
  }
  for (const model of models.local.library) {
    if (model.path === loaded?.modelPath) continue;
    items.push({
      label: `$(cloud-download) ${model.name}`,
      description: `${t("da caricare")} · ${gb(model.sizeBytes)} GB${model.quantization ? ` · ${model.quantization}` : ""}`,
      detail: loaded ? t("Sostituisce il modello caricato ora.") : undefined,
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
      t("Nessun modello disponibile: aggiungi una cartella di modelli o un provider cloud nella pagina Modelli dell'app."),
    );
    return undefined;
  }

  const picked = await vscode.window.showQuickPick(items, {
    title: t("PersonAIly Hub: scegli il modello"),
    matchOnDescription: true,
  });
  if (!picked?.choice) return undefined;
  if (picked.loadPath && !(await load(client, picked.loadPath, picked.choice.label))) return undefined;
  return picked.choice;
}

async function load(client: CoreClient, path: string, name: string): Promise<boolean> {
  const attempt = (force: boolean) =>
    vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: t("Caricamento di {name}…", { name }) },
      () => client.request("POST", "/api/local/load", { path, force } satisfies LoadBody),
    );
  try {
    await attempt(false);
    return true;
  } catch (e) {
    // Il nucleo rifiuta un modello che non entra in memoria con un 400 che lo dice.
    if (!(e instanceof CoreError) || !/non entra|does not fit/.test(e.message)) throw e;
    const answer = await vscode.window.showWarningMessage(
      t("{error} Caricarlo comunque? Potrebbe rallentare molto il computer.", { error: e.message }),
      { modal: true },
      t("Carica comunque"),
    );
    if (answer !== t("Carica comunque")) return false;
    await attempt(true);
    return true;
  }
}
