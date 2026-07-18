//! Built-in SVM/security domain schema.
//!
//! A deliberately small ontology subset the knowledge agent can reason with
//! offline. Class names align with the Neo4j labels in `altius-memory` where
//! they overlap (`Contract`, `Vulnerability`, `Skill`).

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

/// The built-in SVM/security ontology subset.
pub fn svm_security_schema() -> DomainSchema {
    DomainSchema {
        name: "altius-svm-security".into(),
        classes: vec![
            class("Artifact", "Anything the fleet produces or analyzes", None),
            class("Contract", "An on-chain SVM program", Some("Artifact")),
            class("Instruction", "A callable entrypoint of a contract", None),
            class("Account", "An on-chain account a contract touches", None),
            class("Vulnerability", "A security weakness in a contract", None),
            class(
                "MissingSignerCheck",
                "Instruction handler that skips signer validation",
                Some("Vulnerability"),
            ),
            class(
                "MissingOwnerCheck",
                "Instruction handler that skips account-owner validation",
                Some("Vulnerability"),
            ),
            class(
                "ArbitraryCpi",
                "Instruction that CPIs into an attacker-supplied program",
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
                "Contract",
                "Vulnerability",
                "A contract carries a security finding",
            ),
            property(
                "foundIn",
                "Vulnerability",
                "Instruction",
                "A finding is localized to an instruction",
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
    fn builtin_schema_is_consistent() {
        svm_security_schema().validate().unwrap();
    }

    #[test]
    fn validation_catches_dangling_references() {
        let mut schema = svm_security_schema();
        schema
            .properties
            .push(property("bad", "Nope", "Contract", ""));
        assert!(schema.validate().is_err());
    }

    #[test]
    fn vulnerability_taxonomy_is_rooted() {
        let schema = svm_security_schema();
        let missing_signer = schema.class("MissingSignerCheck").unwrap();
        assert_eq!(missing_signer.subclass_of.as_deref(), Some("Vulnerability"));
    }
}
