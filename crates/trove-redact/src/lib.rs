//! Shared redaction detection, extracted verbatim from trove-ship: the
//! deterministic detector registry (detectors.rs), the `.troveignore`
//! allow/deny rules that tune it (troveignore.rs), and the shared types.
//! trove-ship keeps only the redaction pipeline (Redactor, verification,
//! agent review) and re-exports everything from here.

pub mod detectors;
pub mod troveignore;

use serde::Serialize;

pub use detectors::{Detection, LocalIdentity};
pub use troveignore::TroveIgnore;

/// What kind of private information a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Secret,
    Email,
    Phone,
    InternalHost,
    IpAddress,
    LocalPath,
    Username,
    CustomerName,
    Denylisted,
    AgentFlagged,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::Email => "email",
            Self::Phone => "phone",
            Self::InternalHost => "internal-host",
            Self::IpAddress => "ip-address",
            Self::LocalPath => "local-path",
            Self::Username => "username",
            Self::CustomerName => "customer-name",
            Self::Denylisted => "denylisted",
            Self::AgentFlagged => "agent-flagged",
        }
    }

    /// Short tag used inside placeholders such as `[REDACTED:email-1]`.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::Email => "email",
            Self::Phone => "phone",
            Self::InternalHost => "host",
            Self::IpAddress => "ip",
            Self::LocalPath => "path",
            Self::Username => "user",
            Self::CustomerName => "customer",
            Self::Denylisted => "term",
            Self::AgentFlagged => "private",
        }
    }

    /// Whether two spellings that differ only in case are the same value.
    pub fn case_insensitive(self) -> bool {
        !matches!(self, Self::Secret)
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "secret" | "token" | "credential" | "password" | "key" => Self::Secret,
            "email" => Self::Email,
            "phone" | "phone-number" => Self::Phone,
            "host" | "hostname" | "internal-host" | "domain" | "url" => Self::InternalHost,
            "ip" | "ip-address" => Self::IpAddress,
            "path" | "local-path" => Self::LocalPath,
            "user" | "username" | "person" | "name" => Self::Username,
            "customer" | "customer-name" | "client" | "company" => Self::CustomerName,
            _ => Self::AgentFlagged,
        }
    }
}

/// How sure a detector is. High-confidence secrets are what the fail-closed
/// verification refuses to let through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}
