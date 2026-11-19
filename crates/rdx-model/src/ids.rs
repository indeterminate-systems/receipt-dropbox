use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest as _, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error(
        "invalid {kind} `{value}`: use lowercase letters, digits, `.`, `_` and `-`, starting with a letter or digit"
    )]
    Id { kind: &'static str, value: String },
    #[error("invalid digest `{0}`: expected `sha256:` and 64 lowercase hex digits")]
    Digest(String),
}

fn is_slug(s: &str) -> bool {
    let mut bytes = s.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'0'..=b'9'))
        && bytes.all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
        && s.len() <= 128
}

macro_rules! slug_id {
    ($(#[$doc:meta])* $name:ident, $kind:literal) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                if is_slug(&value) {
                    Ok(Self(value))
                } else {
                    Err(IdError::Id { kind: $kind, value })
                }
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({:?})", stringify!($name), self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

slug_id!(
    /// A supporter: someone who pays org costs from a personal account. Pseudonymous on purpose;
    /// names and addresses live with keys (roadmap piece 8), not in the ledger.
    SupporterId,
    "supporter id"
);
slug_id!(
    /// One statement (one account, one period).
    StatementId,
    "statement id"
);
slug_id!(
    /// A line within one statement; unique only within it (see [`crate::LineRef`]).
    LineId,
    "line id"
);
slug_id!(
    /// A receipt, bill or refund note.
    ReceiptId,
    "receipt id"
);

/// `sha256:<64 lowercase hex>`. Over canonical JSON for structured values (same as
/// `rdx_plan::digest`), or over the raw bytes for documents.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Digest(String);

impl Digest {
    pub fn parse(s: &str) -> Result<Self, IdError> {
        match s.strip_prefix("sha256:") {
            Some(hex)
                if hex.len() == 64
                    && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) =>
            {
                Ok(Digest(s.to_string()))
            }
            _ => Err(IdError::Digest(s.to_string())),
        }
    }

    pub fn of_bytes(bytes: &[u8]) -> Self {
        let hash = Sha256::digest(bytes);
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        Digest(format!("sha256:{hex}"))
    }

    pub fn of_json(value: &serde_json::Value) -> Self {
        Digest(rdx_plan::digest(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({})", self.0)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Digest::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
