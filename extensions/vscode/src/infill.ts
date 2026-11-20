// Parti del completamento in linea che non dipendono da VS Code (provate da `npm test`).

/** Caratteri prima e dopo il cursore mandati al modello. */
export const PREFIX_CHARS = 6000;
export const SUFFIX_CHARS = 2000;

/** Il testo intorno al cursore, tagliato alle righe intere. */
export function contextAround(text: string, offset: number, prefixChars = PREFIX_CHARS, suffixChars = SUFFIX_CHARS) {
  let start = Math.max(0, offset - prefixChars);
  if (start > 0) {
    const newline = text.indexOf("\n", start);
    if (newline !== -1 && newline < offset) start = newline + 1;
  }
  let end = Math.min(text.length, offset + suffixChars);
  if (end < text.length) {
    const newline = text.lastIndexOf("\n", end);
    if (newline > offset) end = newline;
  }
  return { prefix: text.slice(start, offset), suffix: text.slice(offset, end) };
}

/**
 * Ripulisce la proposta: niente spazi in coda e niente ripetizione del testo
 * che c'è già dopo il cursore (i modelli a volte riscrivono la `}` finale).
 */
export function cleanCompletion(completion: string, suffix: string): string {
  let text = completion.replace(/\s+$/, "");
  const next = suffix.replace(/^\s+/, "");
  for (let length = Math.min(text.length, next.length, 200); length > 0; length--) {
    if (text.endsWith(next.slice(0, length))) {
      text = text.slice(0, text.length - length).replace(/\s+$/, "");
      break;
    }
  }
  return text;
}
