//! Integration test for [`Neo4jMemoryStore`] against a real Neo4j service
//! (feature `neo4j`).
//!
//! Skips cleanly unless `ALTIUS_NEO4J_URI` is set, so offline `cargo test`
//! stays green. CI provides a Neo4j service container (see
//! `.github/workflows/rust.yml`); locally, `docker compose up neo4j`.

#![cfg(feature = "neo4j")]

use altius_core::{RunId, StepId};
use altius_graph::{MemoryStore, Neo4jMemoryStore};

#[tokio::test]
async fn checkpoint_and_kv_roundtrip_in_neo4j() {
    let Ok(uri) = std::env::var("ALTIUS_NEO4J_URI") else {
        eprintln!("skipping: ALTIUS_NEO4J_URI not set");
        return;
    };
    let user = std::env::var("ALTIUS_NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
    let password = std::env::var("ALTIUS_NEO4J_PASSWORD").unwrap_or_else(|_| "altius-dev".into());

    let store = Neo4jMemoryStore::connect(&uri, &user, &password).expect("connect");
    store.ensure_schema().await.expect("schema");

    let run_id = RunId::new();
    let first = StepId::new();
    let second = StepId::new();

    store
        .put_checkpoint(&run_id, &first, "router", b"{\"step\":1}")
        .await
        .expect("put first checkpoint");
    store
        .put_checkpoint(&run_id, &second, "coder", b"{\"step\":2}")
        .await
        .expect("put second checkpoint");

    let latest = store
        .latest_checkpoint(&run_id)
        .await
        .expect("query latest")
        .expect("checkpoint exists");
    assert_eq!(latest.node, "coder");
    assert_eq!(latest.step_id, second);
    assert_eq!(latest.payload, b"{\"step\":2}");

    let namespace = format!("scratch-{run_id}");
    store
        .put_kv(&namespace, "k", b"\x00\x01\x02binary")
        .await
        .expect("put kv");
    store
        .put_kv(&namespace, "k", b"overwritten")
        .await
        .expect("overwrite kv");
    let value = store.get_kv(&namespace, "k").await.expect("get kv");
    assert_eq!(value.as_deref(), Some(b"overwritten".as_slice()));

    assert!(store
        .get_kv(&namespace, "missing")
        .await
        .expect("get missing")
        .is_none());
}
