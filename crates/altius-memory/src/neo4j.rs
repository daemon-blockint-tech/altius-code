//! Neo4j-backed [`KnowledgeStore`] (feature `neo4j`).
//!
//! Persists the schema in [`crate::schema`] via `neo4rs`. This module is
//! feature-gated and network-dependent: unit tests and offline CI use
//! [`crate::InMemoryKnowledgeStore`]; integration against a real Neo4j
//! service runs only when `ALTIUS_NEO4J_URI` is set (see docker-compose.yml).

use altius_core::{RunId, StepId};
use async_trait::async_trait;

use crate::schema::schema_statements;
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
