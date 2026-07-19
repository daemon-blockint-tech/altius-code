# Production readiness checklist

Status snapshot after in-repo production hardening (July 2026). For competitive
positioning and moat priorities, see [MOAT strategy](MOAT_STRATEGY.md).

## Done in this repository

| Area | Status | Notes |
|------|--------|-------|
| **CI lockfile** | Ready | `Cargo.lock` tracked (not in `.gitignore`); CI uses `cargo * --locked` |
| **Formatting / lint / tests** | Green | `cargo fmt --check`, `cargo clippy --locked -D warnings`, `cargo test --locked` |
| **Fleet auth** | Done | Non-loopback bind fails closed without bearer token |
| **BeeAI run persistence** | Done | SQLite `RunStore` (`~/.altius/runs.db` or `--run-db`) |
| **Graph checkpoint persistence** | Done | `SqliteMemoryStore` + `MemoryStoreCheckpointer` (same `runs.db`); BeeAI→graph run id in kv table |
| **HITL resume** | Done | Graph resume from interrupted node; survives process restart when checkpoint exists; full re-run fallback otherwise |
| **TxGuard choke point** | Done | Policy → simulate → diff → approve → audit → signer UDS |
| **Signer isolation** | Done | Keys in `altius-signerd`; socket `0600`, keypair mode checks |
| **Secret redaction** | Done | `altius_core::redact_secrets` before persistence |
| **Protocol bounds** | Done | Size/depth limits on HTTP/JSON inputs |
| **PWA thin client** | Done | `/app/` chat, run list, approval card (typed `beeacp-client.js`) |
| **BeeACP OpenAPI** | Done | OpenAPI 3.1 at `/openapi.json`; utoipa from `altius-protocol::beeacp` |
| **HITL approval wire** | Done | `awaiting` runs carry typed `approval` on snapshots + SSE `event: run` |
| **SARIF scan gate** | Done | GitHub Actions `scan` job on clean fixture |
| **A2A execution** | Done | Real fleet supervisor path (not echo placeholder) |
| **Remote ops bounds** | Done | Fleet concurrency limits + LLM network timeouts |
| **PWA credentials** | Done | Safer token storage/URL cleanup + CSP headers |
| **Owner-only secrets** | Done | Signer socket/keypair + SQLite run/checkpoint DB permissions |
| **Threat model** | Done | [`SECURITY_THREAT_MODEL.md`](SECURITY_THREAT_MODEL.md) |
| **Fleet architecture** | Done | [`specs/FLEET_ARCHITECTURE.md`](specs/FLEET_ARCHITECTURE.md) |

## Known in-repo limitations (documented, not blockers for demo)

| Limitation | Restart / ops behavior |
|------------|------------------------|
| **ANP / did:wba** | Discovery stub; no full verification. |
| **WASM host imports** | No `fs_read` / network imports yet. |

## External / deployment remaining

These are operator responsibilities or future phases — not fixed by committing
Rust alone:

| Item | Action |
|------|--------|
| **TLS termination** | Reverse proxy (nginx, Caddy, cloud LB) in front of fleet bind; never expose raw HTTP on the public internet |
| **Tailscale / private network** | Prefer mesh VPN or private VPC for remote fleet access instead of public `:8788` |
| **Token rotation** | Rotate `ALTIUS_FLEET_TOKEN` on compromise; avoid logging query tokens from SSE URLs |
| **Neo4j CI service** | Optional: enable `neo4j` job in CI for checkpoint/memory integration (workflow exists; feature-gated) |
| **Trusted Devices** | Product UX for pairing browsers/clients to fleet (not implemented) |
| **Payment / x402 P2** | SPL-token settlement, richer payment flows (see FLEET_ARCHITECTURE stubs) |
| **Hardware / KMS signer** | File backend today; production keys should move to HSM/KMS when available |
| **Telemetry export** | Wire tracing subscriber to SIEM; alerts per threat model |
| **Audit log shipping** | Copy hash-chained TxGuard JSONL to protected remote storage |

## Quick validation (local)

```bash
export RUSTUP_TOOLCHAIN=stable
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
```

## Commit readiness

- **`Cargo.lock`:** generate with `cargo generate-lockfile` (or any `--locked`
  build), then `git add Cargo.lock` — ensure `.gitignore` does not list it
  (`git check-ignore -v Cargo.lock` should print nothing).
