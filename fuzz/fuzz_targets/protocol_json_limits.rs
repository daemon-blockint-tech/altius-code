#![no_main]

use altius_protocol::editor_acp::JsonRpcMessage;
use altius_protocol::limits;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let decoded = JsonRpcMessage::decode(data);
    if decoded.is_ok() {
        assert!(data.len() <= limits::MAX_BODY_BYTES);
    }

    if let Ok(value) = serde_json::from_slice(data) {
        let bounded = limits::bounded_opaque_json("fuzz", &value);
        if serde_json::to_vec(&value)
            .is_ok_and(|encoded| encoded.len() > limits::MAX_OPAQUE_JSON_BYTES)
        {
            assert!(bounded.is_err());
        }
    }
});
