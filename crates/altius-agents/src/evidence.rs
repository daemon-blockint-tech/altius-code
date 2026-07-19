//! Structured tool evidence and deterministic final-answer grounding.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_EXCERPT_BYTES: usize = 512;
const MAX_REFERENCES: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Success,
    Failure,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceEntry {
    pub id: String,
    pub tool_name: String,
    pub status: EvidenceStatus,
    pub result_hash: String,
    pub excerpt: String,
    pub references: Vec<String>,
}

impl EvidenceEntry {
    pub fn from_tool_result(index: usize, tool_name: &str, result: &str) -> Self {
        let parsed = serde_json::from_str::<serde_json::Value>(result).ok();
        let status = match parsed
            .as_ref()
            .and_then(|value| value.get("ok"))
            .and_then(serde_json::Value::as_bool)
        {
            Some(true) => EvidenceStatus::Success,
            Some(false) => EvidenceStatus::Failure,
            None => EvidenceStatus::Unknown,
        };
        let redacted = altius_core::redact_secrets(result);
        let excerpt = truncate(&redacted, MAX_EXCERPT_BYTES);
        let references = parsed.as_ref().map(extract_references).unwrap_or_default();
        let digest = Sha256::digest(result.as_bytes());
        Self {
            id: format!("E{index}"),
            tool_name: tool_name.to_owned(),
            status,
            result_hash: hex::encode(&digest[..8]),
            excerpt,
            references,
        }
    }
}

pub fn format_ledger(entries: &[EvidenceEntry]) -> String {
    if entries.is_empty() {
        return "Evidence ledger: (none; scan/build/simulation claims are unverified)".into();
    }
    let mut out = String::from("Evidence ledger (cite IDs exactly, e.g. [E1]):\n");
    for entry in entries {
        out.push_str(&format!(
            "[{}] tool={} status={:?} hash={}",
            entry.id, entry.tool_name, entry.status, entry.result_hash
        ));
        if !entry.references.is_empty() {
            out.push_str(&format!(" refs={}", entry.references.join(",")));
        }
        out.push('\n');
    }
    out
}

/// Append explicit verification notes when a model makes unsupported success claims.
pub fn ground_final_answer(answer: &str, entries: &[EvidenceEntry]) -> String {
    let lower = answer.to_ascii_lowercase();
    let categories: [(&[&str], &[&str]); 4] = [
        (
            &["test", "tests"],
            &[
                "tests passed",
                "test passed",
                "all tests pass",
                "tests are passing",
            ],
        ),
        (
            &["build", "run_command"],
            &["build passed", "build succeeded", "compiled successfully"],
        ),
        (
            &["scan_project", "lint_project"],
            &[
                "scan passed",
                "scan is clean",
                "no vulnerabilities",
                "lint passed",
            ],
        ),
        (
            &["simulate", "simulation"],
            &["simulation passed", "simulation succeeded"],
        ),
    ];
    let mut notes = Vec::new();
    for (tools, claims) in categories {
        if !claims.iter().any(|claim| lower.contains(claim)) {
            continue;
        }
        let evidence = entries.iter().find(|entry| {
            entry.status == EvidenceStatus::Success
                && tools.iter().any(|tool| entry.tool_name.contains(tool))
        });
        match evidence {
            Some(entry) if !answer.contains(&format!("[{}]", entry.id)) => notes.push(format!(
                "Grounding: this success claim is supported by [{}] (`{}`).",
                entry.id, entry.tool_name
            )),
            Some(_) => {}
            None => notes.push(
                "Grounding: a scan/build/test/simulation success claim above is unverified because no matching successful tool evidence was recorded.".into(),
            ),
        }
    }
    if notes.is_empty() {
        answer.to_owned()
    } else {
        format!("{answer}\n\n{}", notes.join("\n"))
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut boundary = max;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…", &value[..boundary])
}

fn extract_references(value: &serde_json::Value) -> Vec<String> {
    fn walk(value: &serde_json::Value, out: &mut Vec<String>) {
        if out.len() >= MAX_REFERENCES {
            return;
        }
        match value {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    if matches!(
                        key.as_str(),
                        "file" | "path" | "rule_id" | "finding_id" | "id"
                    ) {
                        if let Some(text) = value.as_str() {
                            out.push(format!("{key}:{text}"));
                        }
                    }
                    walk(value, out);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, &mut out);
    out.truncate(MAX_REFERENCES);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_is_bounded_and_extracts_refs() {
        let result = serde_json::json!({
            "ok": true,
            "data": {"findings": [{"rule_id": "SOL-1", "file": "src/lib.rs"}]},
            "padding": "x".repeat(2_000)
        })
        .to_string();
        let entry = EvidenceEntry::from_tool_result(1, "scan_project", &result);
        assert_eq!(entry.status, EvidenceStatus::Success);
        assert!(entry.excerpt.len() <= MAX_EXCERPT_BYTES + 3);
        assert!(entry.references.contains(&"rule_id:SOL-1".into()));
    }

    #[test]
    fn unsupported_claim_is_marked_unverified() {
        let grounded = ground_final_answer("All tests passed.", &[]);
        assert!(grounded.contains("unverified"));
    }

    #[test]
    fn supported_claim_gets_evidence_reference() {
        let entry =
            EvidenceEntry::from_tool_result(1, "run_command", r#"{"ok":true,"data":{"status":0}}"#);
        let grounded = ground_final_answer("The build succeeded.", &[entry]);
        assert!(grounded.contains("[E1]"));
    }
}
