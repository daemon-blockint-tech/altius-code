//! `did:wba` identifiers: syntactic validation now, verification later.

use async_trait::async_trait;

use crate::error::{ProtocolError, Result};
use crate::limits;

/// A syntactically valid `did:wba` identifier.
///
/// Format (per the ANP `did:wba` method spec):
/// `did:wba:<host>[%3A<port>][:<path-segment>]*`, e.g.
/// `did:wba:agents.example.com:agent:altius` or
/// `did:wba:localhost%3A8800:user:alice`.
///
/// Parsing here is **syntax only** — it proves nothing about who controls
/// the identifier. Ownership requires the [`DidVerifier`] path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DidWba {
    did: String,
    host: String,
    port: Option<u16>,
    path_segments: Vec<String>,
}

impl DidWba {
    /// Parse and validate an untrusted DID string.
    pub fn parse(did: &str) -> Result<Self> {
        limits::bounded_string("did", did, limits::MAX_DID_LEN)?;
        let rest = did
            .strip_prefix("did:wba:")
            .ok_or_else(|| ProtocolError::validation("did", "must start with `did:wba:`"))?;

        let mut segments = rest.split(':');
        let authority = segments
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ProtocolError::validation("did", "missing host"))?;

        // `%3A` percent-encodes the colon separating host and port.
        let (host, port) = match authority.split_once("%3A") {
            Some((host, port)) => {
                let port: u16 = port.parse().map_err(|_| {
                    ProtocolError::validation("did", format!("invalid port `{port}`"))
                })?;
                (host, Some(port))
            }
            None => (authority, None),
        };
        if host.is_empty() || !host.chars().all(is_host_char) {
            return Err(ProtocolError::validation(
                "did",
                format!("invalid host `{host}`"),
            ));
        }

        let path_segments: Vec<String> = segments.map(str::to_owned).collect();
        for segment in &path_segments {
            if segment.is_empty() || !segment.chars().all(is_path_char) {
                return Err(ProtocolError::validation(
                    "did",
                    format!("invalid path segment `{segment}`"),
                ));
            }
        }

        Ok(Self {
            did: did.to_owned(),
            host: host.to_owned(),
            port,
            path_segments,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.did
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn path_segments(&self) -> &[String] {
        &self.path_segments
    }
}

fn is_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '-'
}

fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'
}

/// A DID whose control has been cryptographically proven.
///
/// Only a [`DidVerifier`] may mint this type; there is deliberately no
/// public constructor, so "verified" cannot be forged elsewhere in the
/// codebase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedIdentity {
    did: DidWba,
}

impl VerifiedIdentity {
    pub fn did(&self) -> &DidWba {
        &self.did
    }
}

/// The `did:wba` verification path: resolve the DID document over HTTPS
/// and check a signature over `challenge` against its verification methods.
#[async_trait]
pub trait DidVerifier: Send + Sync {
    async fn verify(
        &self,
        did: &DidWba,
        challenge: &[u8],
        signature: &[u8],
    ) -> Result<VerifiedIdentity>;
}

/// Phase-B stub: **always refuses**. Fail closed until real DID-document
/// resolution and signature verification are implemented — a remote peer
/// must never be treated as verified by default.
#[derive(Clone, Copy, Debug, Default)]
pub struct StubDidVerifier;

#[async_trait]
impl DidVerifier for StubDidVerifier {
    async fn verify(
        &self,
        did: &DidWba,
        _challenge: &[u8],
        _signature: &[u8],
    ) -> Result<VerifiedIdentity> {
        Err(ProtocolError::VerificationUnavailable(format!(
            "did:wba cryptographic verification is not implemented; refusing to verify `{}`",
            did.as_str()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_ported_dids() {
        let did = DidWba::parse("did:wba:agents.example.com:agent:altius").unwrap();
        assert_eq!(did.host(), "agents.example.com");
        assert_eq!(did.port(), None);
        assert_eq!(did.path_segments(), ["agent", "altius"]);

        let did = DidWba::parse("did:wba:localhost%3A8800:user:alice").unwrap();
        assert_eq!(did.host(), "localhost");
        assert_eq!(did.port(), Some(8800));
        assert_eq!(did.path_segments(), ["user", "alice"]);
    }

    #[test]
    fn rejects_malformed_dids() {
        for bad in [
            "",
            "did:web:example.com",
            "did:wba:",
            "did:wba:bad host",
            "did:wba:example.com%3Anotaport",
            "did:wba:example.com::empty",
            "did:wba:example.com:seg/ment",
        ] {
            assert!(DidWba::parse(bad).is_err(), "should reject `{bad}`");
        }
        let oversized = format!("did:wba:{}.com", "a".repeat(limits::MAX_DID_LEN));
        assert!(DidWba::parse(&oversized).is_err());
    }

    #[tokio::test]
    async fn stub_verifier_fails_closed() {
        let did = DidWba::parse("did:wba:agents.example.com:agent:altius").unwrap();
        let err = StubDidVerifier
            .verify(&did, b"challenge", b"signature")
            .await
            .unwrap_err();
        assert!(matches!(err, ProtocolError::VerificationUnavailable(_)));
    }
}
