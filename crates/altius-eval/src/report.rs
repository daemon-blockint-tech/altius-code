use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreCard {
    pub case_id: String,
    pub true_positives: u32,
    pub false_negatives: u32,
    pub false_positives: u32,
    /// Arena-style Critical/High recall numerator (x).
    pub critical_high_recall_num: u32,
    /// Arena-style Critical/High recall denominator (y).
    pub critical_high_recall_den: u32,
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
}

impl EvalReport {
    pub fn from_cards(suite: String, cards: Vec<ScoreCard>) -> Self {
        let tp: u32 = cards.iter().map(|c| c.true_positives).sum();
        let fn_count: u32 = cards.iter().map(|c| c.false_negatives).sum();
        let fp: u32 = cards.iter().map(|c| c.false_positives).sum();
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
        let fpr = if tp + fp == 0 {
            0.0
        } else {
            fp as f64 / (tp + fp) as f64
        };
        Self {
            suite,
            cards,
            precision,
            recall,
            critical_high_recall: format!("{ch_num}/{ch_den}"),
            false_positive_rate: fpr,
        }
    }

    pub fn to_markdown(&self) -> String {
        let mut out = format!(
            "# Eval: {}\n\nPrecision: {:.3}\nRecall: {:.3}\nCritical/High recall: {}\nFP rate: {:.3}\n\n",
            self.suite, self.precision, self.recall, self.critical_high_recall, self.false_positive_rate
        );
        out.push_str("| Case | TP | FN | FP | CH |\n| --- | --- | --- | --- | --- |\n");
        for card in &self.cards {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {}/{} |\n",
                card.case_id,
                card.true_positives,
                card.false_negatives,
                card.false_positives,
                card.critical_high_recall_num,
                card.critical_high_recall_den
            ));
        }
        out
    }
}
