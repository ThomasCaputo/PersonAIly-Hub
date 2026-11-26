import { describe, expect, it } from "vitest";
import { splitBlocks } from "./Markdown";

describe("risposta in arrivo divisa in blocchi", () => {
  it("divide sulle righe vuote fuori dal codice", () => {
    expect(splitBlocks("uno\n\ndue")).toEqual(["uno", "due"]);
    expect(splitBlocks("```\na\n\nb\n```\n\ndopo")).toEqual(["```\na\n\nb\n```", "dopo"]);
  });

  it("un blocco aperto con quattro apici non si chiude con tre", () => {
    const text = "````md\n```js\nx\n\n```\n````\n\nfine";
    expect(splitBlocks(text)).toEqual(["````md\n```js\nx\n\n```\n````", "fine"]);
  });

  it("le tilde non chiudono un blocco di apici, né il contrario", () => {
    const text = "```\n~~~\n\n```\n\nfine";
    expect(splitBlocks(text)).toEqual(["```\n~~~\n\n```", "fine"]);
  });

  it("una riga che comincia con apici ma prosegue non chiude il blocco", () => {
    const text = "```\n``` non chiude\n\n```\n\nfine";
    expect(splitBlocks(text)).toEqual(["```\n``` non chiude\n\n```", "fine"]);
  });
});
