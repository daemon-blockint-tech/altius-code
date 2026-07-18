# Neodyme workshop knowledge internalization

Altius internalizes **public** Solana security methodology from:

- https://workshop.neodyme.io/index.html
- https://neodyme.io/en/blog/solana_common_pitfalls/

as Altius-owned heuristics and documentation. **No workshop source, PoCs, or
trademarks are copied.** Rule implementations live in
`crates/altius-svm-tools/src/lints/rules.rs`.

## Mapped themes → Altius rule IDs

| Theme (public) | Altius rule ID | Notes |
| --- | --- | --- |
| Missing signer checks | `svm-missing-signer-check` | Stable v1 ID |
| Missing owner checks | `svm-missing-owner-check` | Stable v1 ID |
| Arbitrary CPI | `svm-arbitrary-cpi` | Stable v1 ID |
| Writable account validation | `svm-unvalidated-writable-account` | Stable v1 ID |
| Integer overflow / lamports | `svm-lamports-overflow-risk`, `svm-unchecked-arithmetic` | Checked-math guidance |
| Account close / revival | `svm-close-without-zeroing` | Error severity |
| PDA bump canonicalization | `svm-pda-bump-canonicalization` | Workshop/pitfalls theme |
| Sysvar / ix introspection | `svm-sysvar-address-validation` | Address validation |
| Account confusion / swapping | `svm-account-confusion` | Key-equality theme |
| Remaining accounts | `svm-remaining-accounts-risk` | Anchor pattern |
| Oracle freshness | `svm-oracle-trust-risk` | Staleness/confidence |

## Dynamic validation

Local-only harness: `LocalSequenceHarness` (`Unverified` → optional
`ReproducedLocal` / `Rejected`). No mainnet execution. Optional Trident binary
probe only — never vendored.

## License / provenance

Public educational material informs Altius rule *themes*. Implementations,
messages, and tests are original Altius work under Apache-2.0.
