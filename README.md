# Guanlan · 观澜

English | [简体中文](README.zh-CN.md)

A desktop coding agent with a native Rust core. The agent loop, conversation history, context management, streaming response parser, tool definitions, local execution, termination conditions, and error handling are implemented directly in Rust, without third-party agent frameworks or SDKs.

Tauri 2 provides the desktop shell; TypeScript powers the interface. General-purpose libraries handle networking, asynchronous I/O, serialization, and desktop integration.

![Guanlan](docs/home.png)

## Download

Get the Windows x64 installer, `Guanlan_<version>_x64-setup.exe`, or the portable executable, `Guanlan_<version>_x64.exe`, from [Releases](https://github.com/dissein/Guanlan/releases/latest).

## Use

Open **模型与设置** (Model & Settings), select a provider, enter your API key, and choose a workspace. Fetch the model list and filter it by name. Guanlan supports OpenAI-compatible Chat Completions services with streaming and tool calling, including local Ollama endpoints.

Send a task to begin. Execution mode can edit files and run commands; planning mode is for reading and analysis. File changes and commands require approval by default. Glass and solid interface materials are available in settings.

API keys are stored in Windows Credential Manager. Conversations are saved under `.guanlan/sessions/` in the selected workspace. File undo restores changes recorded by file tools; it does not reverse shell, Git, or network operations.

## Features

- Native Rust agent loop, SSE parsing, fragmented tool-call assembly, and multi-step execution.
- 13 local tools for file operations, patches, directory listing, search, PowerShell, plans, questions, skills, web requests, and web search.
- Model discovery, name filtering, streaming responses, and reasoning display.
- Tool approvals, cancellation, file diffs, session switching, and undo for the latest 20 file-tool turns.
- Project instructions from `AGENTS.md`, `CLAUDE.md`, and `GEMINI.md`; skills from `.agents/skills/` and `.guanlan-skills/`.
- Environment and system proxy support for network requests.

Windows is the currently supported platform. Commands run with the current user's permissions; workspace restrictions are not an operating-system sandbox.

## Development

Requirements: Node.js 24, stable Rust, Microsoft C++ Build Tools with the Windows SDK and desktop C++ workload, and WebView2 Runtime.

```powershell
git clone https://github.com/dissein/Guanlan.git
cd Guanlan
npm ci
npm run desktop
```

```powershell
npm test
cargo test --locked -p guanlan-core
npm run package -- --ci
```

`npm run dev` previews the interface only. Local installers are generated in `target/release/bundle/nsis/`.

## Automated releases

GitHub Actions runs the tests and builds the Windows installer and portable executable. Pushing a `v<version>` tag publishes a Release with SHA-256 checksums. The tag must match the version in `package.json`, `Cargo.toml`, and `tauri.conf.json`.

```powershell
git tag v0.1.8
git push origin v0.1.8
```

The workflow can also be started manually from Actions. Selecting a branch produces build artifacts; selecting a version tag publishes a Release.

## Structure

```text
crates/guanlan-core/  Rust agent, streaming parser, tools, and sessions
src-tauri/           Desktop commands, credentials, and packaging
src/                 Interface and assets
tests/               Frontend tests
.github/workflows/   Build and release pipeline
docs/                Screenshots and release notes
```

The bundled Noto Sans SC font is distributed under the [SIL Open Font License](src/assets/OFL.txt).
