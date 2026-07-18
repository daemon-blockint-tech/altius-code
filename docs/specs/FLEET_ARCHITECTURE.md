# Altius Multi-Agent Fleet Architecture

Status: living document. Covers the fleet crates layered on top of the
Phase-0 SVM tooling described in
[`FASE-0_SVM_INTEGRATION_SPEC.md`](../FASE-0_SVM_INTEGRATION_SPEC.md).

## 1. Overview

The fleet is a supervisor + specialists topology running on an Altius-owned
Tokio graph runtime. Multiple protocol surfaces feed the same supervisor;
all tools flow through a controlled tool plane; all irreversible on-chain
actions (deploys, transfers, x402 payments) flow through the mandatory
TxGuard pipeline and the isolated signer.

```mermaid
flowchart TB
  subgraph ingress [Ingress]
    CLI[altius fleet run]
    EditorACP[altius fleet acp - Editor ACP stdio]
    BeeACP[altius fleet serve - BeeAI ACP REST]
    A2ASrv[altius fleet a2a - Agent Card + tasks]
  end

  subgraph fleet [Fleet Core]
    Router[Supervisor router]
    Graph[altius-graph runtime]
    Workers[Specialist agents]
  end

  subgraph tools [Tool Plane]
    MCPHost[altius-mcp - MCP server]
    SvmTools[altius-svm-detect / tools]
    Pay[altius-payments x402]
    Onto[altius-ontology]
    Wasm[altius-wasm-agents host]
  end

  subgraph state [State and Knowledge]
    Memory[altius-memory - Neo4j / in-memory]
    Audit[.altius txlog + JSONL trajectories]
  end

  CLI --> Router
  EditorACP --> Router
  BeeACP --> Router
  A2ASrv --> Router
  Router --> Graph --> Workers
  Workers --> MCPHost --> SvmTools
  Workers --> Pay
  Workers --> Onto
  Workers --> Wasm
  Graph --> Memory
  Pay -->|TxRequest| TxGuard[altius-txguard]
  TxGuard --> Signer[altius-signer]
  TxGuard --> Audit
```

## 2. Protocol naming (critical)

Two unrelated protocols share the "ACP" acronym; this repo never uses the
bare acronym in code:

