# User guide

How to use PersonAIly Hub, from installation to your first agent.
The Italian version is in [guida-utente.md](guida-utente.md).

- [Installation](#installation)
- [First start](#first-start)
- [Local models](#local-models)
- [Cloud models](#cloud-models)
- [Chat](#chat)
- [Agents and permissions](#agents-and-permissions)
- [MCP servers](#mcp-servers)
- [Skills](#skills)
- [Plugins](#plugins)
- [Searchable documents](#searchable-documents)
- [VS Code](#vs-code)
- [Backup, updates and bug reports](#backup-updates-and-bug-reports)
- [Keyboard and accessibility](#keyboard-and-accessibility)
- [Troubleshooting](#troubleshooting)

## Installation

PersonAIly Hub 1.0 runs on **64-bit Windows 10 and 11**. From the
[Releases page](https://github.com/ThomasCaputo/personaily-hub/releases) download:

- `PersonAIly-Hub_<version>_x64-setup.exe`, the installer (recommended): it needs no
  administrator rights, adds a Start menu entry and receives updates;
- or `PersonAIly-Hub_<version>_x64-portable.exe`, a single file you can run from anywhere,
  with nothing to install (updates are downloaded by hand).

If Windows SmartScreen says the app is unknown, choose **More info → Run anyway**, or check
the file against `SHA256SUMS.txt` first.

## First start

The first time you open the app, a short guided setup:

1. shows your graphics card and RAM;
2. lets you choose between **a model on your computer** (free and private) and **a cloud
   service** (with your own API key);
3. for a local model, installs the right llama.cpp build (CUDA with an NVIDIA GPU, Vulkan
   with AMD and Intel) and suggests a model that fits your memory, downloaded with one click.

You can skip it and run it later: `Ctrl+K` → **First-run guided setup**.

## Local models

Everything happens on the **Models** page.

- **Engine:** llama.cpp, the program that runs the models. The app downloads and updates it;
  you can keep several builds (CUDA, Vulkan, CPU only) and choose which one to use.
- **Folders:** where your `.gguf` files live. Models downloaded by the app go into one of them.
- **Download a model:** search Hugging Face, pick a quantization (Q4_K_M is a good trade-off)
  and download. Downloads can be stopped and resumed.
- **Loading a model:** open its card, choose the parameters and press **Load**.

**Where the model lives (placement).** The model and its working memory (KV cache) live
**either entirely in VRAM** (fast) **or entirely in RAM** (slower, but there is usually more of
it). Before loading, the app estimates how much memory is needed and warns you if it does not
fit. For models too big for your VRAM there is the **hybrid** placement: choose how many layers
go on the GPU and the rest stays in RAM.

**Context.** How much of the conversation the model can read. Longer context needs more
memory. A bar under the chat shows how full it is.

**Images.** Some models read images through an `mmproj` file: download it with the model and
tick the vision component when loading.

**Automatic unload.** In Models → Engine the model can be unloaded after a few idle minutes to
free the memory.

**Several models together.** Turn on **Several local models together** in Models → Engine: from
then on **Load** adds the model next to the ones already loaded, as long as it fits in your PC's
memory. The app always keeps a margin free for the system (5% of VRAM and 10% of RAM, with a
minimum), so a PC with more memory holds more models. If you want a cap, write it in **At
most**. If a model does not fit, its card offers **Unload the others and load**.

**Loaded models** lists every model with its memory. One of them is the **primary** one: it is
used wherever you do not pick a model (chats set to “The primary one”, agents created before,
`local` in the API, VS Code); usually it is the last one you loaded, and **Make primary** changes
it. In chat, in model comparison and in agents the model picker lists every loaded model by
name.

## Cloud models

In **Models → Cloud providers** add a service: Anthropic, or any OpenAI-compatible one (OpenAI,
OpenRouter, Groq, Mistral, Google AI Studio…). Paste the API key and press **Refresh models**.

- The key is stored in the **Windows Credential Manager**, never in a file.
- For models with a known price the app estimates the cost of every reply. With Anthropic it
  uses the service's prompt caching, so long conversations and agents cost much less.
- In **Settings → Cloud spending** you can set a monthly and a per-conversation limit.
- **Fallback models:** a queue of cloud models to try, in order, when the chosen one returns an
  error (for example "model overloaded" or "too many requests" on free plans). The reply starts
  again with the first model in the queue that works; a note above the reply says who answered
  instead of whom, and every message carries the name of its model. In **Settings → Fallback
  models** you set up the default queue; every chat ("Fallback" menu above the messages) and
  every agent with a cloud model (in its form) follows it, or has its own, which starts as a
  copy of the default one. It does not apply to model comparison.

## Chat

- Pick the model at the top; you can switch mid-conversation.
- **Enter** sends, **Shift+Enter** starts a new line, **Stop** ends the reply.
- **Attachments:** 📎 or drag files into the window (text, PDF, images).
- **Edit** one of your messages or **Regenerate** the last reply: the old version is kept and
  you get it back with the ‹ › arrows.
- **Tools** (above the messages): files in the granted folders, web search, documents. The
  permission level decides what the model may do without asking.
- **Export** saves the conversation as Markdown (or JSON, with every branch).
- The right-hand panel holds the parameters (temperature, top-p, context…) and the system
  prompt, saved per model. Named presets can be exported and imported.
- **Compare:** the page of that name sends the same message to two models side by side. Both conversations are temporary: "Save as conversation" (or "Save both") keeps them in the chat, the others are deleted with "New comparison" or when you close the app.

## Agents and permissions

An **agent** is an assistant that works on a task by itself: it reads and writes the files in
its folders, uses the terminal (PowerShell) and the tools of MCP servers.

1. In **Agents → Agents** create one: name, model, instructions, working folders, tools and
   permission level.
2. In **Sessions → New task** pick the agent and write the task.
3. Follow every step: messages, tools used, files touched. You can **pause**, **stop** or reply
   to the agent to make it continue.

**Permissions.** Three levels:

| Level | Without asking | Asks first |
|---|---|---|
| Always ask | reading | writing, editing, commands |
| Allow file changes | reading, writing, editing | commands |
| Allow everything | everything | — |

When the agent asks, you see exactly what it would do: **Allow** (once), **Always allow in
this session** or **Deny**. No level lets it outside the granted folders.

**Restore point.** Before every session the app takes a snapshot of the agent's folders (git
must be installed): **Undo the changes** puts them back as they were.

**Limits.** *Maximum steps* stops the agent after a number of rounds; *maximum tokens per step*
cuts off a step that runs too long (it happens with small local models) and makes the agent try
again more briefly.

**Delegation.** An agent can hand parts of a task to other agents you choose; each works with
its own folders and permissions.

**Agents working together.** Each agent can have **its own local model**: pick it in the model
picker, also among the ones not loaded yet (“Loaded when it starts”). When the agent starts, the
app loads its model if it is missing, as long as it fits next to the others. So you can have,
for example, a coordinator on Qwen2.5 7B handing pieces of work to helpers on a smaller model.
If the coordinator hands out several pieces in the same turn, the helpers work **together** and
their reports come back together. Helpers need a model that can use tools (for example
Qwen2.5): the app warns you if a model does not support them.

### Scheduled agents

An agent can work by itself at a set time or at intervals: for example every morning at 8, or
every 6 hours. In **Agents → Schedules** (or with **Schedule** on the agent's card) pick the
agent, write the task out in full and choose when: once, every day (also only on some days) or
every few hours.

- **Local model.** If the agent uses a local model, choose which one: it is loaded at the set
  time and unloaded when the work is done, if the schedule loaded it and nobody else used it in
  the meantime. If another model is already loaded, the schedule **waits** for you to unload
  it, or **replaces** it as soon as nobody is using it, as you prefer. After an hour of waiting
  it skips the run.
- **Nobody to approve.** While it works on its own the agent cannot ask you anything: whatever
  would ask for approval is **refused**. The form tells you which actions will be refused with
  the agent's current permissions; to allow them, change the agent's permissions. If you reply
  to the agent in a scheduled session, approvals are asked again.
- **Result.** Every run is a normal session marked "scheduled": you find it in **Sessions** with
  its steps, touched files and restore point. A Windows notification tells you how it went
  (always, only if something goes wrong, or never). **Run now** tries the task straight away,
  with the same rules.
- **App closed or PC off.** Schedules run while the app is on, even only in the notification
  area. With **Start with Windows** the app starts by itself when you sign in, without a window.
  A missed run is done once as soon as the app is back on, or skipped, as you choose; the PC is
  not woken from sleep.

## MCP servers

[MCP](https://modelcontextprotocol.io) servers add tools to chats and agents (GitHub, a
browser, databases…). Set them up in **Settings → MCP servers**:

- **Known servers:** a list of popular servers to add with one click (some ask you to fill in a
  path or a token);
- **Program on this computer (stdio):** a command such as `npx -y @name/server`;
- **Server at an address (HTTP):** the `https://…` address of a remote server. To identify
  yourself choose:
  - **Headers**, with a token you write (usually `Authorization: Bearer <token>`);
  - **Sign in from the browser (OAuth)**, for services such as Notion or Linear: save, press
    **Sign in**, then **Open in browser** and complete the sign-in on the service's page. The app
    renews the token by itself; **Sign out** deletes it. Services that do not let the app
    register itself (for example GitHub) ask you to register your own OAuth app, with redirect
    address `http://127.0.0.1/callback`, and to enter its Client ID in the form.

Environment variables, headers and sign-in tokens are stored in the Credential Manager. **Test** starts the
server and lists its tools. Then tick the servers to use in the agent editor. If the server
offers resources (documents, files), the agent can list and read them. MCP tools always ask for
confirmation unless the agent has "Allow everything".

**In chat:** open the **MCP servers** menu in the tools bar above the messages and tick the
servers for that conversation. They start with the next message and stay running while you
chat; after 10 minutes without messages they close and start again when needed. In chat each
MCP tool asks for confirmation until you choose **Always allow in this conversation** for that
tool: "Allow everything in the folders" does not cover them.

**`/` commands:** if the conversation's servers offer prompts (ready-made texts, sometimes with
arguments), type `/` at the start of the message: the list appears and narrows as you type.
Choose with the arrows and Enter (or a click), fill in any arguments, and the prompt's text
lands in the message box, to review and send. If the server can suggest values for an argument,
a list appears under its box as you type: pick one with the arrows and Enter (or a click), or keep
typing.

## Skills

A skill is a set of ready-made instructions for a certain task (writing minutes, reviewing a
text with your rules…), in the open `SKILL.md` format, the same one Claude uses: skills
already written for other programs work here too. Manage them in **Settings → Skills**:

- **New skill:** write a name (lowercase letters, digits and hyphens), when to use it and the
  instructions;
- **Import a folder** or **Import a .zip:** a skill with its `SKILL.md` and any supporting files
  (templates, examples);
- the checkbox next to each skill turns it on or off; **Edit** changes its `SKILL.md`.

Then, in a chat or an agent, turn on the **Skills** tool: the model sees only the name and
description of the skills that are on and, when a request matches, loads the instructions and
the files it needs. Reading a skill asks for no confirmation; everything else (files, web,
terminal) follows the usual permissions. Import only skills you trust: the model follows their
instructions as if you had written them. Skills are included in backups.

## Plugins

A plugin is a package with skills, MCP servers, agents and `/` commands, in the Claude Code plugin
format: plugins already published install as they are. In **Settings → Plugins** choose **Import
a folder** or **Import a .zip**. The app copies the plugin, creates its parts and shows any notes,
for example server variables to fill in or parts it does not support (such as *hooks*).

- **Skills** appear among the skills, turned on.
- **MCP servers** appear among the servers: none starts until you choose it in a conversation or
  an agent.
- **Agents** appear on the Agents page, without folders: choose them before using the agents.
- **Commands** appear in the `/` menu of every conversation: pick one, write any arguments and the
  text lands in the message box.

A plugin's skills and servers carry a "plugin …" tag and are removed only with **Uninstall**, which
deletes everything the plugin created. Reinstalling a plugin that is already there replaces it:
changes made to its servers and agents are lost. Trust a plugin as you would a program: install
only the ones you trust. Plugins are included in backups.

## Searchable documents

In **Settings → Searchable documents** choose an embedding model (for example nomic-embed-text
or multilingual-e5, in GGUF format) and the folders to index (among the granted ones). Then, in
chat or in agents, turn on the **Documents** tool: the model searches for the passages closest
to the question. Everything stays on your computer.

## VS Code

Install `personaily-hub-vscode_<version>.vsix` (in VS Code: **Extensions → ⋯ → Install from
VSIX…**). The extension talks to the app or, when the app is closed, starts the headless core
(`app-server`) by itself.

- A chat panel with the app's conversations and the open file as context.
- `Ctrl+Alt+I` asks for a change to the selection: it arrives as a diff to accept or reject.
- Agents to start and follow, with confirmations as notifications.
- Inline code completion with a local code model (for example Qwen2.5-Coder), off by default.

The address and token of the local API are in **Settings → Local API**: other
OpenAI-compatible programs can use them too.

## Backup, updates and bug reports

- **Settings → Data backup:** saves conversations, agents, settings, presets, skills and plugins in a `.zip`;
  **Restore from a backup** puts it back at the next start. API keys stay in the Credential
  Manager.
- **Settings → Updates:** at startup the app checks for a new version, downloads it, verifies
  its signature and installs it. The check can be turned off.
- **Settings → Reporting a problem:** **Copy diagnostic information** prepares a text to attach
  to a bug report, without keys, conversations or user name.
- Data lives in `%APPDATA%\dev.personaily.hub`.

## Keyboard and accessibility

- `Ctrl+K` opens the command palette: pages, new conversation, export, theme, conversation
  search.
- `Tab` right after opening reaches the **Skip to content** link.
- In the conversation list `F2` renames the selected one.
- In permission requests the buttons are reachable with `Tab`; `Esc` closes the palette and the
  guided setup.
- Screen readers announce the model's reply when it is complete, errors and permission
  requests. Both the light and the dark theme meet the WCAG AA contrast ratios.

## Troubleshooting

**The model does not fit in memory.** Reduce the context, use the `q8_0` KV cache (already the
default), a smaller quantization, or the RAM / hybrid placement.

**The model is slow.** In RAM that is expected; in VRAM, look for the "spilling into RAM"
warning: VRAM is not enough and the driver is using system memory.

**A local agent gets stuck.** Small models sometimes repeat the same path for minutes: the
per-step token limit stops them. Giving the agent a single folder and a precise task helps too.

**SmartScreen warns at every installation.** The executables are not yet signed with an
Authenticode certificate; update signatures, on the other hand, are always checked.
