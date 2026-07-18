use std::fmt;

/// The specialist roles in the fleet's supervisor pipeline, in the
/// order they run. Mirrors the topology in the fleet plan: explorer →
/// coder → security → release, each with the narrowest tool subset
/// that lets it do its job (see [`crate::tools_for_role`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Understands the project: framework, programs, toolchain.
    Explorer,
    /// Builds and unit-tests the program.
    Coder,
    /// Reviews the security lint findings.
    Security,
    /// Previews the deployment plan (never deploys).
    Release,
}

impl Role {
    pub const PIPELINE: [Role; 4] = [Role::Explorer, Role::Coder, Role::Security, Role::Release];

    pub fn name(self) -> &'static str {
        match self {
            Role::Explorer => "explorer",
            Role::Coder => "coder",
            Role::Security => "security",
            Role::Release => "release",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The system prompt for a specialist. Shared rules first, then the
/// role's specific charge. Prompts state the security boundary
/// explicitly so a confused model at least has it in context — but the
/// boundary is enforced by the tool plane, not by these words.
pub fn system_prompt(role: Role) -> String {
    let shared = "You are one specialist in the Altius Code fleet working on a Solana \
        (SVM) project. Use your tools to gather facts; never fabricate tool output. \
        You have no ability to sign or broadcast transactions, and you must not \
        suggest workarounds to do so — real deployment happens via `altius deploy` \
        with human approval. When you are done, reply with a concise report of what \
        you found; do not call further tools once you have what you need.";
    let specific = match role {
        Role::Explorer => {
            "Your job: identify the project's framework, its programs, \
            the default cluster, and whether the required toolchain is installed. \
            Flag anything missing that would block a build."
        }
        Role::Coder => {
            "Your job: build the program and run the unit tests. Report \
            build artifacts and test results, and diagnose failures precisely from \
            tool output."
        }
        Role::Security => {
            "Your job: run the security lints and triage the findings. \
            Distinguish real risks (missing signer/owner checks, arbitrary CPI) from \
            noise, and say clearly whether the program looks safe to deploy."
        }
        Role::Release => {
            "Your job: preview the deployment plan and explain each step \
            (buffer creation, write chunks, finalize) so a human can approve or \
            reject the real deployment with full understanding."
        }
    };
    format!("{shared}\n\n{specific}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_prompt_states_the_signing_boundary() {
        for role in Role::PIPELINE {
            let prompt = system_prompt(role);
            assert!(
                prompt.contains("no ability to sign"),
                "{role} prompt lost the boundary"
            );
        }
    }
}
