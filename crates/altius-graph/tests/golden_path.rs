//! Golden-path integration test for altius-graph (no network / no Neo4j).

use std::sync::Arc;

use altius_core::{Budget, RunId};
use altius_graph::{
    Checkpointer, ExecutionOutcome, GraphBuilder, GraphExecutor, InMemoryCheckpointer,
    InMemoryStore, MemoryStore, MemoryStoreCheckpointer, Node, NodeResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct DemoState {
    value: i32,
    path: Vec<String>,
    approved: bool,
}

struct NamedFn {
    name: &'static str,
    f: Box<dyn Fn(DemoState) -> NodeResult<DemoState> + Send + Sync>,
}

impl NamedFn {
    fn new(
        name: &'static str,
        f: impl Fn(DemoState) -> NodeResult<DemoState> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            f: Box::new(f),
        }
    }
}

#[async_trait]
impl Node<DemoState> for NamedFn {
    fn name(&self) -> &str {
        self.name
    }

    async fn run(&self, mut state: DemoState) -> altius_graph::GraphResult<NodeResult<DemoState>> {
        state.path.push(self.name.to_owned());
        Ok((self.f)(state))
    }
}

#[tokio::test]
async fn golden_path_router_workers_join_finish() {
    let graph = GraphBuilder::new()
        .add_node(NamedFn::new("router", |mut s| {
            s.value += 1;
            NodeResult::Continue(s)
        }))
        .add_node(NamedFn::new("explorer", |mut s| {
            s.value += 10;
            NodeResult::Continue(s)
        }))
        .add_node(NamedFn::new("coder", |mut s| {
            s.value += 100;
            NodeResult::Continue(s)
        }))
        .add_node(NamedFn::new("critic", |mut s| {
            s.value += 1000;
            NodeResult::Continue(s)
        }))
        .add_node(NamedFn::new("finalize", NodeResult::Finish))
        .set_entry("router")
        .add_fanout_join(
            "router",
            ["explorer", "coder"],
            "critic",
            |branches: Vec<DemoState>| {
                let mut out = DemoState::default();
                for b in branches {
                    out.value += b.value;
                    out.path.extend(b.path);
                }
                out
            },
        )
        .add_edge("critic", "finalize")
        .build()
        .expect("graph builds");

    let checkpointer = Arc::new(InMemoryCheckpointer::<DemoState>::new());
    let executor = GraphExecutor::new(Arc::new(graph), checkpointer.clone(), Budget::unlimited());
    let run_id = RunId::new();

    let outcome = executor
        .run(run_id, DemoState::default())
        .await
        .expect("run ok");

    match outcome {
        ExecutionOutcome::Finished { state, steps } => {
            // router(+1) then explorer(+10) and coder(+100) each see value=1,
            // reducer sums branch values (11 + 101 = 112), critic(+1000) => 1112
            assert_eq!(state.value, 1112);
            assert!(state.path.contains(&"router".to_string()));
            assert!(state.path.contains(&"explorer".to_string()));
            assert!(state.path.contains(&"coder".to_string()));
            assert!(state.path.contains(&"critic".to_string()));
            assert!(state.path.contains(&"finalize".to_string()));
            assert!(steps >= 4);
        }
        other => panic!("expected Finished, got {other:?}"),
    }

    let latest = checkpointer.latest(&run_id).await.unwrap().unwrap();
    assert_eq!(latest.node, "finalize");
}

#[tokio::test]
async fn hitl_interrupt_and_resume() {
    let graph = GraphBuilder::new()
        .add_node(NamedFn::new("prepare", NodeResult::Continue))
        .add_node(NamedFn::new("approve", |s| {
            if s.approved {
                NodeResult::Continue(s)
            } else {
                NodeResult::Interrupt {
                    reason: "needs human approval".into(),
                    state: s,
                }
            }
        }))
        .add_node(NamedFn::new("done", |mut s| {
            s.value = 42;
            NodeResult::Finish(s)
        }))
        .set_entry("prepare")
        .add_edge("prepare", "approve")
        .add_edge("approve", "done")
        .build()
        .unwrap();

    let checkpointer = Arc::new(InMemoryCheckpointer::<DemoState>::new());
    let executor = GraphExecutor::new(Arc::new(graph), checkpointer, Budget::unlimited());
    let run_id = RunId::new();

    let interrupted = executor.run(run_id, DemoState::default()).await.unwrap();

    let ExecutionOutcome::Interrupted {
        reason,
        node,
        mut state,
        steps,
    } = interrupted
    else {
        panic!("expected interrupt");
    };
    assert_eq!(node, "approve");
    assert!(reason.contains("approval"));

    state.approved = true;
    let finished = executor
        .resume(run_id, "approve", state, steps)
        .await
        .unwrap();

    match finished {
        ExecutionOutcome::Finished { state, .. } => assert_eq!(state.value, 42),
        other => panic!("expected Finished after resume, got {other:?}"),
    }
}

#[tokio::test]
async fn memory_store_checkpointer_roundtrip() {
    let store = InMemoryStore::new();
    store.put_kv("ns", "k", b"v").await.unwrap();
    assert_eq!(
        store.get_kv("ns", "k").await.unwrap().as_deref(),
        Some(b"v".as_slice())
    );

    let checkpointer = MemoryStoreCheckpointer::<DemoState, _>::new(store);
    let run = RunId::new();
    let step = altius_core::StepId::new();
    let state = DemoState {
        value: 7,
        path: vec!["a".into()],
        approved: false,
    };
    Checkpointer::put(&checkpointer, &run, &step, "n", &state)
        .await
        .unwrap();
    let latest = checkpointer.latest(&run).await.unwrap().unwrap();
    assert_eq!(latest.state, state);
    assert_eq!(latest.node, "n");
}

#[tokio::test]
async fn conditional_edge_routes() {
    let graph = GraphBuilder::new()
        .add_node(NamedFn::new("start", NodeResult::Continue))
        .add_node(NamedFn::new("left", |mut s| {
            s.value = 1;
            NodeResult::Finish(s)
        }))
        .add_node(NamedFn::new("right", |mut s| {
            s.value = 2;
            NodeResult::Finish(s)
        }))
        .set_entry("start")
        .add_conditional_edge("start", |s: &DemoState| {
            if s.approved {
                Some("left".into())
            } else {
                Some("right".into())
            }
        })
        .build()
        .unwrap();

    let executor = GraphExecutor::new(
        Arc::new(graph),
        Arc::new(InMemoryCheckpointer::new()),
        Budget::unlimited(),
    );

    let outcome = executor
        .run(
            RunId::new(),
            DemoState {
                approved: true,
                ..DemoState::default()
            },
        )
        .await
        .unwrap();
    match outcome {
        ExecutionOutcome::Finished { state, .. } => assert_eq!(state.value, 1),
        other => panic!("{other:?}"),
    }
}
