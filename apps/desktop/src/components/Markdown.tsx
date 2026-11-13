import { lazy, memo, Suspense } from "react";

const loadRenderer = () => import("./MarkdownRenderer");
const MarkdownRenderer = lazy(loadRenderer);

/** Carica il componente in anticipo, quando l'interfaccia è già comparsa. */
export function preloadMarkdown() {
  loadRenderer().catch(() => {});
}

/**
 * Testo del modello: Markdown con tabelle, codice evidenziato e formule.
 * Il componente vero si carica la prima volta che serve; nel frattempo il
 * testo si vede così com'è.
 */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <Suspense fallback={<div className="markdown pre">{text}</div>}>
      <MarkdownRenderer text={text} />
    </Suspense>
  );
});
