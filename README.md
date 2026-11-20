<div align="center">

<img src="apps/desktop/src-tauri/icons/128x128@2x.png" width="112" alt="PersonAIly Hub logo">

# PersonAIly Hub

**Your local and cloud AI models, in one desktop app.**

Chat, autonomous agents, MCP tools and a VS Code extension, on top of
GGUF models running on your own PC or the cloud providers you already use.

[![Checks](https://github.com/ThomasCaputo/personaily-hub/actions/workflows/ci.yml/badge.svg)](https://github.com/ThomasCaputo/personaily-hub/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/ThomasCaputo/personaily-hub?include_prereleases&label=download)](https://github.com/ThomasCaputo/personaily-hub/releases)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache%202.0-blue)](LICENSE)
![Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D4)

[Download](#download) · [Features](#features) · [Getting started](#getting-started) · [Privacy & security](#privacy--security) · [Contributing](CONTRIBUTING.md)

</div>

---

Today you often need two programs: a chat client for cloud models and a separate
manager for local models. PersonAIly Hub puts both in one place, with the same
chat, the same agents and the same tools whichever model you pick.

## Features

**Local models**
- Run `.gguf` models on your PC through [llama.cpp](https://github.com/ggml-org/llama.cpp),
  which the app downloads and updates for you. Pick the CUDA, Vulkan or CPU-only build, or
  keep several and switch between them.
- Search and download models from Hugging Face (several at once, resumable, models split
  into multiple files included), or import files you already have.
- Image input for local vision models through their `mmproj` component.
- A memory estimate before loading: the model and its KV cache live **either in VRAM or in
  RAM**, never silently split, and you are warned if they don't fit. Models too big for your
  VRAM can be split between GPU and CPU, but only as an explicit, advanced choice.
- Live VRAM/RAM usage, tokens per second and context fill. KV cache quantized to `q8_0`
  by default. Optionally unload the model by itself after some idle minutes.

**Cloud models**
- Anthropic, plus any OpenAI-compatible service (OpenAI, OpenRouter, Groq, Mistral,
  Google AI Studio, …) with your own API key.
- Keys are stored in the Windows Credential Manager, never in plain files.
- Optional spending limits, per month and per conversation.

**Chat**
- Streaming answers you can stop, Markdown with code highlighting and math formulas.
- Attachments (text, PDF, images for vision models) and web search with sources.
- Controlled access to the folders you choose: the model can read, create and edit files there.
- Switch model mid-conversation; edit a message or regenerate the reply and move between
  the versions (branches); export a conversation as Markdown or JSON; full-text search
  through your history; drag files into the chat; `Ctrl+K` command palette.
- **Searchable documents:** index the folders you choose with a local embedding model and
  let the models search them, without anything leaving your PC.
- **Compare** two models side by side on the same message.
- Models reply in the language you write in.
- Per-model parameters (temperature, top-p, context length, GPU layers…) and named presets
  you can export and import.

**Agents**
- Create agents, start them on a task and follow every step: messages, tools used, files touched.
- Pause, resume or stop them; run several at once.
- Undo everything an agent changed with one click (a snapshot is taken before each session).
- Agents can hand parts of a task over to other agents.
- Terminal commands and external tools through [MCP](https://modelcontextprotocol.io),
  always behind your permission rules.

**Integrations**
- A local **OpenAI-compatible API**, so existing tools can use your models.
- A **VS Code extension**: chat in the editor with the open file as context, proposed
  changes shown as diffs to accept or reject, and agents you can follow and approve from
  the editor, plus optional inline code completion with a local code model
  ([details](extensions/vscode/README.md)).
- Closing the window keeps the app running in the notification area: the loaded model,
  working agents and the VS Code extension carry on, while the interface frees its memory.

The interface (and the VS Code extension) is available in **English** and **Italian**, with
a light and a dark theme.

## Download

Get the latest version from the [Releases page](https://github.com/ThomasCaputo/personaily-hub/releases):

| File | What it is |
|---|---|
| `PersonAIly-Hub_<version>_x64-setup.exe` | Installer (recommended). No administrator rights needed, adds a Start menu entry and an uninstaller. Also installs the headless core the VS Code extension uses when the app is closed. |
| `PersonAIly-Hub_<version>_x64-portable.exe` | Single executable, nothing to install. |
| `personaily-hub-vscode_<version>.vsix` | VS Code extension. In VS Code: **Extensions** view → **⋯** → **Install from VSIX…** |
| `SHA256SUMS.txt` | Checksums to verify the downloads. |

> [!NOTE]
> The executables are not code-signed yet, so Windows SmartScreen may show a warning
> the first time. Choose **More info → Run anyway**, or check the file against
> `SHA256SUMS.txt` first.

### Requirements

- Windows 10 or 11, 64-bit, with [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)
  (already included in Windows 11).
- For local models: enough RAM or VRAM for the model you choose. An NVIDIA GPU (CUDA)
  or any Vulkan-capable GPU speeds things up, but the CPU build works everywhere.

## Getting started

1. **Local model:** on the **Models** page, install llama.cpp under **Engine**, then get a
   model from **Download a model** (or add a folder with your `.gguf` files under
   **Folders**) and load it.
2. **Cloud model:** on the **Models** page, add a provider under **Cloud providers** and
   paste your API key.
3. Start a **chat**, or create an **agent** and give it a folder and a task.

## Privacy & security

- Everything stays on your PC: conversations, settings and models are stored locally
  (`%APPDATA%\dev.personaily.hub`). Nothing is sent anywhere except to the cloud
  providers you configure, the web pages the model reads for you and, at startup, a check
  on GitHub for a new version (it sends nothing of yours and can be turned off in
  **Settings → Updates**). No telemetry.
- Updates are signed: the app refuses an update whose signature does not match.
- **Settings → Data backup** saves conversations, agents, settings and presets in one file;
  API keys stay in the Windows Credential Manager and are not included.
- Models can only act where you allow them: granted folders, confirmation before edits
  and commands, and a log of every action.
- The local API listens on `127.0.0.1` only and requires a token.
- Extra protections are **on by default** and can be turned off in **Settings → Security**:
  - *Data-leak protection:* a model that can see your files asks before opening an address
    that came neither from you nor from a search.
  - *Link confirmation:* shows the real address of a link before opening it in your browser.

To report a vulnerability, see [SECURITY.md](SECURITY.md).

## Status

PersonAIly Hub is at its first public version (**0.1**). Windows is the main platform;
Linux and macOS are planned. Upcoming work is tracked in the [roadmap](docs/roadmap.md).

## Contributing

Bug reports, ideas and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md)
explains how to build the app from source and run the checks. The design documents in
[`docs/`](docs/) are written in Italian.

## License

PersonAIly Hub is released under the [Apache License 2.0](LICENSE). See also [NOTICE](NOTICE).

It is built with [Tauri](https://tauri.app), [React](https://react.dev) and
[llama.cpp](https://github.com/ggml-org/llama.cpp) (downloaded at runtime, MIT license).
