# Altius Code

Altius Code is a user-friendly, terminal-based AI coding agent. It runs as a
full-screen TUI that understands your codebase, edits files, executes shell
commands, searches the web, and manages long-running tasks — including
blockchain development workflows. Use it interactively, headlessly for
scripting and CI, or embedded in your editor via the Agent Client Protocol
(ACP).

## Overview

Altius Code lives where you already work: the terminal. Instead of copying
snippets back and forth to a chat window, you point it at a project and it
builds an understanding of the codebase, then acts on your behalf — reading
and editing files, running commands, and carrying multi-step tasks through to
completion while keeping you in control.

## Features

- **Full-screen TUI** — an interactive terminal interface designed to be
  approachable: browse the conversation, review proposed changes, and steer
  the agent without leaving your shell.
- **Codebase understanding** — the agent explores your project structure,
  reads relevant files, and grounds its answers and edits in your actual
  code rather than guesses.
- **File editing** — makes precise, reviewable edits across one file or
  many, from quick fixes to multi-file refactors.
- **Shell command execution** — runs builds, tests, linters, and arbitrary
  commands, then reads the output to verify its own work and iterate.
- **Web search** — pulls in up-to-date documentation, API references, and
  answers from the web when the codebase alone isn't enough.
- **Long-running task management** — kicks off, tracks, and follows up on
  tasks that span many steps or take real time to finish, so larger jobs
  don't need constant supervision.
- **Blockchain development** — supports smart-contract and web3 workflows
  alongside general-purpose software development.

## Ways to Run

### Interactive (TUI)

The default mode. Launch Altius Code in a project directory and work with
the agent conversationally in a full-screen terminal interface — ideal for
day-to-day development, debugging, and code exploration.

### Headless

Run Altius Code non-interactively with a prompt and get results back on
stdout. This makes it scriptable: wire it into CI pipelines, git hooks,
cron jobs, or any automation that needs an AI coding agent without a human
at the keyboard.

### Editor Integration (ACP)

Altius Code speaks the [Agent Client Protocol (ACP)](https://agentclientprotocol.com),
so it can be embedded directly in editors that support the protocol. You get
the same agent — codebase awareness, edits, command execution — surfaced
inside your editor's native UI.

## Protocol Naming: Two Different "ACP"s

Two unrelated protocols share the ACP acronym, and Altius implements both.
To keep them straight, this repo consistently uses:

| Name in this repo | Protocol | Purpose | Where |
|---|---|---|---|
| **Editor ACP** | [Agent Client Protocol](https://agentclientprotocol.com) | Editor ↔ agent (JSON-RPC over stdio; sessions, prompts) | `altius fleet acp`, `altius-protocol::editor_acp` |
| **BeeAI ACP** | [Agent Communication Protocol](https://agentcommunicationprotocol.dev) | Agent ↔ agent (REST run lifecycle: create/get/cancel/resume) | `altius fleet serve`, `altius-protocol::beeacp` |

Related but distinct surfaces: **MCP** ([Model Context Protocol](https://modelcontextprotocol.io))
exposes tools to agents (`altius fleet mcp`), and **A2A**
([Agent2Agent](https://github.com/a2aproject/A2A)) publishes the agent card and
task delegation endpoint (`altius fleet a2a`). See
[`docs/specs/FLEET_ARCHITECTURE.md`](docs/specs/FLEET_ARCHITECTURE.md) for the
full architecture.

## Security and verification

The centralized
[`security threat model`](docs/SECURITY_THREAT_MODEL.md) documents signer
isolation, TxGuard and tool/WASM trust boundaries, simulation-to-sign
limitations, monitoring, incident response, and security terminology.

Security boundary fuzz targets live under [`fuzz/`](fuzz/README.md). They use
real `cargo-fuzz`/libFuzzer harnesses and are intentionally isolated from stable
workspace builds.

## Status

Altius Code is under active development. Installation instructions, full
documentation, and source code are coming soon.

## License

Licensed under the [Apache License 2.0](LICENSE).
