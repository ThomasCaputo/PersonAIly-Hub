import { memo, type MouseEvent, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import "highlight.js/styles/github-dark.css";
import "katex/dist/katex.min.css";
import { api } from "../api";
import { t } from "../i18n";

/**
 * Testo del modello: Markdown con tabelle, codice evidenziato e formule.
 * È la parte più pesante dell'interfaccia: si carica alla prima risposta da
 * mostrare (vedi `Markdown.tsx`).
 */
const MarkdownRenderer = memo(function MarkdownRenderer({ text }: { text: string }) {
  return (
    <div className="markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[rehypeKatex, rehypeHighlight]}
        components={{ a: ({ href, children }) => <ExternalLink href={href}>{children}</ExternalLink> }}
      >
        {normalizeMath(text)}
      </ReactMarkdown>
    </div>
  );
});

export default MarkdownRenderer;

/**
 * Un collegamento scritto dal modello. Non si apre nella finestra dell'app
 * (il guscio lo bloccherebbe comunque) ma nel browser di sistema, e prima si
 * mostra l'indirizzo vero: il testo del collegamento può dire tutt'altro.
 */
function ExternalLink({ href, children }: { href?: string; children: ReactNode }) {
  const web = href && /^(https?:|mailto:)/i.test(href);
  if (!web) return <span className="link-disabled">{children}</span>;

  const openIt = async (event: MouseEvent) => {
    event.preventDefault();
    const settings = await api.securitySettings().catch(() => null);
    const ask = settings?.confirmLinks ?? true;
    if (ask && !window.confirm(t("Aprire nel browser questo indirizzo?\n\n{url}", { url: href }))) return;
    api.openExternal(href).catch((e) => window.alert(String(e)));
  };

  return (
    <a href={href} title={href} onClick={openIt}>
      {children}
    </a>
  );
}

/**
 * Molti modelli scrivono le formule come `\( ... \)` e `\[ ... \]`; il
 * componente Markdown capisce solo `$ ... $` e `$$ ... $$`. Qui si converte la
 * prima forma nella seconda, lasciando intatto il codice.
 */
export function normalizeMath(text: string): string {
  // Le parti di indice dispari sono codice (blocchi ``` o `in linea`).
  return text
    .split(/(```[\s\S]*?(?:```|$)|`[^`\n]*`)/g)
    .map((part, index) =>
      index % 2 === 1
        ? part
        : part
            .replace(/\\\[([\s\S]+?)\\\]/g, (_, formula) => `\n$$\n${formula.trim()}\n$$\n`)
            .replace(/\\\(([\s\S]+?)\\\)/g, (_, formula) => `$${formula.trim()}$`),
    )
    .join("");
}
