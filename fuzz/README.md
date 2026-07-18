# Fuzzing security boundaries

These are real `cargo-fuzz`/libFuzzer targets kept outside the main Cargo
workspace so stable workspace builds and CI are unaffected.

Prerequisites:

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
```

Run either target from this directory:

```sh
cargo +nightly fuzz run wasm_guest_abi_decoder
cargo +nightly fuzz run protocol_json_limits
```

For a bounded smoke run:

```sh
cargo +nightly fuzz run wasm_guest_abi_decoder -- -runs=10000
cargo +nightly fuzz run protocol_json_limits -- -runs=10000
```

- `wasm_guest_abi_decoder` exercises packed pointer/length decoding and guest
  memory bounds without granting a guest any host capability.
- `protocol_json_limits` sends arbitrary bytes through Editor ACP JSON-RPC
  decoding and opaque-JSON size enforcement.

Keep generated corpora and crash artifacts private until reviewed: protocol
inputs can originate from real integrations and may contain sensitive data.
