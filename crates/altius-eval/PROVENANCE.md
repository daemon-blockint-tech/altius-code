# Altius Eval Provenance

| Field | Value |
| --- | --- |
| Suite author | Altius |
| Review date | 2026-07-19 |
| License | Apache-2.0 |
| Methodology refs | Trident Arena / Wake Arena (public methodology only) |

## Rules

- Do **not** copy private detectors, leaked prompts, proprietary reports, or
  unlicensed benchmark repositories into this tree.
- Gold labels are Altius-authored fixtures under `fixtures/` or generated in
  tests. The built-in SVM projects are synthetic and contain no third-party
  program source.
- Store source URL + commit hash when importing *ideas*; re-implement detectors
  independently.

## Sources consulted (ideas only)

- https://github.com/Ackee-Blockchain/trident-arena-benchmarks
- https://github.com/Ackee-Blockchain/wake-arena-benchmarks
- https://neodyme.io/en/blog/solana_common_pitfalls/
- https://workshop.neodyme.io/index.html

## Built-in fixture labels

- `fixtures/svm/vulnerable_cross_file`: synthetic missing-signer, arbitrary-CPI,
  and unsafe-close patterns split across files to exercise correlation.
- `fixtures/svm/clean_checked`: synthetic negative control containing signer,
  owner, CPI-target, writable-account, checked-math, and close-data checks.
