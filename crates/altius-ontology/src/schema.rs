//! Built-in chain-neutral + chain-specific security domain schema.
//!
//! Class names align with Neo4j labels in `altius-memory` where they overlap
//! (`Contract`/`Target`, `Vulnerability`, `Skill`, `Evidence`, `Scanner`).

use serde::{Deserialize, Serialize};

/// A class (concept) in the domain schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClassDef {
    pub name: String,
    pub description: String,
    /// Parent class name, if any (single inheritance is enough here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subclass_of: Option<String>,
}

/// A property (relation) between two classes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertyDef {
    pub name: String,
    pub domain: String,
    pub range: String,
    pub description: String,
}

/// A named set of classes and properties.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainSchema {
    pub name: String,
    pub classes: Vec<ClassDef>,
    pub properties: Vec<PropertyDef>,
}

impl DomainSchema {
    pub fn class(&self, name: &str) -> Option<&ClassDef> {
        self.classes.iter().find(|c| c.name == name)
    }

    /// Every property must reference declared classes, and every
    /// `subclass_of` must resolve.
    pub fn validate(&self) -> Result<(), String> {
        for class in &self.classes {
            if let Some(parent) = &class.subclass_of {
                if self.class(parent).is_none() {
                    return Err(format!(
                        "class {} declares unknown parent {parent}",
                        class.name
                    ));
                }
            }
        }
        for property in &self.properties {
            for (side, name) in [("domain", &property.domain), ("range", &property.range)] {
                if self.class(name).is_none() {
                    return Err(format!(
                        "property {} has unknown {side} class {name}",
                        property.name
                    ));
                }
            }
        }
        Ok(())
    }
}

fn class(name: &str, description: &str, subclass_of: Option<&str>) -> ClassDef {
    ClassDef {
        name: name.into(),
        description: description.into(),
        subclass_of: subclass_of.map(Into::into),
    }
}

fn property(name: &str, domain: &str, range: &str, description: &str) -> PropertyDef {
    PropertyDef {
        name: name.into(),
        domain: domain.into(),
        range: range.into(),
        description: description.into(),
    }
}

/// Map a native scanner rule/pattern id onto an ontology class name.
pub fn ontology_class_for_pattern(pattern_id: &str) -> Option<&'static str> {
    match pattern_id {
        "svm-missing-signer-check" => Some("MissingSignerCheck"),
        "svm-missing-owner-check" => Some("MissingOwnerCheck"),
        "svm-arbitrary-cpi" => Some("ArbitraryCpi"),
        "svm-unvalidated-writable-account" => Some("UnvalidatedWritableAccount"),
        "svm-lamports-overflow-risk" => Some("LamportsOverflowRisk"),
        "svm-close-without-zeroing" => Some("CloseWithoutZeroing"),
        "svm-pda-bump-canonicalization" => Some("PdaBumpCanonicalization"),
        "svm-sysvar-address-validation" => Some("SysvarAddressValidation"),
        "svm-account-confusion" => Some("AccountConfusion"),
        "svm-unchecked-arithmetic" => Some("UncheckedArithmetic"),
        "svm-remaining-accounts-risk" => Some("RemainingAccountsRisk"),
        "svm-oracle-trust-risk" => Some("OracleTrustRisk"),
        "evm-reentrancy" => Some("EvmReentrancy"),
        "evm-access-control" => Some("EvmAccessControl"),
        "evm-unchecked-call" => Some("EvmUncheckedCall"),
        "algorand-rekey-risk" => Some("AlgorandRekeyRisk"),
        "cairo-felt-overflow" => Some("CairoFeltOverflow"),
        "cosmos-nondeterminism" => Some("CosmosNondeterminism"),
        "ton-sender-check" => Some("TonSenderCheck"),
        _ => None,
    }
}

/// The built-in multi-chain security ontology.
pub fn svm_security_schema() -> DomainSchema {
    // Kept name for backward compatibility; schema is now chain-neutral +.
    security_schema()
}

