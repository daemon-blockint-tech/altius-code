use regex::Regex;
use std::sync::OnceLock;

/// Redact common secret patterns from free-form text before logging or
/// persisting to Neo4j / MCP responses.
///
/// Patterns covered (case-insensitive where noted):
/// - `api[_-]?key=...` / `authorization: bearer ...`
/// - OpenAI-style `sk-...` tokens
/// - Solana-ish long base58 blobs labeled as secret/key/token
/// - Generic `password=...` / `secret=...` / `token=...` assignments
pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_owned();
    for re in secret_patterns() {
        out = re.replace_all(&out, "[REDACTED]").into_owned();
    }
    out
}

fn secret_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(?i)(api[_-]?key\s*[=:]\s*)\S+",
            r"(?i)(authorization\s*:\s*bearer\s+)\S+",
            r"(?i)(password\s*[=:]\s*)\S+",
            r"(?i)(secret\s*[=:]\s*)\S+",
            r"(?i)(token\s*[=:]\s*)\S+",
            r"\bsk-[A-Za-z0-9_\-]{16,}\b",
            r"(?i)(private[_-]?key\s*[=:]\s*)\S+",
        ]
        .into_iter()
        .map(|p| Regex::new(p).expect("redaction regex compiles"))
        .collect()
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
}
