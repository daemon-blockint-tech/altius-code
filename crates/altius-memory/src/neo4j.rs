//! Neo4j-backed [`KnowledgeStore`] (feature `neo4j`).
//!
//! Persists the schema in [`crate::schema`] via `neo4rs`. This module is
//! feature-gated and network-dependent: unit tests and offline CI use
//! [`crate::InMemoryKnowledgeStore`]; integration against a real Neo4j
//! service runs only when `ALTIUS_NEO4J_URI` is set (see docker-compose.yml).

use altius_core::{RunId, StepId};
use async_trait::async_trait;

use crate::schema::schema_statements;
use crate::security::{
    EvidenceRecord, MitigationRecord, ScannerRecord, SecurityKnowledge, TargetRecord,
    VulnerabilityRecord,
};
use crate::store::{
    ArtifactRecord, KnowledgeError, KnowledgeResult, KnowledgeStore, RunRecord, RunStatus,
    StepRecord,
};

impl From<neo4rs::Error> for KnowledgeError {
    fn from(error: neo4rs::Error) -> Self {
        KnowledgeError::Message(format!("neo4j: {error}"))
    }
}

/// Neo4j-backed fleet knowledge store.
pub struct Neo4jKnowledgeStore {
    graph: neo4rs::Graph,
}

impl Neo4jKnowledgeStore {
    /// Connect with a bolt URI (e.g. `bolt://127.0.0.1:7687`).
    pub fn connect(uri: &str, user: &str, password: &str) -> KnowledgeResult<Self> {
        let config = neo4rs::ConfigBuilder::new()
            .uri(uri)
            .user(user)
            .password(password)
            .build()
            .map_err(|e| KnowledgeError::Message(format!("neo4j config: {e}")))?;
        let graph = neo4rs::Graph::connect(config)
            .map_err(|e| KnowledgeError::Message(format!("neo4j connect: {e}")))?;
        Ok(Self { graph })
    }

    /// Apply idempotent constraints/indexes. Call once at startup.
    pub async fn ensure_schema(&self) -> KnowledgeResult<()> {
        for statement in schema_statements() {
            self.graph.run(neo4rs::query(&statement)).await?;
        }
        Ok(())
    }

    pub fn graph(&self) -> &neo4rs::Graph {
        &self.graph
    }
}

#[async_trait]
impl KnowledgeStore for Neo4jKnowledgeStore {
    async fn record_run(&self, run: RunRecord) -> KnowledgeResult<()> {
        let status = serde_json::to_string(&run.status)
            .map_err(|e| KnowledgeError::Message(e.to_string()))?;
        self.graph
            .run(
                neo4rs::query(
                    "MERGE (r:Run {id: $id}) \
                     SET r.prompt = $prompt, r.created_at = $created_at, r.status = $status",
                )
                .param("id", run.run_id.to_string())
                .param("prompt", run.prompt)
                .param("created_at", run.created_at.to_rfc3339())
                .param("status", status.trim_matches('"').to_owned()),
            )
            .await?;
        Ok(())
    }

    async fn set_run_status(&self, run_id: &RunId, status: RunStatus) -> KnowledgeResult<()> {
        let status =
            serde_json::to_string(&status).map_err(|e| KnowledgeError::Message(e.to_string()))?;
        self.graph
            .run(
                neo4rs::query("MATCH (r:Run {id: $id}) SET r.status = $status")
                    .param("id", run_id.to_string())
                    .param("status", status.trim_matches('"').to_owned()),
            )
            .await?;
        Ok(())
    }

