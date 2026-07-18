#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(prefix) = data.get(..8) else {
        return;
    };
    let packed = i64::from_le_bytes(prefix.try_into().expect("eight-byte prefix"));
    let memory = &data[8..];
    let _ = altius_wasm_agents::decode_guest_output(memory, packed);
});
