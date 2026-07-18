use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::description::AgentDescription;
use crate::error::{ProtocolError, Result};

/// Local agent registry: register descriptions and discover peers.
///
/// Registration stores *claims*, not verified identities — see the
/// fail-closed [`super::DidVerifier`] path for the distinction.
#[async_trait]
pub trait AgentRegistry: Send + Sync {
    /// Validate and store a description, keyed by DID. Re-registering the
    /// same DID replaces the previous description (agents update in place).
    async fn register(&self, description: AgentDescription) -> Result<()>;

    /// Look up one agent by DID string.
    async fn find(&self, did: &str) -> Result<AgentDescription>;

    /// Discover registered agents, optionally filtered by interface
    /// protocol (e.g. `"a2a"`).
    async fn discover(&self, protocol: Option<&str>) -> Result<Vec<AgentDescription>>;
}

/// In-memory [`AgentRegistry`] for local development and tests.
#[derive(Clone, Default)]
pub struct InMemoryRegistry {
    agents: Arc<RwLock<HashMap<String, AgentDescription>>>,
}

impl InMemoryRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentRegistry for InMemoryRegistry {
    async fn register(&self, description: AgentDescription) -> Result<()> {
        // Validation parses the DID, so only well-formed claims are stored.
        let did = description.validate()?;
        self.agents
            .write()
            .await
            .insert(did.as_str().to_owned(), description);
        Ok(())
    }

    async fn find(&self, did: &str) -> Result<AgentDescription> {
        self.agents
            .read()
            .await
            .get(did)
            .cloned()
            .ok_or_else(|| ProtocolError::not_found("agent", did))
    }

    async fn discover(&self, protocol: Option<&str>) -> Result<Vec<AgentDescription>> {
        let agents = self.agents.read().await;
        let mut found: Vec<AgentDescription> = agents
            .values()
            .filter(|description| match protocol {
                Some(protocol) => description
                    .interfaces
                    .iter()
                    .any(|interface| interface.protocol == protocol),
                None => true,
            })
            .cloned()
            .collect();
        found.sort_by(|a, b| a.did.cmp(&b.did));
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anp::InterfaceDescription;

    fn description(did: &str, protocol: &str) -> AgentDescription {
        AgentDescription {
            did: did.into(),
            name: "peer".into(),
            description: "a peer agent".into(),
            interfaces: vec![InterfaceDescription {
                protocol: protocol.into(),
                url: "https://peer.example.com/api".into(),
                description: None,
            }],
            version: None,
        }
    }

    #[tokio::test]
    async fn register_find_discover_round_trip() {
        let registry = InMemoryRegistry::new();
        registry
            .register(description("did:wba:one.example.com:agent:a", "a2a"))
            .await
            .unwrap();
        registry
            .register(description("did:wba:two.example.com:agent:b", "beeacp"))
            .await
            .unwrap();

        let found = registry
            .find("did:wba:one.example.com:agent:a")
            .await
            .unwrap();
        assert_eq!(found.interfaces[0].protocol, "a2a");

        let all = registry.discover(None).await.unwrap();
        assert_eq!(all.len(), 2);

        let a2a_only = registry.discover(Some("a2a")).await.unwrap();
        assert_eq!(a2a_only.len(), 1);
        assert_eq!(a2a_only[0].did, "did:wba:one.example.com:agent:a");
    }

    #[tokio::test]
    async fn register_rejects_invalid_claims() {
        let registry = InMemoryRegistry::new();
        let err = registry
            .register(description("did:web:nope.example.com", "a2a"))
            .await
            .unwrap_err();
        assert!(matches!(err, ProtocolError::Validation { .. }));
        assert!(registry.discover(None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn re_registration_replaces_description() {
        let registry = InMemoryRegistry::new();
        let did = "did:wba:one.example.com:agent:a";
        registry.register(description(did, "a2a")).await.unwrap();
        let mut updated = description(did, "a2a");
        updated.name = "renamed".into();
        registry.register(updated).await.unwrap();
        assert_eq!(registry.find(did).await.unwrap().name, "renamed");
        assert_eq!(registry.discover(None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unknown_did_is_not_found() {
        let registry = InMemoryRegistry::new();
        assert!(matches!(
            registry.find("did:wba:ghost.example.com:agent:x").await,
            Err(ProtocolError::NotFound { .. })
        ));
    }
}
