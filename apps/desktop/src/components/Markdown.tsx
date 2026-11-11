import { memo } from "react";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import "highlight.js/styles/github-dark.css";
import "katex/dist/katex.min.css";

/** Testo del modello: Markdown con tabelle, codice evidenziato e formule. */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <div className="markdown">
      <ReactMarkdown remarkPlugins={[remarkGfm, remarkMath]} rehypePlugins={[rehypeKatex, rehypeHighlight]}>
        {normalizeMath(text)}
      </ReactMarkdown>
    </div>
  );
});

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
