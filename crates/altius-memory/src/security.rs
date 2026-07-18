//! Security knowledge records layered on [`crate::KnowledgeStore`].
//!
//! In-memory is first-class; Neo4j persistence is optional.

use std::collections::HashMap;
use std::sync::Arc;

use altius_core::redact_secrets;
use altius_findings::{Confidence, Finding, Severity, ValidationState};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::store::{KnowledgeError, KnowledgeResult};

/// A scan target (contract/program/module).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TargetRecord {
    pub id: String,
    pub chain: String,
    pub path_or_address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
}

/// Persisted vulnerability / finding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VulnerabilityRecord {
    pub fingerprint: String,
    pub pattern_id: String,
    pub severity: String,
    pub confidence: String,
    pub title: String,
    pub description: String,
    pub target_id: String,
    pub scanner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology_class: Option<String>,
    pub validation: String,
    pub created_at: DateTime<Utc>,
}

/// Evidence span supporting a vulnerability.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: String,
    pub vulnerability_fingerprint: String,
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Scanner identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScannerRecord {
    pub name: String,
    pub kind: String,
}

/// Mitigation skill linked to a vulnerability class.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MitigationRecord {
    pub skill_name: String,
    pub vulnerability_fingerprint: String,
    pub detail: String,
}

impl VulnerabilityRecord {
    pub fn from_finding(finding: &Finding, target_id: impl Into<String>) -> Self {
        Self {
            fingerprint: finding.fingerprint.clone(),
            pattern_id: finding.pattern_id.clone(),
            severity: finding.severity.as_str().into(),
            confidence: finding.confidence.as_str().into(),
            title: redact_secrets(&finding.title),
            description: redact_secrets(&finding.description),
            target_id: target_id.into(),
            scanner: finding.tool.clone(),
            ontology_class: finding.ontology_class.clone(),
            validation: match finding.validation {
                ValidationState::Unverified => "unverified",
                ValidationState::ReproducedLocal => "reproduced_local",
                ValidationState::Rejected => "rejected",
            }
            .into(),
            created_at: Utc::now(),
        }
    }
}

/// Extended store APIs for security knowledge.
#[async_trait]
pub trait SecurityKnowledge: Send + Sync {
    async fn upsert_target(&self, target: TargetRecord) -> KnowledgeResult<()>;
    async fn upsert_scanner(&self, scanner: ScannerRecord) -> KnowledgeResult<()>;
    async fn upsert_vulnerability(&self, vuln: VulnerabilityRecord) -> KnowledgeResult<()>;
    async fn add_evidence(&self, evidence: EvidenceRecord) -> KnowledgeResult<()>;
    async fn link_mitigation(&self, mitigation: MitigationRecord) -> KnowledgeResult<()>;
    async fn vulnerabilities_for_target(
        &self,
        target_id: &str,
    ) -> KnowledgeResult<Vec<VulnerabilityRecord>>;
    async fn evidence_for(&self, fingerprint: &str) -> KnowledgeResult<Vec<EvidenceRecord>>;
}

/// Aggregate confidence when multiple scanners report the same fingerprint.
pub fn aggregate_confidence(existing: Confidence, incoming: Confidence) -> Confidence {
    existing.max(incoming)
}

/// Prefer higher severity when merging duplicates.
pub fn aggregate_severity(existing: Severity, incoming: Severity) -> Severity {
    existing.max(incoming)
}

#[derive(Default)]
struct SecurityInner {
    targets: HashMap<String, TargetRecord>,
    scanners: HashMap<String, ScannerRecord>,
    vulns: HashMap<String, VulnerabilityRecord>,
    evidence: Vec<EvidenceRecord>,
    mitigations: Vec<MitigationRecord>,
}

/// In-memory security knowledge (composable with [`crate::InMemoryKnowledgeStore`]).
#[derive(Clone, Default)]
pub struct InMemorySecurityStore {
    inner: Arc<Mutex<SecurityInner>>,
}

impl InMemorySecurityStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SecurityKnowledge for InMemorySecurityStore {
    async fn upsert_target(&self, target: TargetRecord) -> KnowledgeResult<()> {
        self.inner.lock().await.targets.insert(target.id.clone(), target);
        Ok(())
    }

    async fn upsert_scanner(&self, scanner: ScannerRecord) -> KnowledgeResult<()> {
        self.inner
            .lock()
            .await
            .scanners
            .insert(scanner.name.clone(), scanner);
        Ok(())
    }