- **Editor ACP** — the [Agent Client Protocol](https://agentclientprotocol.com)
  (editor ↔ agent). Implemented in `altius-protocol::editor_acp` as a typed
  JSON-RPC codec (`initialize`, `session/new`, `session/prompt`,
  `session/cancel`) and served by `altius fleet acp` over stdio.
- **BeeAI ACP** — the [Agent Communication Protocol](https://agentcommunicationprotocol.dev)
  (agent ↔ agent). Implemented in `altius-protocol::beeacp` as the REST run
  lifecycle (`created | in-progress | awaiting | completed | failed |
  cancelled`) and served by `altius fleet serve` at `/runs`.
- **MCP** — the [Model Context Protocol](https://modelcontextprotocol.io):
  tools/resources for agents. `altius-mcp` exposes the safe SVM tools
  (detect/build/test/lint) over stdio or HTTP via `altius fleet mcp`.
- **A2A** — [Agent2Agent](https://github.com/a2aproject/A2A): opaque agent
  interoperability. `altius-protocol::a2a` publishes the agent card at
  `/.well-known/agent-card.json` plus a task endpoint (`altius fleet a2a`,
  also merged into `fleet serve`).
- **ANP** — [Agent Network Protocol](https://github.com/agent-network-protocol/AgentNetworkProtocol):
  identity/discovery. `altius-protocol::anp` carries description/discovery
  stubs; `did:wba` verification is future work.

One more disambiguation: `altius-ontology` is about OWL/RDF-style *domain
schemas* (SVM security concepts), not the Ontology blockchain. Ontology-chain
WASM CDT tooling would live as an optional specialist on `altius-wasm-agents`.

## 3. Crate map

| Crate | Layer | Role |
|---|---|---|
| `altius-core` | shared | IDs (`RunId`, `StepId`, …), budgets, redaction |
| `altius-graph` | fleet core | Tokio graph runtime: nodes, edges, checkpoints, fan-out/fan-in, HITL interrupts; `MemoryStore` trait |
| `altius-agents` | fleet core | Role prompt/policy packs + supervisor graph (router → explorer/coder → critic → finalize) |
| `altius-mcp` | tool plane | MCP server wrapping detect/build/test/lint |
| `altius-protocol` | ingress | Editor ACP codec, BeeAI ACP runs, A2A card/tasks, ANP stubs, shared input limits |
| `altius-payments` | tool plane | x402 402-challenge parsing → `TxKind::Payment` `TxRequest` → settlement **only** via `TxGuard::submit` → `X-PAYMENT` proof header |
| `altius-memory` | state | Neo4j knowledge graph (feature `neo4j`) + in-memory fallback; redacted JSONL trajectory logging |
| `altius-ontology` | knowledge | Built-in SVM/security domain schema + `OntologyClient` adapter trait for external ontology MCP servers |
| `altius-wasm-agents` | tool plane | Capability-limited WASM module registry (deny-by-default); execution runtime is a deliberate stub |
| `altius-txguard` | guardrail | Policy → simulate → diff → approve → audit → sign; `TxKind::Payment` is irreversible and approval-gated by default |
| `altius-signer` | guardrail | Isolated signer process, `Pubkey`/`Sign` only |
| `altius-cli` | ingress | `altius detect | deploy | fleet run|serve|mcp|acp|a2a` |

Dependency direction stays acyclic:
`cli → agents/protocol → graph/mcp/memory/payments → core/txguard/svm-*`.

## 4. Agent topology

| Agent | Responsibility | Dangerous tools |
|---|---|---|
| `router` | Decompose, route, merge, enforce budgets | none |
| `explorer` | Codebase search / intelligence | read-only |
| `coder` | Edits, builds, tests | writes files; no signing |
| `security` | Lint/audit review | read-only |
| `deployer` | Produces `TxRequest`s only | must call TxGuard |
| `payment` | x402 paid API calls | must call TxGuard (`TxKind::Payment`) |
| `knowledge` | Neo4j + ontology queries | schema-gated graph writes |
| `critic` | Trajectory QA before finalize | none |

Router, explorer, coder, and critic are live graph nodes; security,
deployer, payment, and knowledge have prompt/policy packs and backing crates
but their graph-node wiring is still pending (see `stub_roles()` in
`altius-agents`).

## 5. Payments (x402) flow

1. An agent's HTTP call returns `402 Payment Required` with an x402 JSON
   challenge (`x402Version`, `accepts[]`).
2. `altius_payments::PaymentChallenge::parse` validates it as untrusted
   input; `select_solana_requirement` picks an `exact`-scheme, known-network,
   native-SOL requirement (SPL assets are rejected for now).
3. `build_payment_request` produces a `TxRequest` with
   `TxKind::Payment { lamports }`.
4. `settle_via_guard` submits it through `TxGuard::submit` — policy
   (`Payment` sits in the default `deny_instructions`, so approval is always
   required; `max_lamports_out` caps the amount), mandatory simulation, diff,
   approval, audit log, and only then the isolated signer.
5. The signed transaction becomes an `X-PAYMENT` proof header
   (`PaymentProof`) for the HTTP retry. Headless configurations
   (`FailClosed` / `AutoApprove`) deny payments; there is no bypass.

## 6. Knowledge and state

- **Per-run state:** `altius-graph` checkpoints typed state after each node
  (`Checkpointer`), through the `MemoryStore` trait (in-memory default).
- **Cross-session knowledge:** `altius-memory` persists `Run`, `Step`,
  `Artifact`, `Contract`, `Vulnerability`, `Skill` nodes with `EXECUTED`,
  `HAS_STEP`, `PRODUCED`, `CALLED`, `DEPLOYED`, `PAID`,
  `HAS_VULNERABILITY`, `HAS_SKILL`, `HAS_CHECKPOINT` relationships. Schema
  statements are idempotent (`IF NOT EXISTS`) and applied at startup.
- **Trajectories:** `JsonlTrajectoryLogger` appends redacted per-step events
  as JSONL, independent of Neo4j.
- Neo4j is always optional: feature `neo4j`, in-memory fallback for tests
  and offline CI. Locally: `docker compose up -d neo4j`, then
  `ALTIUS_NEO4J_URI=bolt://127.0.0.1:7687 cargo test -p altius-memory
  --features neo4j`.

## 7. Security invariants (non-negotiable)

- No private keys in model context; the signer API stays `Pubkey`/`Sign`.
- No path to broadcast without `TxGuard::submit`; `altius-payments` has no
  signer access of its own.
- All remote protocol inputs (MCP, BeeAI ACP, A2A, ANP, x402 challenges,
  ontology data) are untrusted and bounds-checked (`altius-protocol::limits`
  and per-crate validation).
- Payment and mainnet actions require human approval; headless defaults
  deny.
- Secrets are redacted (`altius_core::redact_secrets`) before anything is
  persisted to Neo4j or trajectory files.
- WASM specialists get deny-by-default capabilities and no signing
  capability exists at all.

## 8. Intentional stubs / future work

- ANP `did:wba` verification and full discovery.
- WASM execution runtime (wasmtime-class with fuel metering) behind the
  existing `WasmAgentHost` API.
- MCP client-side attach for external servers (agent-lsp, ontology MCP).
- Graph-node wiring for security/deployer/payment/knowledge specialists.
- SPL-token x402 settlement.
- Eval harness; adversarial prompt-injection fixtures only if explicitly
  enabled (no third-party leaked prompts, ever).
