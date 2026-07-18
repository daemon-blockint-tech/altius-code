use std::sync::Arc;
use std::time::Instant;

use altius_core::{Budget, RunId, StepId};
use futures::future::join_all;
use tracing::{debug, info};

use crate::checkpointer::Checkpointer;
use crate::error::{GraphError, GraphResult};
use crate::graph::{Graph, Outgoing};
use crate::node::NodeResult;
use crate::state::State;

/// Terminal / pause outcome of a graph run.
#[derive(Clone, Debug)]
pub enum ExecutionOutcome<S> {
    /// Graph reached [`NodeResult::Finish`] or ran out of edges.
    Finished { state: S, steps: u64 },
    /// Graph hit a HITL interrupt; resume with [`GraphExecutor::resume`].
    Interrupted {
        reason: String,
        node: String,
        state: S,
        steps: u64,
    },
}

/// Runs a compiled [`Graph`] with checkpointing and optional budgets.
pub struct GraphExecutor<S: State> {
    graph: Arc<Graph<S>>,
    checkpointer: Arc<dyn Checkpointer<S>>,
    budget: Budget,
}

impl<S: State> GraphExecutor<S> {
    pub fn new(
        graph: Arc<Graph<S>>,
        checkpointer: Arc<dyn Checkpointer<S>>,
        budget: Budget,
    ) -> Self {
        Self {
            graph,
            checkpointer,
            budget,
        }
    }

    pub fn graph(&self) -> &Graph<S> {
        &self.graph
    }

    /// Execute from the graph entry node.
    pub async fn run(&self, run_id: RunId, initial: S) -> GraphResult<ExecutionOutcome<S>> {
        self.run_from(run_id, self.graph.entry().to_owned(), initial, 0)
            .await
    }

    /// Resume after [`ExecutionOutcome::Interrupted`].
    ///
    /// `next_node` is typically the interrupted node (re-run) or an explicit
    /// successor chosen by the human approver.
    pub async fn resume(
        &self,
        run_id: RunId,
        next_node: impl Into<String>,
        state: S,
        steps_so_far: u64,
    ) -> GraphResult<ExecutionOutcome<S>> {
        let next = next_node.into();
        if self.graph.node(&next).is_none() {
            return Err(GraphError::resume(format!("unknown resume node `{next}`")));
        }
        self.run_from(run_id, next, state, steps_so_far).await
    }

    /// Resume from the latest checkpoint for `run_id`, re-entering that node.
    pub async fn resume_from_checkpoint(&self, run_id: RunId) -> GraphResult<ExecutionOutcome<S>> {
        let checkpoint = self
            .checkpointer
            .latest(&run_id)
            .await?
            .ok_or_else(|| GraphError::resume(format!("no checkpoint for run `{run_id}`")))?;
        self.resume(run_id, checkpoint.node, checkpoint.state, 0)
            .await
    }

    async fn run_from(
        &self,
        run_id: RunId,
        start_node: String,
        initial: S,
        steps_so_far: u64,
    ) -> GraphResult<ExecutionOutcome<S>> {
        let started = Instant::now();
        let mut current = start_node;
        let mut state = initial;
        let mut steps = steps_so_far;

        loop {
            self.check_budget(steps, started)?;

            let node = self
                .graph
                .node(&current)
                .ok_or_else(|| GraphError::UnknownNode(current.clone()))?
                .clone();

            debug!(run_id = %run_id, node = %current, step = steps, "running node");
            let result = node
                .run(state)
                .await
                .map_err(|e| GraphError::node_failed(current.clone(), e.to_string()))?;

            steps += 1;
            let step_id = StepId::new();

            match result {
                NodeResult::Finish(s) => {
                    self.checkpoint(&run_id, &step_id, &current, &s).await?;
                    info!(run_id = %run_id, steps, "graph finished");
                    return Ok(ExecutionOutcome::Finished { state: s, steps });
                }
                NodeResult::Interrupt { reason, state: s } => {
                    self.checkpoint(&run_id, &step_id, &current, &s).await?;
                    info!(run_id = %run_id, node = %current, %reason, "graph interrupted");
                    return Ok(ExecutionOutcome::Interrupted {
                        reason,
                        node: current,
                        state: s,
                        steps,
                    });
                }
                NodeResult::Goto { next, state: s } => {
                    self.checkpoint(&run_id, &step_id, &current, &s).await?;
                    if self.graph.node(&next).is_none() {
                        return Err(GraphError::UnknownNode(next));
                    }
                    state = s;
                    current = next;
                }
                NodeResult::Continue(s) => {
                    self.checkpoint(&run_id, &step_id, &current, &s).await?;
                    match self
                        .next_after_continue(&run_id, &current, s, &mut steps, started)
                        .await?
                    {
                        Next::Finished(final_state) => {
                            return Ok(ExecutionOutcome::Finished {
                                state: final_state,
                                steps,
                            });
                        }
                        Next::Interrupted {
                            reason,
                            node,
                            state: s,
                        } => {
                            return Ok(ExecutionOutcome::Interrupted {
                                reason,
                                node,
                                state: s,
                                steps,
                            });
                        }
                        Next::Continue { node, state: s } => {
                            current = node;
                            state = s;
                        }
                    }
                }
            }
        }
    }