    async fn record_step(&self, step: StepRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MATCH (r:Run {id: $run_id}) \
                     MERGE (a:Agent {id: $agent}) \
                     CREATE (s:Step {id: $step_id, summary: $summary, created_at: $created_at}) \
                     CREATE (r)-[:HAS_STEP]->(s) \
                     CREATE (a)-[:EXECUTED]->(s)",
                )
                .param("run_id", step.run_id.to_string())
                .param("step_id", step.step_id.to_string())
                .param("agent", step.agent)
                .param("summary", step.summary)
                .param("created_at", step.created_at.to_rfc3339()),
            )
            .await?;
        Ok(())
    }

    async fn record_artifact(&self, artifact: ArtifactRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MATCH (s:Step {id: $step_id}) \
                     CREATE (a:Artifact {id: randomUUID(), kind: $kind, content: $content, \
                             created_at: $created_at}) \
                     CREATE (s)-[:PRODUCED]->(a)",
                )
                .param("step_id", artifact.step_id.to_string())
                .param("kind", artifact.kind)
                .param("content", artifact.content)
                .param("created_at", artifact.created_at.to_rfc3339()),
            )
            .await?;
        Ok(())
    }

    async fn run(&self, run_id: &RunId) -> KnowledgeResult<Option<RunRecord>> {
        let mut rows = self
            .graph
            .execute(
                neo4rs::query(
                    "MATCH (r:Run {id: $id}) \
                     RETURN r.prompt AS prompt, r.created_at AS created_at, r.status AS status",
                )
                .param("id", run_id.to_string()),
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        let prompt: String = row
            .get("prompt")
            .map_err(|e| KnowledgeError::Message(e.to_string()))?;
        let created_at: String = row
            .get("created_at")
            .map_err(|e| KnowledgeError::Message(e.to_string()))?;
        let status: String = row
            .get("status")
            .map_err(|e| KnowledgeError::Message(e.to_string()))?;
        Ok(Some(RunRecord {
            run_id: *run_id,
            prompt,
            created_at: created_at
                .parse()
                .map_err(|e| KnowledgeError::Message(format!("created_at: {e}")))?,
            status: serde_json::from_value(serde_json::Value::String(status))
                .map_err(|e| KnowledgeError::Message(e.to_string()))?,
        }))
    }

    async fn steps(&self, run_id: &RunId) -> KnowledgeResult<Vec<StepRecord>> {
        let mut rows = self
            .graph
            .execute(
                neo4rs::query(
                    "MATCH (:Run {id: $id})-[:HAS_STEP]->(s:Step)<-[:EXECUTED]-(a:Agent) \
                     RETURN s.id AS step_id, a.id AS agent, s.summary AS summary, \
                            s.created_at AS created_at \
                     ORDER BY s.created_at",
                )
                .param("id", run_id.to_string()),
            )
            .await?;
        let mut steps = Vec::new();
        while let Some(row) = rows.next().await? {
            let step_id: String = row
                .get("step_id")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            let agent: String = row
                .get("agent")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            let summary: String = row
                .get("summary")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            let created_at: String = row
                .get("created_at")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            steps.push(StepRecord {
                run_id: *run_id,
                step_id: step_id
                    .parse::<uuid::Uuid>()
                    .map(StepId::from)
                    .map_err(|e| KnowledgeError::Message(format!("step_id: {e}")))?,
                agent,
                summary,
                created_at: created_at
                    .parse()
                    .map_err(|e| KnowledgeError::Message(format!("created_at: {e}")))?,
            });
        }
        Ok(steps)
    }

    async fn artifacts(&self, run_id: &RunId) -> KnowledgeResult<Vec<ArtifactRecord>> {
        let mut rows = self
            .graph
            .execute(
                neo4rs::query(
                    "MATCH (:Run {id: $id})-[:HAS_STEP]->(s:Step)-[:PRODUCED]->(a:Artifact) \
                     RETURN s.id AS step_id, a.kind AS kind, a.content AS content, \
                            a.created_at AS created_at \
                     ORDER BY a.created_at",
                )
                .param("id", run_id.to_string()),
            )
            .await?;
        let mut artifacts = Vec::new();
        while let Some(row) = rows.next().await? {
            let step_id: String = row
                .get("step_id")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            let kind: String = row
                .get("kind")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            let content: String = row
                .get("content")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            let created_at: String = row
                .get("created_at")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            artifacts.push(ArtifactRecord {
                run_id: *run_id,
                step_id: step_id
                    .parse::<uuid::Uuid>()
                    .map(StepId::from)
                    .map_err(|e| KnowledgeError::Message(format!("step_id: {e}")))?,
                kind,
                content,
                created_at: created_at
                    .parse()
                    .map_err(|e| KnowledgeError::Message(format!("created_at: {e}")))?,
            });
        }
        Ok(artifacts)
    }
}

