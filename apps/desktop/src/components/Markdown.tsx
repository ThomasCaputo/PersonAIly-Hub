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
 *
 * Con `streaming` il testo sta ancora arrivando: invece di rianalizzare tutta
 * la risposta a ogni pezzo (oltre 60 ms a ridisegno con 3000 token, misurato),
 * si divide in blocchi e si rianalizza solo l'ultimo; gli altri non cambiano
 * e `memo` li salta.
 */
export const Markdown = memo(function Markdown({ text, streaming }: { text: string; streaming?: boolean }) {
  return (
    <Suspense fallback={<div className="markdown pre">{text}</div>}>
      {streaming ? (
        <div className="markdown-stream">
          {splitBlocks(text).map((block, i) => (
            <MarkdownRenderer key={i} text={block} />
          ))}
        </div>
      ) : (
        <MarkdownRenderer text={text} />
      )}
    </Suspense>
  );
});

/**
 * Divide il Markdown nei punti in cui un blocco è sicuramente finito: le righe
 * vuote fuori dai blocchi di codice (``` o ~~~) e dalle formule `$$`. Un blocco
 * già chiuso non cambia più mentre il testo cresce.
 */
/** L'apertura di un blocco di codice: almeno tre ` o ~. */
const FENCE = /^ {0,3}(`{3,}|~{3,})/;

export function splitBlocks(text: string): string[] {
  const blocks: string[] = [];
  let current: string[] = [];
  let fence: string | null = null;
  let math = false;
  let open: RegExpExecArray | null;
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (fence) {
      // Chiude solo una recinzione dello stesso carattere, lunga almeno quanto
      // quella d'apertura e senza altro sulla riga: dentro un blocco aperto con
      // ```` le righe ``` sono codice, come nel Markdown vero.
      const close = FENCE.exec(line);
      if (close && close[1][0] === fence[0] && close[1].length >= fence.length && trimmed === close[1]) fence = null;
    } else if ((open = FENCE.exec(line))) {
      fence = open[1];
    } else if (trimmed === "$$" || (trimmed.startsWith("$$") && !trimmed.slice(2).includes("$$"))) {
      math = !math;
    } else if (trimmed === "" && !math && current.length > 0) {
      blocks.push(current.join("\n"));
      current = [];
      continue;
    }
    if (trimmed !== "" || current.length > 0) current.push(line);
  }
  if (current.length > 0 || blocks.length === 0) blocks.push(current.join("\n"));
  return blocks;
}
