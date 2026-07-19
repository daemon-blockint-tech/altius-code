//! Deterministic, offline-capable intent and risk classification.

use serde::{Deserialize, Serialize};

use crate::supervisor::{resolve_forced_route, FleetRoute};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskIntent {
    Explore,
    Edit,
    Build,
    Browse,
    Security,
    GitHub,
    #[default]
    General,
}

/// Auditable route decision produced before any model call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDecision {
    pub route: FleetRoute,
    pub intent: TaskIntent,
    pub risk: RiskLevel,
    pub reason: String,
    pub signals: Vec<String>,
    pub forced: bool,
}

pub fn classify_route(agent_name: Option<&str>, prompt: &str) -> RouteDecision {
    if let Some(route) = resolve_forced_route(agent_name, prompt) {
        return RouteDecision {
            route,
            intent: intent_for_route(route),
            risk: risk_for_prompt(prompt),
            reason: "explicit agent, @mention, or slash-skill override".into(),
            signals: vec!["forced_route".into()],
            forced: true,
        };
    }

    let lower = prompt.to_ascii_lowercase();
    let mut signals = Vec::new();
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    let security = has(&[
        "audit",
        "scan",
        "vulnerab",
        "exploit",
        "security",
        "threat model",
        "arbitrary cpi",
        "signer check",
        "reentr",
        "sanctions",
        "malware",
    ]);
    let on_chain_side_effect = has(&[
        "sign transaction",
        "broadcast",
        "mainnet",
        "deploy program",
        "transfer funds",
        "send sol",
        "private key",
        "seed phrase",
        "wallet drain",
    ]);
    let browser = (lower.contains("http://") || lower.contains("https://"))
        && has(&[
            "open",
            "navigate",
            "browse",
            "website",
            "page",
            "click",
            "fill",
            "screenshot",
            "download",
        ]);
    let github = has(&[
        "pull request",
        "github issue",
        "github.com",
        "workflow run",
        "repository checks",
    ]);
    let build = has(&[
        "build",
        "compile",
        "cargo test",
        "run tests",
        "clippy",
        "fmt",
        "lint",
    ]);
    let edit = has(&[
        "implement",
        "edit",
        "change",
        "fix",
        "refactor",
        "add ",
        "remove ",
        "rename",
        "write ",
    ]);
    let explore = has(&[
        "find",
        "search",
        "inspect",
        "explain",
        "summarize",
        "where is",
        "review",
    ]);

    if security {
        signals.push("security_domain".into());
    }
    if on_chain_side_effect {
        signals.push("on_chain_side_effect".into());
    }
    if browser {
        signals.push("url_browser_action".into());
    }
    if github {
        signals.push("github_resource".into());
    }
    if build {
        signals.push("build_or_test".into());
    }
    if edit {
        signals.push("filesystem_edit".into());
    }
    if explore {
        signals.push("read_only_investigation".into());
    }

    let risk = risk_for_prompt(prompt);
    let (route, intent, reason) = if security || on_chain_side_effect {
        (
            FleetRoute::Security,
            TaskIntent::Security,
            if on_chain_side_effect {
                "security review selected for risky on-chain side effects"
            } else {
                "security intent requires read-only security specialist"
            },
        )
    } else if browser {
        (
            FleetRoute::Browser,
            TaskIntent::Browse,
            "URL plus explicit browser interaction requested",
        )
    } else if github {
        (
            FleetRoute::GitHub,
            TaskIntent::GitHub,
            "GitHub resource or operation requested",
        )
    } else if edit || build {
        (
            FleetRoute::Coder,
            if build {
                TaskIntent::Build
            } else {
                TaskIntent::Edit
            },
            "edit/build side effects require coder tools",
        )
    } else if explore {
        (
            FleetRoute::Explorer,
            TaskIntent::Explore,
            "read-only investigation intent",
        )
    } else {
        signals.push("ambiguous_general_task".into());
        (
            FleetRoute::Explorer,
            TaskIntent::General,
            "ambiguous task defaults to least-privilege explorer",
        )
    };

    RouteDecision {
        route,
        intent,
        risk,
        reason: reason.into(),
        signals,
        forced: false,
    }
}

fn intent_for_route(route: FleetRoute) -> TaskIntent {
    match route {
        FleetRoute::Explorer | FleetRoute::Both => TaskIntent::Explore,
        FleetRoute::Coder => TaskIntent::Edit,
        FleetRoute::Browser => TaskIntent::Browse,
        FleetRoute::GitHub => TaskIntent::GitHub,
        FleetRoute::Security => TaskIntent::Security,
    }
}

fn risk_for_prompt(prompt: &str) -> RiskLevel {
    let lower = prompt.to_ascii_lowercase();
    if ["private key", "seed phrase", "wallet drain", "exfiltrate"]
        .iter()
        .any(|s| lower.contains(s))
    {
        RiskLevel::Critical
    } else if [
        "sign transaction",
        "broadcast",
        "mainnet",
        "deploy program",
        "transfer funds",
        "send sol",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        RiskLevel::High
    } else if ["write", "edit", "implement", "build", "click", "fill"]
        .iter()
        .any(|s| lower.contains(s))
    {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_routes_win() {
        let decision = classify_route(Some("browser"), "audit this program");
        assert_eq!(decision.route, FleetRoute::Browser);
        assert!(decision.forced);
    }

    #[test]
    fn classifies_structured_signals() {
        let security = classify_route(None, "deploy program to mainnet");
        assert_eq!(security.route, FleetRoute::Security);
        assert_eq!(security.risk, RiskLevel::High);
        assert!(security.signals.contains(&"on_chain_side_effect".into()));

        let browser = classify_route(None, "open https://example.com and click login");
        assert_eq!(browser.route, FleetRoute::Browser);

        let coder = classify_route(None, "implement the parser and run cargo test");
        assert_eq!(coder.route, FleetRoute::Coder);
        assert_eq!(coder.intent, TaskIntent::Build);
    }

    #[test]
    fn ambiguous_defaults_to_least_privilege() {
        let decision = classify_route(None, "hello fleet");
        assert_eq!(decision.route, FleetRoute::Explorer);
        assert_eq!(decision.risk, RiskLevel::Low);
    }
}
