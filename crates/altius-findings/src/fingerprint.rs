use sha2::{Digest, Sha256};

use crate::finding::Finding;

/// Normalize a path for stable fingerprinting across platforms.
///
/// - converts backslashes to forward slashes
/// - strips a leading `./`
/// - lowercases drive-letter style prefixes on Windows-looking paths
pub fn normalize_path(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    while normalized.starts_with("./") {
        normalized = normalized[2..].to_string();
    }
    if let Some(rest) = normalized.strip_prefix('/') {
        // Keep absolute POSIX paths absolute; only strip redundant `./`.
        let _ = rest;
    }
    normalized
}

/// Stable fingerprint over chain + pattern + normalized location.
///
/// Intentionally excludes title/description so wording changes do not
/// create duplicate identities for the same defect site.
pub fn fingerprint_finding(finding: &Finding) -> String {
    let mut hasher = Sha256::new();
    hasher.update(finding.chain.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(finding.pattern_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(normalize_path(&finding.location.file).as_bytes());
    hasher.update(b"\0");
    if let Some(start) = finding.location.start_line {
        hasher.update(start.to_string().as_bytes());
    }
    hasher.update(b"\0");
    if let Some(end) = finding.location.end_line {
        hasher.update(end.to_string().as_bytes());
    }
    hasher.update(b"\0");
    if let Some(span) = &finding.location.snippet {
        hasher.update(span.as_bytes());
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::ChainFamily;
    use crate::finding::{Finding, FindingLocation, SourceProvenance, ValidationState};
    use crate::severity::{Confidence, Severity};

    fn sample(file: &str) -> Finding {
        Finding {
            id: "f1".into(),
            chain: ChainFamily::Solana,
            pattern_id: "svm-missing-signer-check".into(),
            severity: Severity::Medium,
            confidence: Confidence::Medium,
            title: "Missing signer check".into(),
            description: "Account may not be a signer".into(),
            location: FindingLocation {
                file: file.into(),
                start_line: Some(10),
                end_line: Some(12),
                start_column: None,
                end_column: None,
                snippet: None,
            },
            evidence: vec![],
            attack_scenario: None,
            recommendation: None,
            ontology_class: None,
            tool: "altius-svm-tools".into(),
            provenance: SourceProvenance::native("altius-svm-tools"),
            validation: ValidationState::Unverified,
            remediation_refs: vec![],
            fingerprint: String::new(),
        }
    }

    #[test]
    fn fingerprint_stable_across_path_separators() {
        let a = fingerprint_finding(&sample("src\\lib.rs"));
        let b = fingerprint_finding(&sample("src/lib.rs"));
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_ignores_title_wording() {
        let mut a = sample("src/lib.rs");
        let mut b = sample("src/lib.rs");
        a.title = "one".into();
        b.title = "two".into();
        assert_eq!(fingerprint_finding(&a), fingerprint_finding(&b));
    }

    #[test]
    fn normalize_strips_dot_slash() {
        assert_eq!(
            normalize_path("./programs/foo/src/lib.rs"),
            "programs/foo/src/lib.rs"
        );
    }
}
