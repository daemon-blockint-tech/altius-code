use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{GraphError, GraphResult};
use crate::node::Node;
use crate::state::State;

/// Routes a state snapshot to the next node name after a conditional edge.
pub type EdgeRouter<S> = Arc<dyn Fn(&S) -> Option<String> + Send + Sync>;

/// Merges fan-out branch states into a single join state.
pub type JoinReducer<S> = Arc<dyn Fn(Vec<S>) -> S + Send + Sync>;

#[derive(Clone)]
pub(crate) enum Outgoing<S> {
    /// Follow a single static edge.
    Direct(String),
    /// Choose next node dynamically.
    Conditional(EdgeRouter<S>),
    /// Run several nodes in parallel, then reduce into `join`.
    FanOut {
        targets: Vec<String>,
        join: String,
        reducer: JoinReducer<S>,
    },
}

/// Immutable compiled agent graph.
pub struct Graph<S: State> {
    pub(crate) nodes: HashMap<String, Arc<dyn Node<S>>>,
    pub(crate) outgoing: HashMap<String, Outgoing<S>>,
    pub(crate) entry: String,
}

impl<S: State> Graph<S> {
    pub fn entry(&self) -> &str {
        &self.entry
    }

    pub fn node(&self, name: &str) -> Option<&Arc<dyn Node<S>>> {
        self.nodes.get(name)
    }

    pub fn node_names(&self) -> impl Iterator<Item = &str> {
        self.nodes.keys().map(|s| s.as_str())
    }
}

/// Fluent builder for [`Graph`].
pub struct GraphBuilder<S: State> {
    nodes: HashMap<String, Arc<dyn Node<S>>>,
    outgoing: HashMap<String, Outgoing<S>>,
    entry: Option<String>,
}

impl<S: State> Default for GraphBuilder<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: State> GraphBuilder<S> {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            outgoing: HashMap::new(),
            entry: None,
        }
    }

    pub fn add_node(mut self, node: impl Node<S> + 'static) -> Self {
        let name = node.name().to_owned();
        self.nodes.insert(name, Arc::new(node));
        self
    }

    pub fn add_node_arc(mut self, node: Arc<dyn Node<S>>) -> Self {
        let name = node.name().to_owned();
        self.nodes.insert(name, node);
        self
    }

    pub fn set_entry(mut self, name: impl Into<String>) -> Self {
        self.entry = Some(name.into());
        self
    }

    /// Static directed edge `from -> to`.
    pub fn add_edge(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.outgoing
            .insert(from.into(), Outgoing::Direct(to.into()));
        self
    }

    /// Conditional edge: `router` returns the next node name, or `None` to finish.
    pub fn add_conditional_edge<F>(mut self, from: impl Into<String>, router: F) -> Self
    where
        F: Fn(&S) -> Option<String> + Send + Sync + 'static,
    {
        self.outgoing
            .insert(from.into(), Outgoing::Conditional(Arc::new(router)));
        self
    }

    /// Fan-out from `from` into `targets`, then join at `join` with `reducer`.
    pub fn add_fanout_join<F, I, T>(
        mut self,
        from: impl Into<String>,
        targets: I,
        join: impl Into<String>,
        reducer: F,
    ) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
        F: Fn(Vec<S>) -> S + Send + Sync + 'static,
    {
        let targets: Vec<String> = targets.into_iter().map(Into::into).collect();
        self.outgoing.insert(
            from.into(),
            Outgoing::FanOut {
                targets,
                join: join.into(),
                reducer: Arc::new(reducer),
            },
        );
        self
    }

    pub fn build(self) -> GraphResult<Graph<S>> {
        let entry = self
            .entry
            .ok_or_else(|| GraphError::build("entry node not set"))?;
        if !self.nodes.contains_key(&entry) {
            return Err(GraphError::build(format!(
                "entry node `{entry}` is not registered"
            )));
        }
        for (from, out) in &self.outgoing {
            if !self.nodes.contains_key(from) {
                return Err(GraphError::build(format!(
                    "edge source `{from}` is not a registered node"
                )));
            }
            match out {
                Outgoing::Direct(to) => {
                    if !self.nodes.contains_key(to) {
                        return Err(GraphError::build(format!(
                            "edge target `{to}` is not a registered node"
                        )));
                    }
                }
                Outgoing::Conditional(_) => {}
                Outgoing::FanOut { targets, join, .. } => {
                    for t in targets {
                        if !self.nodes.contains_key(t) {
                            return Err(GraphError::build(format!(
                                "fan-out target `{t}` is not a registered node"
                            )));
                        }
                    }
                    if !self.nodes.contains_key(join) {
                        return Err(GraphError::build(format!(
                            "join node `{join}` is not a registered node"
                        )));
                    }
                }
            }
        }
        Ok(Graph {
            nodes: self.nodes,
            outgoing: self.outgoing,
            entry,
        })
    }
}
