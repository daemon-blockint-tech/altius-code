//! Slash skills: short prefixes that force a fleet route.
//!
//! Skills are Altius-owned UX sugar over `agent_name` / `@Mention` routing.
//! They are not a third-party plugin marketplace.

use crate::supervisor::FleetRoute;

/// A parsed leading slash skill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashSkill {
    /// Canonical skill name (`scan`, `browser`, `audit`, `pay`).
    pub name: &'static str,
    /// Route forced by this skill.
    pub route: FleetRoute,
    /// Prompt with the skill prefix stripped (may be empty).
    pub remainder: String,
}

/// Known web3-oriented slash skills.
pub fn known_skills() -> &'static [(&'static str, FleetRoute)] {
    &[
        ("scan", FleetRoute::Security),
        ("audit", FleetRoute::Security),
        ("browser", FleetRoute::Browser),
        ("github", FleetRoute::GitHub),
        // Payment specialist graph node is still stubbed; route through
        // the supervisor so policy/prompts still apply.
        ("pay", FleetRoute::Both),
    ]
}

/// Parse a leading `/skill` from `prompt`. Returns `None` when absent.
pub fn parse_slash_skill(prompt: &str) -> Option<SlashSkill> {
    let trimmed = prompt.trim_start();
    if !trimmed.starts_with('/') {
        return None;
    }
    let rest = &trimmed[1..];
    let (name_raw, remainder) = match rest.split_once(|c: char| c.is_whitespace()) {
        Some((name, rem)) => (name, rem.trim_start().to_owned()),
        None => (rest, String::new()),
    };
    if name_raw.is_empty() {
        return None;
    }
    let lower = name_raw.to_ascii_lowercase();
    for (name, route) in known_skills() {
        if *name == lower {
            return Some(SlashSkill {
                name,
                route: *route,
                remainder,
            });
        }
    }
    None
}

/// Agent name implied by a skill (wire format for BeeACP `agent_name`).
pub fn agent_name_for_route(route: FleetRoute) -> &'static str {
    match route {
        FleetRoute::Security => "security",
        FleetRoute::Browser => "browser",
        FleetRoute::GitHub => "github",
        FleetRoute::Explorer | FleetRoute::Coder | FleetRoute::Both => "altius",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scan_and_strips_prefix() {
        let skill = parse_slash_skill("/scan look for CPI bugs").unwrap();
        assert_eq!(skill.name, "scan");
        assert_eq!(skill.route, FleetRoute::Security);
        assert_eq!(skill.remainder, "look for CPI bugs");
    }

    #[test]
    fn parses_browser_case_insensitive() {
        let skill = parse_slash_skill("  /Browser open https://example.com").unwrap();
        assert_eq!(skill.name, "browser");
        assert_eq!(skill.route, FleetRoute::Browser);
    }

    #[test]
    fn parses_github_skill() {
        let skill = parse_slash_skill("/github inspect pull requests").unwrap();
        assert_eq!(skill.name, "github");
        assert_eq!(skill.route, FleetRoute::GitHub);
        assert_eq!(agent_name_for_route(skill.route), "github");
    }

    #[test]
    fn unknown_skill_is_none() {
        assert!(parse_slash_skill("/deploy everything").is_none());
        assert!(parse_slash_skill("no slash").is_none());
    }

    #[test]
    fn skill_only_prompt_has_empty_remainder() {
        let skill = parse_slash_skill("/audit").unwrap();
        assert!(skill.remainder.is_empty());
        assert_eq!(skill.route, FleetRoute::Security);
    }
}
