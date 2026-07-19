use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreCard {
    pub case_id: String,
    pub true_positives: u32,
    pub false_negatives: u32,
    pub false_positives: u32,
    /// One case-level true negative when a clean fixture produces no findings.
    pub true_negatives: u32,
    /// Arena-style Critical/High recall numerator (x).
    pub critical_high_recall_num: u32,
    /// Arena-style Critical/High recall denominator (y).
    pub critical_high_recall_den: u32,
    /// Scanner invocation latency for this case.
    pub latency_ms: u64,
    /// Whether the scanner completed and returned a report.
    pub tool_succeeded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalReport {
    pub suite: String,
    pub cards: Vec<ScoreCard>,
    pub precision: f64,
    pub recall: f64,
    pub critical_high_recall: String,
    pub false_positive_rate: f64,
    pub tool_success_rate: f64,
    pub latency_ms: u64,
    /// Available for optional live model evaluators; native offline scanning
    /// does not consume LLM tokens or provider cost.
    pub estimated_input_tokens: Option<u64>,
    pub estimated_output_tokens: Option<u64>,
    pub estimated_cost_usd: Option<f64>,
}

impl EvalReport {
    pub fn from_cards(suite: String, cards: Vec<ScoreCard>) -> Self {
        let tp: u32 = cards.iter().map(|c| c.true_positives).sum();
        let fn_count: u32 = cards.iter().map(|c| c.false_negatives).sum();
        let fp: u32 = cards.iter().map(|c| c.false_positives).sum();
        let tn: u32 = cards.iter().map(|c| c.true_negatives).sum();
        let ch_num: u32 = cards.iter().map(|c| c.critical_high_recall_num).sum();
        let ch_den: u32 = cards.iter().map(|c| c.critical_high_recall_den).sum();
        let precision = if tp + fp == 0 {
            1.0
        } else {
            tp as f64 / (tp + fp) as f64
        };
        let recall = if tp + fn_count == 0 {
            1.0
        } else {
            tp as f64 / (tp + fn_count) as f64
        };
        let fpr = if fp + tn == 0 {
            0.0
        } else {
            fp as f64 / (fp + tn) as f64
        };
        let successful_tools = cards.iter().filter(|card| card.tool_succeeded).count();
        let tool_success_rate = if cards.is_empty() {
            1.0
        } else {
            successful_tools as f64 / cards.len() as f64
        };
        let latency_ms = cards.iter().map(|card| card.latency_ms).sum();
        Self {
            suite,
            cards,
            precision,
            recall,
            critical_high_recall: format!("{ch_num}/{ch_den}"),
            false_positive_rate: fpr,
            tool_success_rate,
            latency_ms,
            estimated_input_tokens: None,
            estimated_output_tokens: None,
            estimated_cost_usd: None,
        }
    }

    pub fn to_markdown(&self) -> String {
        let mut out = format!(
            "# Eval: {}\n\nPrecision: {:.3}\nRecall: {:.3}\nCritical/High recall: {}\nFP rate: {:.3}\nTool success rate: {:.3}\nLatency: {} ms\nToken/cost estimate: unavailable (offline native scanners)\n\n",
            self.suite,
            self.precision,
            self.recall,
            self.critical_high_recall,
            self.false_positive_rate,
            self.tool_success_rate,
            self.latency_ms,
        );
        out.push_str("| Case | TP | FN | FP | TN | CH | Tool | Latency ms |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n");
        for card in &self.cards {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {}/{} | {} | {} |\n",
                card.case_id,
                card.true_positives,
                card.false_negatives,
                card.false_positives,
                card.true_negatives,
                card.critical_high_recall_num,
                card.critical_high_recall_den,
                if card.tool_succeeded { "ok" } else { "failed" },
                card.latency_ms,
            ));
        }
        out
    }
}
