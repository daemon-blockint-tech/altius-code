//! Altius evaluation moat: provenance-controlled gold labels and Arena-style scoring.
//!
//! Public Trident/Wake Arena results are methodology references only — fixtures
//! and labels here are Altius-owned.

mod gold;
mod report;
mod score;

pub use gold::{GoldCase, GoldLabel, GoldSuite};
pub use report::{EvalReport, ScoreCard};
pub use score::{score_suite, EvalError};