    async fn next_after_continue(
        &self,
        run_id: &RunId,
        from: &str,
        state: S,
        steps: &mut u64,
        started: Instant,
    ) -> GraphResult<Next<S>> {
        let Some(outgoing) = self.graph.outgoing.get(from) else {
            return Ok(Next::Finished(state));
        };

        match outgoing {
            Outgoing::Direct(next) => Ok(Next::Continue {
                node: next.clone(),
                state,
            }),
            Outgoing::Conditional(router) => match router(&state) {
                Some(next) => {
                    if self.graph.node(&next).is_none() {
                        return Err(GraphError::UnknownNode(next));
                    }
                    Ok(Next::Continue { node: next, state })
                }
                None => Ok(Next::Finished(state)),
            },
            Outgoing::FanOut {
                targets,
                join,
                reducer,
            } => {
                self.check_budget(*steps, started)?;
                let limit = self.budget.parallel_limit();
                if targets.len() > limit {
                    return Err(GraphError::BudgetExceeded(format!(
                        "fan-out width {} exceeds max_parallel {}",
                        targets.len(),
                        limit
                    )));
                }

                let mut futures = Vec::with_capacity(targets.len());
                for target in targets {
                    let node = self
                        .graph
                        .node(target)
                        .ok_or_else(|| GraphError::UnknownNode(target.clone()))?
                        .clone();
                    let branch_state = state.clone();
                    let name = target.clone();
                    futures.push(async move {
                        let result = node
                            .run(branch_state)
                            .await
                            .map_err(|e| GraphError::node_failed(name.clone(), e.to_string()))?;
                        Ok::<_, GraphError>((name, result))
                    });
                }

                let results = join_all(futures).await;
                let mut branch_states = Vec::with_capacity(results.len());
                for item in results {
                    let (name, result) = item?;
                    *steps += 1;
                    let step_id = StepId::new();
                    match result {
                        NodeResult::Finish(s) | NodeResult::Continue(s) => {
                            self.checkpoint(run_id, &step_id, &name, &s).await?;
                            branch_states.push(s);
                        }
                        NodeResult::Goto { next: _, state: s } => {
                            // Fan-out branches ignore nested Goto; state still joins.
                            self.checkpoint(run_id, &step_id, &name, &s).await?;
                            branch_states.push(s);
                        }
                        NodeResult::Interrupt { reason, state: s } => {
                            self.checkpoint(run_id, &step_id, &name, &s).await?;
                            return Ok(Next::Interrupted {
                                reason,
                                node: name,
                                state: s,
                            });
                        }
                    }
                }

                let joined = reducer(branch_states);
                let step_id = StepId::new();
                self.checkpoint(run_id, &step_id, join, &joined).await?;
                Ok(Next::Continue {
                    node: join.clone(),
                    state: joined,
                })
            }
        }
    }

    async fn checkpoint(
        &self,
        run_id: &RunId,
        step_id: &StepId,
        node: &str,
        state: &S,
    ) -> GraphResult<()> {
        self.checkpointer.put(run_id, step_id, node, state).await
    }

    fn check_budget(&self, steps: u64, started: Instant) -> GraphResult<()> {
        if self.budget.steps_exceeded(steps) {
            return Err(GraphError::BudgetExceeded(format!(
                "max_steps {:?} exceeded (at {steps})",
                self.budget.max_steps
            )));
        }
        if let Some(max) = self.budget.max_wall_time {
            if started.elapsed() > max {
                return Err(GraphError::BudgetExceeded(format!(
                    "max_wall_time {max:?} exceeded"
                )));
            }
        }
        Ok(())
    }
}

enum Next<S> {
    Continue {
        node: String,
        state: S,
    },
    Finished(S),
    Interrupted {
        reason: String,
        node: String,
        state: S,
    },
}