    async fn upsert_vulnerability(&self, mut vuln: VulnerabilityRecord) -> KnowledgeResult<()> {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.vulns.get(&vuln.fingerprint) {
            // Keep higher confidence / severity labels lexicographically via parse-ish compare.
            let keep_conf = existing.confidence.clone();
            let keep_sev = existing.severity.clone();
            if confidence_rank(&vuln.confidence) < confidence_rank(&keep_conf) {
                vuln.confidence = keep_conf;
            }
            if severity_rank(&vuln.severity) < severity_rank(&keep_sev) {
                vuln.severity = keep_sev;
            }
        }
        guard.vulns.insert(vuln.fingerprint.clone(), vuln);
        Ok(())
    }

    async fn add_evidence(&self, evidence: EvidenceRecord) -> KnowledgeResult<()> {
        let mut guard = self.inner.lock().await;
        if !guard.vulns.contains_key(&evidence.vulnerability_fingerprint) {
            return Err(KnowledgeError::Message(format!(
                "unknown vulnerability {}",
                evidence.vulnerability_fingerprint
            )));
        }
        guard.evidence.push(EvidenceRecord {
            id: evidence.id,
            vulnerability_fingerprint: evidence.vulnerability_fingerprint,
            file: evidence.file,
            start_line: evidence.start_line,
            snippet: evidence.snippet.map(|s| redact_secrets(&s)),
        });
        Ok(())
    }

    async fn link_mitigation(&self, mitigation: MitigationRecord) -> KnowledgeResult<()> {
        self.inner.lock().await.mitigations.push(MitigationRecord {
            skill_name: mitigation.skill_name,
            vulnerability_fingerprint: mitigation.vulnerability_fingerprint,
            detail: redact_secrets(&mitigation.detail),
        });
        Ok(())
    }

    async fn vulnerabilities_for_target(
        &self,
        target_id: &str,
    ) -> KnowledgeResult<Vec<VulnerabilityRecord>> {
        Ok(self
            .inner
            .lock()
            .await
            .vulns
            .values()
            .filter(|v| v.target_id == target_id)
            .cloned()
            .collect())
    }

    async fn evidence_for(&self, fingerprint: &str) -> KnowledgeResult<Vec<EvidenceRecord>> {
        Ok(self
            .inner
            .lock()
            .await
            .evidence
            .iter()
            .filter(|e| e.vulnerability_fingerprint == fingerprint)
            .cloned()
            .collect())
    }
}

fn confidence_rank(value: &str) -> u8 {
    match value {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

fn severity_rank(value: &str) -> u8 {
    match value {
        "critical" => 5,
        "high" => 4,
        "medium" => 3,
        "low" => 2,
        "info" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use altius_findings::Finding;

    #[tokio::test]
    async fn upsert_and_query_finding() {
        let store = InMemorySecurityStore::new();
        store
            .upsert_target(TargetRecord {
                id: "prog-1".into(),
                chain: "solana".into(),
                path_or_address: "programs/vault".into(),
                framework: Some("anchor".into()),
            })
            .await
            .unwrap();
        store
            .upsert_scanner(ScannerRecord {
                name: "altius-svm-tools".into(),
                kind: "native".into(),
            })
            .await
            .unwrap();
        let finding = Finding::from_lint(
            "svm-missing-signer-check",
            false,
            "missing signer",
            "src/lib.rs",
        );
        let vuln = VulnerabilityRecord::from_finding(&finding, "prog-1");
        let fp = vuln.fingerprint.clone();
        store.upsert_vulnerability(vuln).await.unwrap();
        store
            .add_evidence(EvidenceRecord {
                id: "ev1".into(),
                vulnerability_fingerprint: fp.clone(),
                file: "src/lib.rs".into(),
                start_line: Some(10),
                snippet: Some("next_account_info".into()),
            })
            .await
            .unwrap();
        assert_eq!(store.vulnerabilities_for_target("prog-1").await.unwrap().len(), 1);
        assert_eq!(store.evidence_for(&fp).await.unwrap().len(), 1);
    }

    #[test]
    fn confidence_aggregation_prefers_high() {
        assert_eq!(
            aggregate_confidence(Confidence::Low, Confidence::High),
            Confidence::High
        );
    }
}
