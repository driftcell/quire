//! Validated domain identifiers and revisions.
//!
//! Each newtype rejects invalid values at construction and at JSON deserialization time, while
//! serializing transparently as its primitive value.

use std::{
    fmt::{Display, Formatter},
    ops::Deref,
};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::error::ProtocolError;

const ID_REASON: &str =
    "use 1-64 ASCII letters/digits plus '-' '_' or '.', starting with an alphanumeric character";

#[derive(Debug, Clone, Copy)]
struct IdRules {
    allow_dot: bool,
    allow_underscore: bool,
    allow_dash: bool,
}

const DOC_ID_RULES: IdRules = IdRules {
    allow_dot: false,
    allow_underscore: true,
    allow_dash: true,
};
const BLOCK_ID_RULES: IdRules = IdRules {
    allow_dot: true,
    allow_underscore: true,
    allow_dash: true,
};
const SESSION_ID_RULES: IdRules = IdRules {
    allow_dot: true,
    allow_underscore: true,
    allow_dash: true,
};

fn validate_id(value: &str, rules: IdRules) -> Result<(), String> {
    if value.is_empty() {
        return Err("value is empty".into());
    }
    if value.len() > 64 {
        return Err("value exceeds 64 bytes".into());
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err("value is empty".into());
    };
    if !first.is_ascii_alphanumeric() {
        return Err("must start with an ASCII letter or digit".into());
    }
    for ch in chars {
        let allowed = ch.is_ascii_alphanumeric()
            || (rules.allow_dash && ch == '-')
            || (rules.allow_underscore && ch == '_')
            || (rules.allow_dot && ch == '.');
        if !allowed {
            return Err(format!("character `{ch}` is not allowed"));
        }
    }
    Ok(())
}

macro_rules! id_newtype {
    ($name:ident, $rules:expr, $error:ident, $examples:expr) => {
        /// Validated identifier.
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Parse and validate an identifier.
            pub fn parse(value: impl Into<String>) -> Result<Self, ProtocolError> {
                let value = value.into();
                validate_id(&value, $rules).map_err(|reason| ProtocolError::$error {
                    value: value.clone(),
                    reason,
                    valid_examples: $examples
                        .iter()
                        .map(|example| (*example).to_string())
                        .collect(),
                })?;
                Ok(Self(value))
            }

            /// Borrow the identifier as a string.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the newtype and return the inner string.
            pub fn into_inner(self) -> String {
                self.0
            }

            /// Suggested recovery hint for tool errors.
            pub fn recovery_hint() -> &'static str {
                ID_REASON
            }
        }

        impl TryFrom<String> for $name {
            type Error = ProtocolError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = ProtocolError;
            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_str() == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                self.as_str() == other.as_str()
            }
        }

        impl Deref for $name {
            type Target = str;
            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                Self::try_from(raw).map_err(D::Error::custom)
            }
        }
    };
}

id_newtype!(
    DocId,
    DOC_ID_RULES,
    InvalidDocId,
    &["q3-report", "report_2026"]
);
id_newtype!(
    BlockId,
    BLOCK_ID_RULES,
    InvalidBlockId,
    &["sec_intro", "b_01", "q3-revenue"]
);
id_newtype!(
    SourceId,
    BLOCK_ID_RULES,
    InvalidSourceId,
    &["sec10", "src_market"]
);
id_newtype!(
    SessionId,
    SESSION_ID_RULES,
    InvalidSessionId,
    &["sess_01", "agent-run-42"]
);

impl BlockId {
    /// Placeholder used when an agent omits a block ID. Patch insertion auto-assigns a real ID.
    pub fn pending() -> Self {
        Self("b_pending".to_string())
    }

    pub fn is_pending(&self) -> bool {
        self.0 == "b_pending"
    }
}

/// Monotonic non-negative report revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Revision(i64);

impl Default for Revision {
    fn default() -> Self {
        Self::ZERO
    }
}

impl Revision {
    /// Revision zero (an unsaved document).
    pub const ZERO: Self = Self(0);
    /// Revision one (the first committed report).
    pub const ONE: Self = Self(1);

    /// Construct a non-negative revision.
    pub fn new(value: i64) -> Result<Self, ProtocolError> {
        if value < 0 {
            Err(ProtocolError::InvalidRevision { value })
        } else {
            Ok(Self(value))
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl TryFrom<i64> for Revision {
    type Error = ProtocolError;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Revision> for i64 {
    fn from(value: Revision) -> Self {
        value.0
    }
}

impl Display for Revision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for Revision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = i64::deserialize(deserializer)?;
        Self::new(raw).map_err(D::Error::custom)
    }
}

/// Validated base-62 fractional rank.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Rank(String);

impl Rank {
    /// Parse and validate a base-62 rank.
    pub fn parse(value: impl Into<String>) -> Result<Self, ProtocolError> {
        let value = value.into();
        let valid_chars = !value.is_empty()
            && value.len() <= 64
            && value.bytes().all(|byte| byte.is_ascii_alphanumeric());
        if !valid_chars {
            return Err(ProtocolError::invalid_rank(
                value,
                "expected 1-64 ASCII base-62 characters",
            ));
        }
        if value.ends_with('0') {
            return Err(ProtocolError::invalid_rank(
                value,
                "canonical fractional ranks cannot end with the minimum digit `0`",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl TryFrom<String> for Rank {
    type Error = ProtocolError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<&str> for Rank {
    type Error = ProtocolError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl AsRef<str> for Rank {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for Rank {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Display for Rank {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Rank {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_ids() {
        assert!(DocId::parse("report_1").is_ok());
        assert!(DocId::parse("").is_err());
        assert!(DocId::parse("-leading").is_err());
        assert!(DocId::parse("has space").is_err());
        assert!(BlockId::parse("sec_intro").is_ok());
        assert!(SourceId::parse("sec10").is_ok());
        assert!(SessionId::parse("sess_01").is_ok());
        assert!(SessionId::parse("sess/01").is_err());
    }

    #[test]
    fn revision_is_non_negative() {
        assert!(Revision::new(-1).is_err());
        assert_eq!(Revision::new(3).unwrap().next(), Revision::new(4).unwrap());
    }

    #[test]
    fn rank_validation() {
        assert!(Rank::parse("U").is_ok());
        assert!(Rank::parse("0").is_err());
        assert!(Rank::parse("a!").is_err());
    }
}
