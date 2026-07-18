use altius_core::AgentId;

use crate::prompts;

/// Known fleet agent roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentRole {
    Router,
    Explorer,
    Coder,
    Critic,
    /// Web automation via an attached browser MCP server (`@Browser`).
    Browser,
    /// Read-only vulnerability scanning (`@Security`).
    Security,
    /// Stub — must call TxGuard when implemented (Phase C adjacent).
    Deployer,
    /// x402 settlement via `altius-payments` + TxGuard; graph node pending.
    Payment,
    /// Fleet knowledge graph (`altius-memory`) + ontology (`altius-ontology`);
    /// graph node pending.
    Knowledge,
}

impl AgentRole {
    pub fn id(self) -> AgentId {
        AgentId::new(self.as_str())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Router => "router",
            Self::Explorer => "explorer",
            Self::Coder => "coder",
            Self::Critic => "critic",
            Self::Browser => "browser",
            Self::Security => "security",
            Self::Deployer => "deployer",
            Self::Payment => "payment",
            Self::Knowledge => "knowledge",
        }
    }

    pub fn system_prompt(self) -> &'static str {
        match self {
            Self::Router => prompts::ROUTER_SYSTEM,
            Self::Explorer => prompts::EXPLORER_SYSTEM,
            Self::Coder => prompts::CODER_SYSTEM,
            Self::Critic => prompts::CRITIC_SYSTEM,
            Self::Browser => prompts::BROWSER_SYSTEM,
            Self::Security => prompts::SECURITY_SYSTEM,
            Self::Deployer => prompts::DEPLOYER_STUB_SYSTEM,
            Self::Payment => prompts::PAYMENT_STUB_SYSTEM,
            Self::Knowledge => prompts::KNOWLEDGE_STUB_SYSTEM,
        }
    }

    /// Roles with a real node implementation in the supervisor graph.
    pub fn phase_a_active(self) -> bool {
        matches!(
            self,
            Self::Router
                | Self::Explorer
                | Self::Coder
                | Self::Critic
                | Self::Browser
                | Self::Security
        )
    }
}

/// Metadata for stub roles that are named but not yet wired into the graph.
#[derive(Clone, Debug)]
pub struct StubRole {
    pub role: AgentRole,
    pub note: &'static str,
}

pub fn stub_roles() -> Vec<StubRole> {
    vec![
        StubRole {
            role: AgentRole::Deployer,
            note: "Phase C-adjacent: TxRequest only through TxGuard",
        },
        StubRole {
            role: AgentRole::Payment,
            note: "altius-payments landed (x402 via TxGuard); graph node wiring pending",
        },
        StubRole {
            role: AgentRole::Knowledge,
            note: "altius-memory + altius-ontology landed; graph node wiring pending",
        },
    ]
}
