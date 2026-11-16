// Pannello chat di PersonAIly Hub. Parla con l'estensione tramite postMessage.
// @ts-check
(function () {
  // @ts-ignore: fornita da VS Code ai pannelli web
  const vscode = acquireVsCodeApi();
  const $ = (id) => /** @type {HTMLElement} */ (document.getElementById(id));
  const list = $("messages");
  const input = /** @type {HTMLTextAreaElement} */ ($("input"));
  const sendButton = /** @type {HTMLButtonElement} */ ($("send"));
  const includeContext = /** @type {HTMLInputElement} */ ($("includeContext"));
  const status = $("status");
  // Come CONTEXT_MARKS in src/edits.ts: in italiano e in inglese.
  const CONTEXT_MARKS = ["[Contesto da VS Code:", "[Context from VS Code:"];

  /** Testi già tradotti dall'estensione (messaggio "texts"); chiave = testo italiano. */
  let texts = {};
  function L(text, vars) {
    const out = texts[text] ?? text;
    return vars ? out.replace(/\{(\w+)\}/g, (whole, name) => (name in vars ? String(vars[name]) : whole)) : out;
  }

  let busy = false;
  /** Elemento della risposta in arrivo. */
  let live = null;
  let liveText = "";

  const saved = vscode.getState() || {};
  includeContext.checked = saved.includeContext ?? true;
  includeContext.addEventListener("change", () => vscode.setState({ includeContext: includeContext.checked }));

  function escape(text) {
    return text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  }

  /** Markdown essenziale: blocchi di codice (con pulsanti), `codice`, **grassetto**, a capo. */
  function render(text, withActions) {
    const out = document.createElement("div");
    const re = /(^|\n)(`{3,}|~{3,})([^\n]*)\n([\s\S]*?)(?:\n\2[ \t]*(?=\n|$)|$)/g;
    let last = 0;
    for (let m = re.exec(text); m; m = re.exec(text)) {
      appendProse(out, text.slice(last, m.index));
      appendCode(out, m[4], m[3].trim(), withActions);
      last = re.lastIndex;
    }
    appendProse(out, text.slice(last));
    return out;
  }

  function appendProse(parent, text) {
    if (!text.trim()) return;
    const p = document.createElement("div");
    p.className = "prose";
    p.innerHTML = escape(text.trim())
      .replace(/`([^`\n]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>")
      .replace(/\n/g, "<br>");
    parent.appendChild(p);
  }

  function appendCode(parent, code, language, withActions) {
    const box = document.createElement("div");
    box.className = "code";
    if (withActions) {
      const bar = document.createElement("div");
      bar.className = "code-bar";
      bar.appendChild(Object.assign(document.createElement("span"), { textContent: language }));
      const apply = Object.assign(document.createElement("button"), {
        className: "link",
        textContent: L("Applica come diff"),
        title: L("Mostra la modifica nel file e chiedi conferma"),
      });
      apply.addEventListener("click", () => vscode.postMessage({ type: "apply", code }));
      const copy = Object.assign(document.createElement("button"), { className: "link", textContent: L("Copia") });
      copy.addEventListener("click", async () => {
        await navigator.clipboard.writeText(code);
        copy.textContent = L("Copiato");
        setTimeout(() => (copy.textContent = L("Copia")), 1500);
      });
      bar.append(apply, copy);
      box.appendChild(bar);
    }
    const pre = document.createElement("pre");
    pre.textContent = code;
    box.appendChild(pre);
    parent.appendChild(box);
  }

  /** Il contesto dell'editor si mostra come etichetta, non come codice intero. */
  function renderUser(text) {
    const out = document.createElement("div");
    const mark = CONTEXT_MARKS.find((m) => text.startsWith(m));
    if (mark) {
      const header = text.slice(0, text.indexOf("]") + 1);
      const fenceMatch = text.slice(header.length).match(/^\n(`{3,})[^\n]*\n[\s\S]*?\n\1(\n|$)/);
      if (fenceMatch) {
        const chip = document.createElement("div");
        chip.className = "chip";
        chip.textContent = "📎 " + header.slice(mark.length, -1).trim();
        out.appendChild(chip);
        text = text.slice(header.length + fenceMatch[0].length).trim();
      }
    }
    out.appendChild(render(text, false));
    return out;
  }

  function addMessage(role, content, extra) {
    const el = document.createElement("div");
    el.className = `msg ${role}`;
    if (role === "user") el.appendChild(renderUser(content));
    else if (role === "tool") {
      el.textContent = `${extra.isError ? "⚠️" : "🔧"} ${extra.toolName ?? L("strumento")}: ${content.slice(0, 160)}${content.length > 160 ? "…" : ""}`;
    } else {
      if (content) el.appendChild(render(content, true));
      if (extra?.tools?.length) {
        const t = document.createElement("div");
        t.className = "dim";
        t.textContent = L("Usa: {tools}", { tools: extra.tools.join(", ") });
        el.appendChild(t);
      }
    }
    list.appendChild(el);
    return el;
  }

  function scroll() {
    list.scrollTop = list.scrollHeight;
  }

  function setBusy(value) {
    busy = value;
    sendButton.textContent = value ? L("Interrompi") : L("Invia");
    status.textContent = value ? L("Sto rispondendo…") : "";
  }

  function send() {
    if (busy) {
      vscode.postMessage({ type: "stop" });
      return;
    }
    const text = input.value.trim();
    if (!text) return;
    input.value = "";
    setBusy(true);
    vscode.postMessage({ type: "send", text, includeContext: includeContext.checked });
  }

  sendButton.addEventListener("click", send);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  });
  $("model").addEventListener("click", () => vscode.postMessage({ type: "chooseModel" }));

  function showError(message) {
    const el = document.createElement("div");
    el.className = "msg error";
    el.textContent = message;
    list.appendChild(el);
    scroll();
  }

  window.addEventListener("message", (event) => {
    const msg = event.data;
    switch (msg.type) {
      case "texts":
        texts = msg.texts;
        break;
      case "state": {
        $("model").textContent = msg.model ? `● ${msg.model}` : L("Scegli il modello");
        $("title").textContent = msg.title;
        list.replaceChildren();
        for (const m of msg.messages) addMessage(m.role, m.content, m);
        if (msg.messages.length === 0) {
          const hint = document.createElement("div");
          hint.className = "dim hint";
          hint.textContent = L(
            "Fai una domanda sul codice. Con la casella sotto spuntata il modello vede il file aperto, o solo la selezione. Per modificare direttamente il file: Ctrl+Alt+I.",
          );
          list.appendChild(hint);
        }
        live = null;
        liveText = "";
        scroll();
        break;
      }
      case "pending":
        list.querySelector(".hint")?.remove();
        addMessage("user", msg.text);
        scroll();
        break;
      case "text":
      case "reasoning":
        if (msg.type === "reasoning") {
          status.textContent = L("Sto ragionando…");
          break;
        }
        status.textContent = L("Sto rispondendo…");
        if (!live) live = addMessage("assistant", "");
        liveText += msg.text;
        live.replaceChildren(render(liveText, false));
        scroll();
        break;
      case "toolRequest": {
        const r = msg.request;
        const box = document.createElement("div");
        box.className = "msg request";
        box.appendChild(Object.assign(document.createElement("div"), { textContent: L("Il modello chiede: {summary}", { summary: r.summary }) }));
        const row = document.createElement("div");
        row.className = "row";
        for (const [approval, label] of [
          ["once", "Consenti"],
          ["always", "Consenti sempre"],
          ["deny", "Nega"],
        ]) {
          const b = Object.assign(document.createElement("button"), { textContent: L(label) });
          if (approval !== "once") b.className = "secondary";
          b.addEventListener("click", () => {
            vscode.postMessage({ type: "approve", id: r.id, approval });
            box.remove();
          });
          row.appendChild(b);
        }
        box.appendChild(row);
        list.appendChild(box);
        scroll();
        break;
      }
      case "notice":
        if (msg.text) showError(msg.text);
        break;
      case "error":
        showError(msg.message);
        break;
      case "done":
        setBusy(false);
        break;
    }
  });

  vscode.postMessage({ type: "ready" });
})();
