# Altius Agent

A VS Code extension for agentic Solana development against
[Altius Code](https://github.com/daemon-blockint-tech/altius-code): dispatch
fleet runs, review guarded security scan findings inline, and walk a
TxGuard-guided deploy — without leaving the editor.

This is the extension bundled as a built-in by the Altius IDE fork scaffold
(`ide/`), but it is a normal extension: it runs in stock VS Code today.

## Features

- **Fleet Dispatch** (Altius activity bar view) — send a prompt to an agent
  (`altius`, `security`, `browser`, ...) via a running `altius fleet serve`
  instance, watch runs update, and resume/deny/cancel runs that pause for
  approval. Talks to the same [BeeAI ACP](https://agentcommunicationprotocol.dev)
  `/runs*` HTTP API as the project's existing PWA thin client
  (`crates/altius-cli/assets/pwa`).
- **Security Findings** (Altius activity bar view) — `Altius: Scan Project
  for Security Findings` runs `altius scan --format json`, surfaces findings
  as editor diagnostics (Problems panel, inline squiggles) and as a tree
  grouped by severity; click a finding to jump to its location.
- **Guarded Deploy** — `Altius: Guarded Deploy (Dry Run)` runs `altius
  deploy --dry-run` (policy + mandatory simulation only; `FailClosed`
  guarantees nothing is ever signed). `Altius: Guarded Deploy (Sign &
  Submit)` runs the real pipeline behind a modal warning and a typed
  confirmation, streaming TxGuard's policy/simulate/diff/audit/sign steps to
  the "Altius" output channel.

## Requirements

- The `altius` CLI on `PATH` (or set `altius.cliPath`) — see the [repo
  README](../../../README.md#ways-to-run) to build it.
- For Fleet Dispatch: a running `altius fleet serve` instance (defaults to
  `http://127.0.0.1:8788`, overridable via `altius.fleetUrl`). Set a bearer
  token with `Altius: Set Fleet Bearer Token` if the server requires one.
- For deploy: `altius-signerd` running and `ALTIUS_SIGNER_SOCKET` set in the
  environment VS Code was launched from (same requirement as the CLI).

## Settings

| Setting | Default | Description |
|---|---|---|
| `altius.cliPath` | `altius` | Path to the `altius` binary. |
| `altius.fleetUrl` | `http://127.0.0.1:8788` | Base URL of `altius fleet serve`. |
| `altius.scanPath` | *(workspace root)* | Path passed to `altius scan --path`, relative to the workspace root. |
| `altius.scanChain` | `auto` | Chain family passed to `altius scan --chain`. |
| `altius.deployProjectPath` | *(workspace root)* | Path passed to `altius deploy --project`, relative to the workspace root. |

## Development

```sh
npm install
npm run compile   # or: npm run watch
```

Press `F5` in VS Code (with this folder open) to launch an Extension
Development Host with the extension loaded.
