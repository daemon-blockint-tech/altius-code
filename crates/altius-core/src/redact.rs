use regex::Regex;
use std::sync::OnceLock;

/// Redact common secret patterns from free-form text before logging or
/// persisting to Neo4j / MCP responses / model context.
///
/// Patterns covered (case-insensitive where noted):
/// - `api[_-]?key=...` / `authorization: bearer ...`
/// - OpenAI-style `sk-...` tokens
/// - Generic `password=...` / `secret=...` / `token=...` / `private_key=...`
/// - Solana JSON keypair arrays (32 or 64 bytes `0..=255`)
/// - Long base58 blobs adjacent to key/secret/token labels
pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_owned();
    for re in secret_patterns() {
        out = re.replace_all(&out, "[REDACTED]").into_owned();
    }
    out
}

/// True when `text` looks like a Solana-style JSON secret-key array.
/// Used by agent FS / tool envelopes to refuse rather than only redact.
pub fn contains_probable_private_key(text: &str) -> bool {
    keypair_array_pattern().is_match(text)
}

fn keypair_array_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\[\s*(?:(?:1?\d{1,2}|2[0-4]\d|25[0-5])\s*,\s*){31,63}(?:1?\d{1,2}|2[0-4]\d|25[0-5])\s*\]")
            .expect("keypair array regex compiles")
    })
}

fn secret_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let mut patterns: Vec<Regex> = [
            r"(?i)(api[_-]?key\s*[=:]\s*)\S+",
            r"(?i)(authorization\s*:\s*bearer\s+)\S+",
            r"(?i)(password\s*[=:]\s*)\S+",
            r"(?i)(secret\s*[=:]\s*)\S+",
            r"(?i)(token\s*[=:]\s*)\S+",
            r"\bsk-[A-Za-z0-9_\-]{16,}\b",
            r"(?i)(private[_-]?key\s*[=:]\s*)\S+",
            // Labeled long base58 (Solana secret-key style).
            r"(?i)(?:secret|private[_-]?key|keypair)\s*[=:]\s*[1-9A-HJ-NP-Za-km-z]{64,}",
        ]
        .into_iter()
        .map(|p| Regex::new(p).expect("redaction regex compiles"))
        .collect();
        patterns.push(keypair_array_pattern().clone());
        patterns
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_api_key_assignment() {
        let raw = "connecting with api_key=super-secret-value please";
        let redacted = redact_secrets(raw);
        assert!(!redacted.contains("super-secret-value"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_openai_style_key() {
        let raw = "using sk-abcdefghijklmnopqrstuvwxyz123456";
        let redacted = redact_secrets(raw);
        assert!(!redacted.contains("sk-abcdefghijklmnopqrstuvwxyz123456"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn leaves_benign_text() {
        let raw = "detect Anchor project at ./programs/vault";
        assert_eq!(redact_secrets(raw), raw);
    }

    #[test]
    fn redacts_solana_keypair_json_array() {
        let bytes: Vec<String> = (0u8..64).map(|b| b.to_string()).collect();
        let raw = format!("id.json contents: [{}]", bytes.join(", "));
        assert!(contains_probable_private_key(&raw));
        let redacted = redact_secrets(&raw);
        assert!(!redacted.contains("12, 13, 14"));
        assert!(redacted.contains("[REDACTED]"));
    }
}