/// Chain-neutral vulnerability roots plus chain-specific subclasses.
pub fn security_schema() -> DomainSchema {
    DomainSchema {
        name: "altius-security".into(),
        classes: vec![
            class("Artifact", "Anything the fleet produces or analyzes", None),
            class(
                "Target",
                "A contract/program/module under analysis",
                Some("Artifact"),
            ),
            class(
                "Contract",
                "An on-chain program (alias of Target)",
                Some("Target"),
            ),
            class("Instruction", "A callable entrypoint of a contract", None),
            class("Account", "An on-chain account a contract touches", None),
            class(
                "Evidence",
                "Source span or dynamic trace supporting a finding",
                None,
            ),
            class(
                "Scanner",
                "A native or adapter scanner that emits findings",
                None,
            ),
            class("Vulnerability", "A security weakness in a target", None),
            class(
                "MissingSignerCheck",
                "Handler that skips signer validation",
                Some("Vulnerability"),
            ),
            class(
                "MissingOwnerCheck",
                "Handler that skips account-owner validation",
                Some("Vulnerability"),
            ),
            class(
                "ArbitraryCpi",
                "CPI into an attacker-supplied program",
                Some("Vulnerability"),
            ),
            class(
                "UnvalidatedWritableAccount",
                "Mutates account without writable checks",
                Some("Vulnerability"),
            ),
            class(
                "LamportsOverflowRisk",
                "Unchecked lamport arithmetic",
                Some("Vulnerability"),
            ),
            class(
                "CloseWithoutZeroing",
                "Account close without zeroing data (revival)",
                Some("Vulnerability"),
            ),
            class(
                "PdaBumpCanonicalization",
                "Non-canonical PDA bump handling",
                Some("Vulnerability"),
            ),
            class(
                "SysvarAddressValidation",
                "Sysvar/ix introspection without address checks",
                Some("Vulnerability"),
            ),
            class(
                "AccountConfusion",
                "Account identity swap / confusion risk",
                Some("Vulnerability"),
            ),
            class(
                "UncheckedArithmetic",
                "Financial mul/div without checked math",
                Some("Vulnerability"),
            ),
            class(
                "RemainingAccountsRisk",
                "Unvalidated remaining_accounts usage",
                Some("Vulnerability"),
            ),
            class(
                "OracleTrustRisk",
                "Oracle/price feed without freshness checks",
                Some("Vulnerability"),
            ),
            class(
                "EvmReentrancy",
                "EVM reentrancy / checks-effects-interactions risk",
                Some("Vulnerability"),
            ),
            class(
                "EvmAccessControl",
                "Missing or weak EVM access control",
                Some("Vulnerability"),
            ),
            class(
                "EvmUncheckedCall",
                "Unchecked low-level call/delegatecall",
                Some("Vulnerability"),
            ),
            class(
                "AlgorandRekeyRisk",
                "Algorand rekey/close-to risk",
                Some("Vulnerability"),
            ),
            class(
                "CairoFeltOverflow",
                "Cairo felt arithmetic overflow/wrap risk",
                Some("Vulnerability"),
            ),
            class(
                "CosmosNondeterminism",
                "Cosmos/CosmWasm nondeterminism or panic divergence",
                Some("Vulnerability"),
            ),
            class(
                "TonSenderCheck",
                "TON sender / notification authentication risk",
                Some("Vulnerability"),
            ),
            class("Skill", "A reusable procedure the fleet learned", None),
        ],
        properties: vec![
            property(
                "hasInstruction",
                "Contract",
                "Instruction",
                "A contract exposes an instruction",
            ),
            property(
                "touchesAccount",
                "Instruction",
                "Account",
                "An instruction reads or writes an account",
            ),
            property(
                "hasVulnerability",
                "Target",
                "Vulnerability",
                "A target carries a security finding",
            ),
            property(
                "foundIn",
                "Vulnerability",
                "Instruction",
                "A finding is localized to an instruction",
            ),
            property(
                "supportedBy",
                "Vulnerability",
                "Evidence",
                "Evidence that supports a finding",
            ),
            property(
                "detectedBy",
                "Vulnerability",
                "Scanner",
                "Scanner that emitted the finding",
            ),
            property(
                "mitigatedBy",
                "Vulnerability",
                "Skill",
                "A procedure that fixes a class of findings",
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_validates() {
        security_schema().validate().unwrap();
        svm_security_schema().validate().unwrap();
    }

    #[test]
    fn pattern_mapping_covers_core_svm_rules() {
        assert_eq!(
            ontology_class_for_pattern("svm-missing-signer-check"),
            Some("MissingSignerCheck")
        );
        assert_eq!(
            ontology_class_for_pattern("svm-oracle-trust-risk"),
            Some("OracleTrustRisk")
        );
        assert_eq!(ontology_class_for_pattern("unknown"), None);
    }
}
