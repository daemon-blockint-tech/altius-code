use std::fmt;

/// Which SVM program framework a workspace is written against.
///
/// Detection order matters: `Anchor.toml` is checked first because an
/// Anchor workspace's `Cargo.toml` files also depend on `solana-program`
/// transitively, which would otherwise be mistaken for a native project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Framework {
    /// Project driven by an `Anchor.toml` workspace manifest.
    Anchor,
    /// Project depending directly on the `pinocchio` crate (no IDL, no
    /// Anchor macros).
    Pinocchio,
    /// Project depending on `solana-program` / `solana-sdk` without
    /// Anchor or Pinocchio.
    Native,
}

impl fmt::Display for Framework {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Framework::Anchor => "anchor",
            Framework::Pinocchio => "pinocchio",
            Framework::Native => "native",
        };
        f.write_str(name)
    }
}
