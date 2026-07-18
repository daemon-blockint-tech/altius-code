//! Wire protocol between `altius-txguard` (or any other caller) and the
//! signer process. Deliberately narrow: two requests, nothing that could
//! ever expose key material.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Pubkey,
    Sign { message: Vec<u8> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Pubkey { bytes: Vec<u8> },
    Signature { bytes: Vec<u8> },
    Error { message: String },
}