#[async_trait]
impl SecurityKnowledge for Neo4jKnowledgeStore {
    async fn upsert_target(&self, target: TargetRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MERGE (t:Target {id: $id}) \
                     SET t.chain = $chain, t.path_or_address = $path, t.framework = $framework \
                     WITH t \
                     MERGE (c:Contract {address: $id}) \
                     SET c.chain = $chain, c.path = $path",
                )
                .param("id", target.id)
                .param("chain", target.chain)
                .param("path", target.path_or_address)
                .param("framework", target.framework.unwrap_or_default()),
            )
            .await?;
        Ok(())
    }

    async fn upsert_scanner(&self, scanner: ScannerRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query("MERGE (s:Scanner {name: $name}) SET s.kind = $kind")
                    .param("name", scanner.name)
                    .param("kind", scanner.kind),
            )
            .await?;
        Ok(())
    }

    async fn upsert_vulnerability(&self, vuln: VulnerabilityRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MERGE (v:Vulnerability {fingerprint: $fp}) \
                     SET v.pattern_id = $pattern_id, v.severity = $severity, \
                         v.confidence = $confidence, v.title = $title, \
                         v.description = $description, v.validation = $validation, \
                         v.ontology_class = $ontology_class, v.created_at = $created_at \
                     WITH v \
                     MERGE (t:Target {id: $target_id}) \
                     MERGE (t)-[:HAS_VULNERABILITY]->(v) \
                     MERGE (s:Scanner {name: $scanner}) \
                     MERGE (v)-[:DETECTED_BY]->(s)",
                )
                .param("fp", vuln.fingerprint)
                .param("pattern_id", vuln.pattern_id)
                .param("severity", vuln.severity)
                .param("confidence", vuln.confidence)
                .param("title", vuln.title)
                .param("description", vuln.description)
                .param("validation", vuln.validation)
                .param("ontology_class", vuln.ontology_class.unwrap_or_default())
                .param("created_at", vuln.created_at.to_rfc3339())
                .param("target_id", vuln.target_id)
                .param("scanner", vuln.scanner),
            )
            .await?;
        Ok(())
    }

    async fn add_evidence(&self, evidence: EvidenceRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MATCH (v:Vulnerability {fingerprint: $fp}) \
                     MERGE (e:Evidence {id: $id}) \
                     SET e.file = $file, e.start_line = $start_line, e.snippet = $snippet \
                     MERGE (v)-[:SUPPORTED_BY]->(e)",
                )
                .param("fp", evidence.vulnerability_fingerprint)
                .param("id", evidence.id)
                .param("file", evidence.file)
                .param(
                    "start_line",
                    evidence.start_line.map(|n| n as i64).unwrap_or(-1),
                )
                .param("snippet", evidence.snippet.unwrap_or_default()),
            )
            .await?;
        Ok(())
    }

    async fn link_mitigation(&self, mitigation: MitigationRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MATCH (v:Vulnerability {fingerprint: $fp}) \
                     MERGE (s:Skill {name: $skill}) \
                     SET s.detail = $detail \
                     MERGE (v)-[:MITIGATED_BY]->(s)",
                )
                .param("fp", mitigation.vulnerability_fingerprint)
                .param("skill", mitigation.skill_name)
                .param("detail", mitigation.detail),
            )
            .await?;
        Ok(())
    }

    async fn vulnerabilities_for_target(
        &self,
        target_id: &str,
    ) -> KnowledgeResult<Vec<VulnerabilityRecord>> {
        let mut rows = self
            .graph
            .execute(
                neo4rs::query(
                    "MATCH (:Target {id: $id})-[:HAS_VULNERABILITY]->(v:Vulnerability) \
                     OPTIONAL MATCH (v)-[:DETECTED_BY]->(s:Scanner) \
                     RETURN v.fingerprint AS fingerprint, v.pattern_id AS pattern_id, \
                            v.severity AS severity, v.confidence AS confidence, \
                            v.title AS title, v.description AS description, \
                            v.validation AS validation, v.ontology_class AS ontology_class, \
                            v.created_at AS created_at, coalesce(s.name, '') AS scanner",
                )
                .param("id", target_id.to_owned()),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            let created_at: String = row
                .get("created_at")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            out.push(VulnerabilityRecord {
                fingerprint: row
                    .get("fingerprint")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                pattern_id: row
                    .get("pattern_id")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                severity: row
                    .get("severity")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                confidence: row
                    .get("confidence")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                title: row
                    .get("title")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                description: row
                    .get("description")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                target_id: target_id.to_owned(),
                scanner: row
                    .get("scanner")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                ontology_class: {
                    let value: String = row
                        .get("ontology_class")
                        .map_err(|e| KnowledgeError::Message(e.to_string()))?;
                    if value.is_empty() {
                        None
                    } else {
                        Some(value)
                    }
                },
                validation: row
                    .get("validation")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                created_at: created_at
                    .parse()
                    .map_err(|e| KnowledgeError::Message(format!("created_at: {e}")))?,
            });
        }
        Ok(out)
    }

    async fn evidence_for(&self, fingerprint: &str) -> KnowledgeResult<Vec<EvidenceRecord>> {
        let mut rows = self
            .graph
            .execute(
                neo4rs::query(
                    "MATCH (:Vulnerability {fingerprint: $fp})-[:SUPPORTED_BY]->(e:Evidence) \
                     RETURN e.id AS id, e.file AS file, e.start_line AS start_line, \
                            e.snippet AS snippet",
                )
                .param("fp", fingerprint.to_owned()),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            let start_line: i64 = row.get("start_line").unwrap_or(-1);
            out.push(EvidenceRecord {
                id: row
                    .get("id")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                vulnerability_fingerprint: fingerprint.to_owned(),
                file: row
                    .get("file")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                start_line: if start_line < 0 {
                    None
                } else {
                    Some(start_line as u32)
                },
                snippet: {
                    let value: String = row
                        .get("snippet")
                        .map_err(|e| KnowledgeError::Message(e.to_string()))?;
                    if value.is_empty() {
                        None
                    } else {
                        Some(value)
                    }
                },
            });
        }
        Ok(out)
    }
}

