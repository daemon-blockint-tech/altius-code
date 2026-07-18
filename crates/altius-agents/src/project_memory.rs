//! Project instruction files (`.altius.md` / `ALTIUS.md`).
//!
//! Loaded at node-run time from the project root, redacted, and injected into
//! the specialist system prompt. Skills / slash commands are Phase B.

use std::path::Path;

/// Cap on injected project memory (bytes, after read).
const MAX_MEMORY_BYTES: usize = 12 * 1024;

const CANDIDATES: &[&str] = &[".altius.md", "ALTIUS.md"];

/// Load the first existing project memory file under `project_root`.
/// Returns `None` when absent or empty after trim.
pub fn load(project_root: &Path) -> Option<String> {
    for name in CANDIDATES {
        let path = project_root.join(name);
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let redacted = altius_core::redact_secrets(trimmed);
        let capped = if redacted.len() <= MAX_MEMORY_BYTES {
            redacted
        } else {
            let mut boundary = MAX_MEMORY_BYTES;
            while !redacted.is_char_boundary(boundary) {
                boundary -= 1;
            }
            format!("{}…[truncated]", &redacted[..boundary])
        };
        return Some(capped);
    }
    None
}

/// Format memory for system-prompt injection.
pub fn format_for_system(memory: &str) -> String {
    format!("Project instructions:\n{memory}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_dot_altius_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".altius.md"),
            "Use Anchor. Never deploy mainnet.\napi_key=sk-secret-value-here",
        )
        .unwrap();
        let mem = load(dir.path()).expect("memory");
        assert!(mem.contains("Use Anchor"));
        assert!(!mem.contains("sk-secret-value-here") || mem.contains("[REDACTED]"));
    }

    #[test]
    fn missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_none());
    }
}
