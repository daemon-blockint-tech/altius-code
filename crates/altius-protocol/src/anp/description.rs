use serde::{Deserialize, Serialize};

use super::did::DidWba;
use crate::error::Result;
use crate::limits;

/// One interface an agent exposes (e.g. its BeeAI ACP or A2A endpoint).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceDescription {
    /// Protocol spoken at the endpoint (e.g. `beeacp`, `a2a`, `mcp`).
    pub protocol: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Simplified ANP agent description: who an agent claims to be and where
/// it can be reached.
///
/// This is a *claim* published by a remote peer. Nothing here is verified;
/// the DID is only checked syntactically and cryptographic proof goes
/// through [`super::DidVerifier`] (which currently fails closed).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDescription {
    /// The agent's `did:wba` identifier.
    pub did: String,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<InterfaceDescription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl AgentDescription {
    /// Bounded validation of an untrusted description; returns the parsed
    /// DID so callers can key on it.
    pub fn validate(&self) -> Result<DidWba> {
        let did = DidWba::parse(&self.did)?;
        limits::bounded_string("name", &self.name, limits::MAX_NAME_LEN)?;
        limits::bounded_string(
            "description",
            &self.description,
            limits::MAX_DESCRIPTION_LEN,
        )?;
        limits::bounded_opt_string("version", self.version.as_deref(), limits::MAX_NAME_LEN)?;
        limits::bounded_list("interfaces", self.interfaces.len(), limits::MAX_LIST_LEN)?;
        for interface in &self.interfaces {
            limits::bounded_string(
                "interface.protocol",
                &interface.protocol,
                limits::MAX_NAME_LEN,
            )?;
            limits::bounded_url("interface.url", &interface.url)?;
            limits::bounded_opt_string(
                "interface.description",
                interface.description.as_deref(),
                limits::MAX_DESCRIPTION_LEN,
            )?;
        }
        Ok(did)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn description() -> AgentDescription {
        AgentDescription {
            did: "did:wba:agents.example.com:agent:altius".into(),
            name: "altius".into(),
            description: "Altius SVM fleet agent".into(),
            interfaces: vec![InterfaceDescription {
                protocol: "beeacp".into(),
                url: "https://agents.example.com/runs".into(),
                description: Some("BeeAI ACP run API".into()),
            }],
            version: Some("0.1.0".into()),
        }
    }

    #[test]
    fn valid_description_round_trips() {
        let desc = description();
        let did = desc.validate().unwrap();
        assert_eq!(did.host(), "agents.example.com");
        let value = serde_json::to_value(&desc).unwrap();
        assert_eq!(value["interfaces"][0]["protocol"], "beeacp");
        let back: AgentDescription = serde_json::from_value(value).unwrap();
        assert_eq!(back, desc);
    }

    #[test]
    fn rejects_bad_did_and_bad_interface_url() {
        let mut bad = description();
        bad.did = "did:key:z6Mk".into();
        assert!(bad.validate().is_err());

        let mut bad = description();
        bad.interfaces[0].url = "javascript:alert(1)".into();
        assert!(bad.validate().is_err());
    }
}