#[async_trait]
impl SecurityKnowledge for Neo4jKnowledgeStore {
    async fn upsert_target(&self, target: TargetRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MERGE (t:Target {id: $id}) \
                     SET t.chain = $chain, t.path_or_address = $path, t.framework = $framework \
                     WITH t \
                     MERGE (c:Contract {address: $id}) \
                     SET c.chain = $chain, c.path = $path",
                )
                .param("id", target.id)
                .param("chain", target.chain)
                .param("path", target.path_or_address)
                .param("framework", target.framework.unwrap_or_default()),
            )
            .await?;
        Ok(())
    }

    async fn upsert_scanner(&self, scanner: ScannerRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MERGE (s:Scanner {name: $name}) SET s.kind = $kind",
                )
                .param("name", scanner.name)
                .param("kind", scanner.kind),
            )
            .await?;
        Ok(())
    }

    async fn upsert_vulnerability(&self, vuln: VulnerabilityRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MERGE (v:Vulnerability {fingerprint: $fp}) \
                     SET v.pattern_id = $pattern_id, v.severity = $severity, \
                         v.confidence = $confidence, v.title = $title, \
                         v.description = $description, v.validation = $validation, \
                         v.ontology_class = $ontology_class, v.created_at = $created_at \
                     WITH v \
                     MERGE (t:Target {id: $target_id}) \
                     MERGE (t)-[:HAS_VULNERABILITY]->(v) \
                     MERGE (s:Scanner {name: $scanner}) \
                     MERGE (v)-[:DETECTED_BY]->(s)",
                )
                .param("fp", vuln.fingerprint)
                .param("pattern_id", vuln.pattern_id)
                .param("severity", vuln.severity)
                .param("confidence", vuln.confidence)
                .param("title", vuln.title)
                .param("description", vuln.description)
                .param("validation", vuln.validation)
                .param(
                    "ontology_class",
                    vuln.ontology_class.unwrap_or_default(),
                )
                .param("created_at", vuln.created_at.to_rfc3339())
                .param("target_id", vuln.target_id)
                .param("scanner", vuln.scanner),
            )
            .await?;
        Ok(())
    }

    async fn add_evidence(&self, evidence: EvidenceRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MATCH (v:Vulnerability {fingerprint: $fp}) \
                     MERGE (e:Evidence {id: $id}) \
                     SET e.file = $file, e.start_line = $start_line, e.snippet = $snippet \
                     MERGE (v)-[:SUPPORTED_BY]->(e)",
                )
                .param("fp", evidence.vulnerability_fingerprint)
                .param("id", evidence.id)
                .param("file", evidence.file)
                .param("start_line", evidence.start_line.map(|n| n as i64).unwrap_or(-1))
                .param("snippet", evidence.snippet.unwrap_or_default()),
            )
            .await?;
        Ok(())
    }

    async fn link_mitigation(&self, mitigation: MitigationRecord) -> KnowledgeResult<()> {
        self.graph
            .run(
                neo4rs::query(
                    "MATCH (v:Vulnerability {fingerprint: $fp}) \
                     MERGE (s:Skill {name: $skill}) \
                     SET s.detail = $detail \
                     MERGE (v)-[:MITIGATED_BY]->(s)",
                )
                .param("fp", mitigation.vulnerability_fingerprint)
                .param("skill", mitigation.skill_name)
                .param("detail", mitigation.detail),
            )
            .await?;
        Ok(())
    }

    async fn vulnerabilities_for_target(
        &self,
        target_id: &str,
    ) -> KnowledgeResult<Vec<VulnerabilityRecord>> {
        let mut rows = self
            .graph
            .execute(
                neo4rs::query(
                    "MATCH (:Target {id: $id})-[:HAS_VULNERABILITY]->(v:Vulnerability) \
                     OPTIONAL MATCH (v)-[:DETECTED_BY]->(s:Scanner) \
                     RETURN v.fingerprint AS fingerprint, v.pattern_id AS pattern_id, \
                            v.severity AS severity, v.confidence AS confidence, \
                            v.title AS title, v.description AS description, \
                            v.validation AS validation, v.ontology_class AS ontology_class, \
                            v.created_at AS created_at, coalesce(s.name, '') AS scanner",
                )
                .param("id", target_id.to_owned()),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            let created_at: String = row
                .get("created_at")
                .map_err(|e| KnowledgeError::Message(e.to_string()))?;
            out.push(VulnerabilityRecord {
                fingerprint: row
                    .get("fingerprint")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                pattern_id: row
                    .get("pattern_id")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                severity: row
                    .get("severity")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                confidence: row
                    .get("confidence")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                title: row
                    .get("title")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                description: row
                    .get("description")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                target_id: target_id.to_owned(),
                scanner: row
                    .get("scanner")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                ontology_class: {
                    let value: String = row
                        .get("ontology_class")
                        .map_err(|e| KnowledgeError::Message(e.to_string()))?;
                    if value.is_empty() {
                        None
                    } else {
                        Some(value)
                    }
                },
                validation: row
                    .get("validation")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                created_at: created_at
                    .parse()
                    .map_err(|e| KnowledgeError::Message(format!("created_at: {e}")))?,
            });
        }
        Ok(out)
    }

    async fn evidence_for(&self, fingerprint: &str) -> KnowledgeResult<Vec<EvidenceRecord>> {
        let mut rows = self
            .graph
            .execute(
                neo4rs::query(
                    "MATCH (:Vulnerability {fingerprint: $fp})-[:SUPPORTED_BY]->(e:Evidence) \
                     RETURN e.id AS id, e.file AS file, e.start_line AS start_line, \
                            e.snippet AS snippet",
                )
                .param("fp", fingerprint.to_owned()),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            let start_line: i64 = row
                .get("start_line")
                .unwrap_or(-1);
            out.push(EvidenceRecord {
                id: row
                    .get("id")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                vulnerability_fingerprint: fingerprint.to_owned(),
                file: row
                    .get("file")
                    .map_err(|e| KnowledgeError::Message(e.to_string()))?,
                start_line: if start_line < 0 {
                    None
                } else {
                    Some(start_line as u32)
                },
                snippet: {
                    let value: String = row
                        .get("snippet")
                        .map_err(|e| KnowledgeError::Message(e.to_string()))?;
                    if value.is_empty() {
                        None
                    } else {
                        Some(value)
                    }
                },
            });
        }
        Ok(out)
    }
}
