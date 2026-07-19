# Altius Code

Altius Code is a security-first blockchain development agent and automation
CLI. The current release provides project detection, multi-chain security
scanning, guarded Solana deployment, a headless multi-agent fleet, and
ACP/A2A/MCP protocol services.

## Overview

Altius Code lives where you already work: the terminal. Instead of copying
snippets back and forth to a chat window, you point it at a project and it
builds an understanding of the codebase, then acts on your behalf — reading
and editing files, running commands, and carrying multi-step tasks through to
completion while keeping you in control.

Under the hood it is a Rust workspace of small, focused crates: a supervisor
+ specialists agent fleet on top of an in-house async graph runtime, native
multi-chain security scanners, and a mandatory guardrail pipeline that routes
every irreversible on-chain action through policy, simulation, approval,
audit, and an isolated signer.

## Features

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
- **Security scanning** — native, read-only vulnerability scanners with a
  CI-friendly SARIF gate (see [Blockchain security scanning](#blockchain-security-scanning)).
- **Multi-agent fleet** — a supervisor routes work to specialists (explorer,
  coder, security, browser) and exposes them over several protocol surfaces.
- **Fleet Web UI** — a built-in Next.js PWA with streaming chat, run history,
  approval cards, agent selector, dark mode, and mobile-responsive layout.
- **Terminal UI** — an interactive ratatui-based TUI for quick local work.

## Workspace map

The repository is a Cargo workspace of focused crates under
[`crates/`](crates/). The `altius` binary lives in `altius-cli`.

| Crate | Role |
|---|---|
| `altius-cli` | The `altius` binary: `detect`, `scan`, `eval`, `deploy`, and `fleet` subcommands. |
| `altius-core` | Shared types (IDs, budgets, errors, secret redaction). |
| `altius-graph` | Tokio-based agent graph runtime (checkpoints, resume, interrupts). |
| `altius-agents` | Fleet agent roles, LLM client trait, supervisor graph, and slash skills. |
| `altius-protocol` | Protocol surfaces: BeeAI ACP runs, Editor ACP JSON-RPC, A2A card/tasks, ANP stubs. |
| `altius-mcp` | Model Context Protocol host and external MCP client. |
| `altius-payments` | x402 / machine-payment challenge handling, settled only through TxGuard. |
| `altius-txguard` | The policy → simulate → diff → approve → audit → sign choke point. |
| `altius-signer` | Isolated signer process (`altius-signerd`) exposing only pubkey/sign over a UDS. |
| `altius-memory` | Knowledge and state layer (Neo4j schema with an in-memory fallback). |
| `altius-ontology` | Domain ontology layer plus an adapter trait for external ontology servers. |
| `altius-wasm-agents` | Capability-limited WASM sandbox host (no imports; fuel/memory caps). |
| `altius-findings` | Canonical multi-chain finding and scan-report models. |
| `altius-detect` | Chain-agnostic project detection registry. |
| `altius-scanners` | Feature-gated native multi-chain security scanners. |
| `altius-svm-detect` | SVM framework detection (Anchor, Pinocchio, native). |
| `altius-svm-tools` | SVM-specific tooling used by scanners and the fleet. |
| `altius-eval` | Security evaluation harness (recall/precision against gold labels). |

## Ways to Run

Build the pinned workspace toolchain and CLI from source:

```sh
cargo build --locked --release -p altius-cli -p altius-signer
./target/release/altius --help
```

### Local CLI

Run `altius detect`, `altius scan`, `altius deploy`, or `altius fleet run`.
Use `altius <command> --help` for command-specific configuration.

### Headless

Run Altius Code non-interactively with a prompt and get results back on
stdout. This makes it scriptable: wire it into CI pipelines, git hooks,
cron jobs, or any automation that needs an AI coding agent without a human
at the keyboard. The `altius fleet run --prompt "…"` command drives the
supervisor graph headlessly, with an `--offline` deterministic mode for
demos and CI.

### Fleet Web UI (PWA)

`altius fleet serve` ships a built-in Next.js PWA at `/app/` — a chat-style
interface for the multi-agent fleet with:

- **Run history sidebar** — list, search, filter by status (active/done/failed)
- **Streaming chat panel** — real-time SSE updates, message roles (user/agent/tool/system)
- **Approval cards** — transaction previews with lamport deltas, invoked programs, compute units
- **Agent selector** — pick from skills advertised in the A2A agent card
- **Dark mode** — persisted to localStorage, Anthropic design tokens
- **Mobile responsive** — sidebar becomes overlay drawer on small screens
- **Keyboard shortcuts** — `Cmd+K` new chat, `Esc` close sidebar, `Cmd+Enter` submit

The UI is a statically-exported Next.js app (source in
[`crates/altius-cli/assets/pwa-next/`](crates/altius-cli/assets/pwa-next/))
built to `out/` and copied into `assets/pwa/` for Axum's `ServeDir`. To rebuild:

```sh
cd crates/altius-cli/assets/pwa-next
pnpm install && pnpm build
cp -r out/* ../pwa/
```

Then start the server:

```sh
altius fleet serve --offline --bind 127.0.0.1:8788
# Open http://127.0.0.1:8788/app/
```

### Terminal UI (TUI)

Running `altius` with no subcommand launches an interactive terminal
interface built with ratatui. It provides a split-pane layout for command
input, output history, and status — useful for quick local work without
leaving the terminal.

```sh
./target/release/altius
```

### Editor Integration (ACP)

Altius Code speaks the [Agent Client Protocol (ACP)](https://agentclientprotocol.com),
so it can be embedded directly in editors that support the protocol. You get
the same agent — codebase awareness, edits, command execution — surfaced
inside your editor's native UI via `altius fleet acp`.

### CLI commands

The `altius` binary groups its functionality into a handful of subcommands:

- `altius detect <project>` — identify the SVM framework at a path.
- `altius scan --path . --chain auto --format json|markdown|sarif` — run the
  read-only security scanners.
- `altius eval` — run the evaluation harness against gold-label fixtures.
- `altius deploy --project .` — build a deployment plan and run every
  transaction through the TxGuard pipeline (supports `--dry-run`).
- `altius fleet run|serve|mcp|acp|a2a` — the multi-agent fleet surfaces.

## Blockchain security scanning

`altius scan` runs native, read-only scanners over a project and emits
structured findings. It auto-detects the chain family (or takes `--chain`
explicitly) and can render results as JSON, Markdown, or
[SARIF](https://sarifweb.azurewebsites.net/) for code-scanning integrations.

- **Output formats:** `--format json` (default), `markdown`, or `sarif`.
- **CI gate:** `--fail-on-findings` exits non-zero when High/Critical
  findings are present, so a scan can fail a pipeline.
- **Chain coverage:** SVM is enabled by default; additional families
  (`evm`, `algorand`, `cairo`, `cosmos`, `ton`) are feature-gated in
  `altius-scanners`.

Scanners are strictly read-only: they inspect source and never sign or
broadcast anything.

## Agent skills and plugin packs

**Slash skills** are short, leading prefixes that force a fleet route. They
are Altius-owned UX sugar over agent-name / `@Mention` routing, not a
third-party marketplace. The built-in skills are defined in
[`crates/altius-agents/src/skills.rs`](crates/altius-agents/src/skills.rs):

| Skill | Routes to |
|---|---|
| `/scan`, `/audit` | Security specialist (read-only scanners) |
| `/browser` | Browser specialist (attached browser MCP server) |
| `/github` | GitHub specialist (attached GitHub MCP server) |
| `/pay` | Supervisor (payment specialist is still stubbed, so policy and prompts still apply) |

**Plugin packs (v0)** are a small JSON manifest that bundles the skills a
deployment advertises plus optional MCP child-process attachments. There is
no install step beyond placing a file and pointing the server at it with
`altius fleet serve --plugin <path>` (or `ALTIUS_FLEET_PLUGIN`). See the
example at
[`examples/plugins/web3-starter.json`](examples/plugins/web3-starter.json)
and the loader in
[`crates/altius-cli/src/plugin.rs`](crates/altius-cli/src/plugin.rs).

### GitHub connector

Altius can attach directly to an authenticated streamable-HTTP GitHub MCP
server for repository inspection and pull-request workflows:

```bash
export GITHUB_TOKEN="<fine-grained-token>"

altius fleet run \
  --github-mcp-url https://api.githubcopilot.com/mcp/ \
  --prompt "/github inspect open pull requests"
```

The connector is read-only by default. To permit branch/file writes and
pull-request creation/update, explicitly add
`--github-access pull-requests`. Merge, delete, release, workflow-dispatch,
and repository-administration tools remain denied. The token value is read
from `GITHUB_TOKEN` (or the variable named by `--github-token-env`) at
connection time; it is never accepted as a CLI value, logged, persisted, or
placed in model context. Use a fine-grained token scoped to only the target
repositories and required operations.

**Remote deployment:** binding to a non-loopback address requires
`--token` or `ALTIUS_FLEET_TOKEN` (loopback-only demos may omit it). Clients
use `Authorization: Bearer <token>`; append `?token=` only on
`/runs/{id}/events` for SSE. See
[`docs/SECURITY_THREAT_MODEL.md`](docs/SECURITY_THREAT_MODEL.md#remote-fleet-altius-fleet-serve).

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

## Development

The workspace builds on stable Rust:

```sh
cargo build --workspace
cargo test --workspace
```

Continuous integration ([`.github/workflows/rust.yml`](.github/workflows/rust.yml))
enforces the same gates locally reproducible commands cover:

- **Formatting:** `cargo fmt --all -- --check`.
- **Clippy (deny warnings):**
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- **Money-path coverage:** `cargo llvm-cov` over the `altius-txguard`,
  `altius-signer`, and `altius-payments` crates with `--fail-under-lines 80`.
  The floor is 80%; measured line coverage on these crates is ~86%.
- **Scan gate:** builds the CLI and runs `altius scan … --format sarif
  --fail-on-findings` against a clean fixture, uploading the SARIF artifact.
- **Neo4j (feature-gated):** compiles and runs the `altius-memory` integration
  test against a real Neo4j service container.

### Fuzzing

Security boundary fuzzers live under [`fuzz/`](fuzz/README.md) and run on the
nightly toolchain with `cargo-fuzz` (kept out of the stable workspace):

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked

# from the fuzz/ directory
cargo +nightly fuzz run wasm_guest_abi_decoder
cargo +nightly fuzz run protocol_json_limits
```

`wasm_guest_abi_decoder` exercises packed pointer/length decoding and guest
memory bounds; `protocol_json_limits` fuzzes Editor ACP JSON-RPC decoding and
opaque-JSON size enforcement. Keep generated corpora and crash artifacts
private until reviewed.

## Status

Altius Code is under active development. Public HTTP fleet services default to
loopback; a non-loopback bind requires `--token` or `ALTIUS_FLEET_TOKEN`.
Production signer deployments must use an owner-only (`0600`) keypair file and
should run `altius-signerd` under a dedicated OS identity. See the
[security threat model](docs/SECURITY_THREAT_MODEL.md) for deployment
assumptions and remaining limitations.

## License

Licensed under the [Apache License 2.0](LICENSE).
