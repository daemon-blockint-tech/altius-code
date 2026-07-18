//! Adapter trait for ontology backends.
//!
//! [`StaticOntologyClient`] serves the built-in schema and is what the
//! knowledge agent uses today. An MCP-backed client speaking to an external
//! OWL/RDF ontology server (open-ontologies style) is an intentional stub:
//! it lands once `altius-mcp` exposes client-side attach for external
//! servers, and its remote responses will be treated as untrusted input.

use async_trait::async_trait;
use thiserror::Error;

use crate::schema::{ClassDef, DomainSchema, PropertyDef};

#[derive(Debug, Error)]
pub enum OntologyError {
    #[error("unknown class {0}")]
    UnknownClass(String),

    #[error("not implemented: {0}")]
    NotImplemented(String),

    #[error("{0}")]
    Message(String),
}

pub type OntologyResult<T> = Result<T, OntologyError>;

/// Read-only ontology queries the knowledge agent relies on.
#[async_trait]
pub trait OntologyClient: Send + Sync {
    /// Every class in the active schema.
    async fn list_classes(&self) -> OntologyResult<Vec<ClassDef>>;

    /// One class by name.
    async fn describe_class(&self, name: &str) -> OntologyResult<ClassDef>;

    /// Properties whose domain or range involves `class_name`.
    async fn properties_of(&self, class_name: &str) -> OntologyResult<Vec<PropertyDef>>;

    /// Transitive subclasses of `class_name` (e.g. all vulnerability kinds).
    async fn subclasses_of(&self, class_name: &str) -> OntologyResult<Vec<ClassDef>>;
}

/// [`OntologyClient`] backed by an in-process [`DomainSchema`].
pub struct StaticOntologyClient {
    schema: DomainSchema,
}

impl StaticOntologyClient {
    /// Wrap a schema, validating it first.
    pub fn new(schema: DomainSchema) -> OntologyResult<Self> {
        schema.validate().map_err(OntologyError::Message)?;
        Ok(Self { schema })
    }

    /// The built-in SVM/security schema.
    pub fn builtin() -> Self {
        Self::new(crate::schema::svm_security_schema()).expect("built-in schema validates")
    }

    pub fn schema(&self) -> &DomainSchema {
        &self.schema
    }
}

#[async_trait]
impl OntologyClient for StaticOntologyClient {
    async fn list_classes(&self) -> OntologyResult<Vec<ClassDef>> {
        Ok(self.schema.classes.clone())
    }

    async fn describe_class(&self, name: &str) -> OntologyResult<ClassDef> {
        self.schema
            .class(name)
            .cloned()
            .ok_or_else(|| OntologyError::UnknownClass(name.to_owned()))
    }

    async fn properties_of(&self, class_name: &str) -> OntologyResult<Vec<PropertyDef>> {
        if self.schema.class(class_name).is_none() {
            return Err(OntologyError::UnknownClass(class_name.to_owned()));
        }
        Ok(self
            .schema
            .properties
            .iter()
            .filter(|p| p.domain == class_name || p.range == class_name)
            .cloned()
            .collect())
    }

    async fn subclasses_of(&self, class_name: &str) -> OntologyResult<Vec<ClassDef>> {
        if self.schema.class(class_name).is_none() {
            return Err(OntologyError::UnknownClass(class_name.to_owned()));
        }
        let mut result = Vec::new();
        let mut frontier = vec![class_name.to_owned()];
        while let Some(current) = frontier.pop() {
            for class in &self.schema.classes {
                if class.subclass_of.as_deref() == Some(current.as_str()) {
                    frontier.push(class.name.clone());
                    result.push(class.clone());
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn builtin_client_answers_class_queries() {
        let client = StaticOntologyClient::builtin();
        let classes = client.list_classes().await.unwrap();
        assert!(classes.iter().any(|c| c.name == "Contract"));

        let contract = client.describe_class("Contract").await.unwrap();
        assert_eq!(contract.subclass_of.as_deref(), Some("Artifact"));

        let props = client.properties_of("Vulnerability").await.unwrap();
        assert!(props.iter().any(|p| p.name == "hasVulnerability"));
    }

    #[tokio::test]
    async fn subclass_queries_are_transitive() {
        let client = StaticOntologyClient::builtin();
        let vulns = client.subclasses_of("Vulnerability").await.unwrap();
        let names: Vec<_> = vulns.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"MissingSignerCheck"));
        assert!(names.contains(&"ArbitraryCpi"));
    }

    #[tokio::test]
    async fn unknown_class_is_an_error() {
        let client = StaticOntologyClient::builtin();
        assert!(matches!(
            client.describe_class("Nope").await,
            Err(OntologyError::UnknownClass(_))
        ));
    }
}
