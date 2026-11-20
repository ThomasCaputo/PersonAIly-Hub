import type { LoadParams } from "./api";
import { t } from "./i18n";

/** "Solo VRAM", "Solo RAM" o "Ibrida: 20 layer sulla GPU". */
export function placementLabel(params: LoadParams): string {
  if (params.placement === "vram") return t("Solo VRAM");
  if (params.placement === "ram") return t("Solo RAM");
  return t("Ibrida: {n} layer sulla GPU", { n: params.gpuLayers ?? 0 });
}
