//! Integration test against a real Neo4j service (feature `neo4j`).
//!
//! Skips cleanly unless `ALTIUS_NEO4J_URI` is set, so offline `cargo test`
//! stays green. CI provides a Neo4j service container (see
//! `.github/workflows/rust.yml`); locally, `docker compose up neo4j`.

#![cfg(feature = "neo4j")]

use altius_core::{RunId, StepId};
use altius_memory::{
    new_run_record, new_step_record, KnowledgeStore, Neo4jKnowledgeStore, RunStatus,
};

#[tokio::test]
async fn run_lifecycle_persists_in_neo4j() {
    let Ok(uri) = std::env::var("ALTIUS_NEO4J_URI") else {
        eprintln!("skipping: ALTIUS_NEO4J_URI not set");
        return;
    };
    let user = std::env::var("ALTIUS_NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
    let password = std::env::var("ALTIUS_NEO4J_PASSWORD").unwrap_or_else(|_| "altius-dev".into());

    let store = Neo4jKnowledgeStore::connect(&uri, &user, &password).expect("connect");
    store.ensure_schema().await.expect("schema");

    let run_id = RunId::new();
    let step_id = StepId::new();

    store
        .record_run(new_run_record(run_id, "integration: lint fixture project"))
        .await
        .expect("record run");
    store
        .record_step(new_step_record(
            run_id,
            step_id,
            "coder",
            "ran cargo clippy",
        ))
        .await
        .expect("record step");
    store
        .set_run_status(&run_id, RunStatus::Completed)
        .await
        .expect("set status");

    let run = store
        .run(&run_id)
        .await
        .expect("query run")
        .expect("run exists");
    assert_eq!(run.status, RunStatus::Completed);

    let steps = store.steps(&run_id).await.expect("query steps");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].agent, "coder");
}
