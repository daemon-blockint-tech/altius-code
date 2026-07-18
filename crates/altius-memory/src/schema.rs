//! Neo4j knowledge-graph schema for the fleet (Phase D).
//!
//! Node labels and relationship types are defined once here so Cypher in the
//! Neo4j store, docs, and tests never drift apart.

/// Node labels in the fleet knowledge graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeLabel {
    /// A fleet agent role (router, coder, payment, …).
    Agent,
    /// One user task execution.
    Run,
    /// One graph step inside a run.
    Step,
    /// Something a step produced (patch, report, plan, tx signature).
    Artifact,
    /// An on-chain program the fleet analyzed or deployed.
    Contract,
    /// A security finding attached to a contract.
    Vulnerability,
    /// A reusable procedure the fleet learned (procedural memory).
    Skill,
    /// A serialized graph-state checkpoint.
    Checkpoint,
}

impl NodeLabel {
    pub const ALL: [NodeLabel; 8] = [
        NodeLabel::Agent,
        NodeLabel::Run,
        NodeLabel::Step,
        NodeLabel::Artifact,
        NodeLabel::Contract,
        NodeLabel::Vulnerability,
        NodeLabel::Skill,
        NodeLabel::Checkpoint,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "Agent",
            Self::Run => "Run",
            Self::Step => "Step",
            Self::Artifact => "Artifact",
            Self::Contract => "Contract",
            Self::Vulnerability => "Vulnerability",
            Self::Skill => "Skill",
            Self::Checkpoint => "Checkpoint",
        }
    }
}

/// Relationship types in the fleet knowledge graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelType {
    /// `(:Agent)-[:EXECUTED]->(:Step)`
    Executed,
    /// `(:Run)-[:HAS_STEP]->(:Step)`
    HasStep,
    /// `(:Step)-[:PRODUCED]->(:Artifact)`
    Produced,
    /// `(:Step)-[:CALLED]->(:Contract)` — read-only interaction.
    Called,
    /// `(:Run)-[:DEPLOYED]->(:Contract)` — went through TxGuard.
    Deployed,
    /// `(:Run)-[:PAID]->(:Artifact)` — x402 settlement, through TxGuard.
    Paid,
    /// `(:Contract)-[:HAS_VULNERABILITY]->(:Vulnerability)`
    HasVulnerability,
    /// `(:Agent)-[:HAS_SKILL]->(:Skill)`
    HasSkill,
    /// `(:Run)-[:HAS_CHECKPOINT]->(:Checkpoint)`
    HasCheckpoint,
}

impl RelType {
    pub const ALL: [RelType; 9] = [
        RelType::Executed,
        RelType::HasStep,
        RelType::Produced,
        RelType::Called,
        RelType::Deployed,
        RelType::Paid,
        RelType::HasVulnerability,
        RelType::HasSkill,
        RelType::HasCheckpoint,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Executed => "EXECUTED",
            Self::HasStep => "HAS_STEP",
            Self::Produced => "PRODUCED",
            Self::Called => "CALLED",
            Self::Deployed => "DEPLOYED",
            Self::Paid => "PAID",
            Self::HasVulnerability => "HAS_VULNERABILITY",
            Self::HasSkill => "HAS_SKILL",
            Self::HasCheckpoint => "HAS_CHECKPOINT",
        }
    }
}

/// Idempotent Cypher statements creating uniqueness constraints and indexes.
/// Run once at startup by the Neo4j store (`CREATE ... IF NOT EXISTS`).
pub fn schema_statements() -> Vec<String> {
    let mut statements = vec![
        "CREATE CONSTRAINT agent_id IF NOT EXISTS FOR (a:Agent) REQUIRE a.id IS UNIQUE".into(),
        "CREATE CONSTRAINT run_id IF NOT EXISTS FOR (r:Run) REQUIRE r.id IS UNIQUE".into(),
        "CREATE CONSTRAINT step_id IF NOT EXISTS FOR (s:Step) REQUIRE s.id IS UNIQUE".into(),
        "CREATE CONSTRAINT artifact_id IF NOT EXISTS FOR (a:Artifact) REQUIRE a.id IS UNIQUE"
            .into(),
        "CREATE CONSTRAINT contract_address IF NOT EXISTS FOR (c:Contract) REQUIRE c.address IS UNIQUE"
            .into(),
        "CREATE CONSTRAINT skill_name IF NOT EXISTS FOR (s:Skill) REQUIRE s.name IS UNIQUE".into(),
    ];
    statements
        .push("CREATE INDEX run_created_at IF NOT EXISTS FOR (r:Run) ON (r.created_at)".into());
    statements.push(
        "CREATE INDEX vulnerability_severity IF NOT EXISTS FOR (v:Vulnerability) ON (v.severity)"
            .into(),
    );
    statements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_rels_are_stable() {
        assert_eq!(NodeLabel::Run.as_str(), "Run");
        assert_eq!(RelType::Paid.as_str(), "PAID");
        assert_eq!(NodeLabel::ALL.len(), 8);
        assert_eq!(RelType::ALL.len(), 9);
    }

    #[test]
    fn schema_statements_cover_core_labels() {
        let joined = schema_statements().join("\n");
        for label in ["Agent", "Run", "Step", "Artifact", "Contract", "Skill"] {
            assert!(joined.contains(label), "missing constraint for {label}");
        }
        assert!(joined.contains("IF NOT EXISTS"));
    }
}
