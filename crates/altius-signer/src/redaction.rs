//! Heuristics used by the agent's own `Read`/`Grep`/shell tool layer to
//! keep private key material out of the model's context window, per
//! Phase 0 spec §7. These are deliberately conservative (prefer false
//! positives over false negatives): a file that merely *looks* like a
//! keypair being refused is a minor annoyance, a real keypair slipping
//! through is not.
//!
//! This crate does not wire these checks into any tool itself — that
//! belongs to the agent's file-access layer, which is out of scope for
//! this crate. What lives here is the shared, testable policy so every
//! call site uses the same rules.

use std::path::Path;

/// True if `path` matches a naming convention Solana tooling commonly uses
/// for keypair files, so a generic file-reading tool can refuse it (or
/// require an explicit override) before ever opening it.
pub fn looks_like_keypair_path(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    if file_name == "id.json" {
        return true;
    }
    if file_name.to_ascii_lowercase().contains("keypair") {
        return true;
    }

    let path_str = path.to_string_lossy();
    path_str.contains(".config/solana") && file_name.ends_with(".json")
}

/// Best-effort scan for content that looks like a raw Ed25519 keypair or
/// secret key: a JSON array of exactly 64 (or 32) small integers. This is
/// intentionally simple text scanning, not a JSON parser, so it can run
/// cheaply over arbitrary command output or file contents before that
/// content is added to the model's context.
pub fn contains_probable_private_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some((count, end)) = scan_number_array(&text[i..]) {
                if count == 64 || count == 32 {
                    return true;
                }
                i += end.max(1);
                continue;
            }
        }
        i += 1;
    }
    false
}

/// Given text starting at `[`, returns `(count_of_numbers, bytes_consumed)`
/// if what follows is a bracketed, comma-separated list of integers each
/// in 0..=255 — the shape of a JSON-encoded byte array. Returns `None` if
/// the bracketed content is not purely a small-integer list (so normal
/// arrays of other data don't get flagged).
fn scan_number_array(text: &str) -> Option<(usize, usize)> {
    let mut chars = text.char_indices();
    let (_, open) = chars.next()?;
    debug_assert_eq!(open, '[');

    let mut count = 0usize;
    let mut current = String::new();
    for (idx, ch) in chars {
        match ch {
            '0'..='9' => current.push(ch),
            ',' | ' ' | '\n' | '\t' | '\r' => {
                if !current.is_empty() {
                    let value: u32 = current.parse().ok()?;
                    if value > 255 {
                        return None;
                    }
                    count += 1;
                    current.clear();
                }
            }
            ']' => {
                if !current.is_empty() {
                    let value: u32 = current.parse().ok()?;
                    if value > 255 {
                        return None;
                    }
                    count += 1;
                }
                return Some((count, idx + 1));
            }
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn flags_common_keypair_paths() {
        assert!(looks_like_keypair_path(&PathBuf::from("id.json")));
        assert!(looks_like_keypair_path(&PathBuf::from(
            "/home/user/.config/solana/id.json"
        )));
        assert!(looks_like_keypair_path(&PathBuf::from(
            "./program-keypair.json"
        )));
    }

    #[test]
    fn does_not_flag_ordinary_files() {
        assert!(!looks_like_keypair_path(&PathBuf::from("Anchor.toml")));
        assert!(!looks_like_keypair_path(&PathBuf::from("src/lib.rs")));
    }

    #[test]
    fn detects_64_byte_array_as_probable_key() {
        let bytes: Vec<String> = (0..64u32).map(|n| (n % 256).to_string()).collect();
        let text = format!("here is some output: [{}] end", bytes.join(", "));
        assert!(contains_probable_private_key(&text));
    }

    #[test]
    fn ignores_unrelated_arrays() {
        assert!(!contains_probable_private_key("[1, 2, 3]"));
        assert!(!contains_probable_private_key("not an array at all"));
    }
}
