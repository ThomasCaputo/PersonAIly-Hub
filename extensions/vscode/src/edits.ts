// Parti delle modifiche al codice che non dipendono da VS Code.

/** Il modello scelto, nella forma dell'API (`ModelRef` del nucleo). */
export type ModelRef = { kind: "local" } | { kind: "cloud"; providerId: string; model: string };

/** Nome del modello per l'endpoint compatibile con OpenAI. */
export function openAiModelName(model: ModelRef): string {
  return model.kind === "local" ? "local" : `${model.providerId}/${model.model}`;
}

export const EDIT_SYSTEM_PROMPT = [
  "Sei un assistente di programmazione che modifica codice.",
  "Ricevi un frammento di un file e un'istruzione.",
  "Rispondi SOLO con il frammento modificato per intero, in un unico blocco di codice delimitato da ```,",
  "senza spiegazioni prima o dopo.",
  "Mantieni lo stile, l'indentazione e tutto ciò che l'istruzione non chiede di cambiare.",
].join(" ");

export interface EditTarget {
  fileName: string;
  languageId: string;
  code: string;
  /** Righe (da 1) della selezione; assenti se si modifica tutto il file. */
  startLine?: number;
  endLine?: number;
}

export function editPrompt(target: EditTarget, instruction: string): string {
  const where =
    target.startLine !== undefined
      ? `Righe ${target.startLine}-${target.endLine} del file ${target.fileName}.`
      : `File intero: ${target.fileName}.`;
  return `${where}\n\n${fence(target.code, target.languageId)}\n\nIstruzione: ${instruction}`;
}

/** Il codice in un blocco delimitato, con abbastanza apici da non chiudersi prima. */
export function fence(code: string, languageId = ""): string {
  const longest = Math.max(2, ...(code.match(/`+/g) ?? []).map((run) => run.length));
  const ticks = "`".repeat(longest + 1);
  return `${ticks}${languageId}\n${code.replace(/\n$/, "")}\n${ticks}`;
}

/**
 * Il codice proposto dal modello: il blocco delimitato più lungo della
 * risposta, oppure tutto il testo se non ce ne sono. `null` se è vuoto.
 */
export function extractCode(reply: string): string | null {
  const blocks: string[] = [];
  const re = /(^|\n)(`{3,}|~{3,})[^\n]*\n([\s\S]*?)\n\2[ \t]*(?=\n|$)/g;
  for (let match = re.exec(reply); match; match = re.exec(reply)) blocks.push(match[3]);
  // Un blocco aperto e mai chiuso (risposta troncata) vale comunque.
  if (blocks.length === 0) {
    const open = reply.match(/(^|\n)(`{3,}|~{3,})[^\n]*\n([\s\S]*)$/);
    if (open) blocks.push(open[3]);
  }
  const code = blocks.length > 0 ? blocks.reduce((a, b) => (b.length > a.length ? b : a)) : reply.trim();
  return code.trim() ? code : null;
}

/**
 * Adatta il codice proposto al punto in cui va: stessi a capo del documento
 * e stessa chiusura (con o senza a capo finale) del testo che sostituisce.
 */
export function fitReplacement(proposed: string, original: string, eol: "\n" | "\r\n"): string {
  let code = reindent(proposed.replace(/\r\n/g, "\n").replace(/\n+$/, ""), original.replace(/\r\n/g, "\n"));
  if (/\r?\n$/.test(original)) code += "\n";
  return eol === "\r\n" ? code.replace(/\n/g, "\r\n") : code;
}

/** Il rientro comune alle righe non vuote. */
function commonIndent(text: string): string {
  const indents = text
    .split("\n")
    .filter((line) => line.trim())
    .map((line) => line.match(/^[ \t]*/)![0]);
  if (indents.length === 0) return "";
  return indents.reduce((a, b) => {
    let i = 0;
    while (i < a.length && i < b.length && a[i] === b[i]) i++;
    return a.slice(0, i);
  });
}

/**
 * I modelli tolgono spesso il rientro alle righe selezionate dentro una
 * funzione: se il codice proposto non ne ha e l'originale sì, lo si rimette.
 */
function reindent(proposed: string, original: string): string {
  const wanted = commonIndent(original);
  if (!wanted || commonIndent(proposed)) return proposed;
  return proposed
    .split("\n")
    .map((line) => (line.trim() ? wanted + line : line))
    .join("\n");
}

/** Il testo con l'intervallo `[start, end)` sostituito. */
export function replaceRange(text: string, start: number, end: number, replacement: string): string {
  return text.slice(0, start) + replacement + text.slice(end);
}

/** Intestazione con cui il contesto dell'editor entra nel messaggio della chat. */
export const CONTEXT_MARK = "[Contesto da VS Code:";

export function contextBlock(target: EditTarget): string {
  const where =
    target.startLine !== undefined
      ? `${target.fileName}, righe ${target.startLine}-${target.endLine} selezionate`
      : `${target.fileName}, file intero`;
  return `${CONTEXT_MARK} ${where}]\n${fence(target.code, target.languageId)}`;
}
