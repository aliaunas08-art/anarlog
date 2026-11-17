//! Entry shape, the "ids and counts only" validation (D1.5) and the
//! canonical form that is hashed (D1.3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Error;

/// `prev_hash` of the first entry.
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

const MAX_ID_LEN: usize = 160;
const MAX_IDS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum Source {
    App,
    Cli,
    Job,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::App => "app",
            Source::Cli => "cli",
            Source::Job => "job",
        }
    }
}

/// A detail value. There is deliberately no free-text variant: identifiers
/// are checked by [`is_identifier`] (no spaces, bounded length), so a
/// sentence of meeting content cannot be stored by mistake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DetailValue {
    Count(i64),
    Flag(bool),
    Id(String),
    Ids(Vec<String>),
}

pub type Detail = BTreeMap<String, DetailValue>;

/// What a caller supplies; `seq`, `ts` and the hashes are added by the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEntry {
    pub actor_id: String,
    pub role: String,
    pub workspace_id: String,
    pub action: String,
    pub object_type: String,
    pub object_id: String,
    pub source: Source,
    pub device_id: String,
    pub detail: Detail,
}

impl NewEntry {
    /// An entry with an empty actor context (filled by the Tauri helper or
    /// the caller).
    pub fn new(action: &str, object_type: &str, object_id: impl Into<String>) -> Self {
        Self {
            actor_id: String::new(),
            role: String::new(),
            workspace_id: String::new(),
            action: action.to_string(),
            object_type: object_type.to_string(),
            object_id: object_id.into(),
            source: Source::App,
            device_id: String::new(),
            detail: Detail::new(),
        }
    }

    pub fn with(mut self, key: &str, value: DetailValue) -> Self {
        self.detail.insert(key.to_string(), value);
        self
    }

    pub fn count(self, key: &str, value: i64) -> Self {
        self.with(key, DetailValue::Count(value))
    }

    pub fn flag(self, key: &str, value: bool) -> Self {
        self.with(key, DetailValue::Flag(value))
    }

    pub fn id(self, key: &str, value: impl Into<String>) -> Self {
        self.with(key, DetailValue::Id(value.into()))
    }

    pub fn ids(self, key: &str, values: Vec<String>) -> Self {
        self.with(key, DetailValue::Ids(values))
    }

    pub fn source(mut self, source: Source) -> Self {
        self.source = source;
        self
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        let names = [("action", &self.action), ("object_type", &self.object_type)];
        for (field, value) in names {
            if value.is_empty() || !is_name(value) {
                return Err(Error::InvalidField(field));
            }
        }
        let ids = [
            ("actor_id", &self.actor_id),
            ("role", &self.role),
            ("workspace_id", &self.workspace_id),
            ("object_id", &self.object_id),
            ("device_id", &self.device_id),
        ];
        for (field, value) in ids {
            if !value.is_empty() && !is_identifier(value) {
                return Err(Error::InvalidField(field));
            }
        }
        for (key, value) in &self.detail {
            if !is_name(key) {
                return Err(Error::InvalidField("detail key"));
            }
            let ok = match value {
                DetailValue::Count(_) | DetailValue::Flag(_) => true,
                DetailValue::Id(id) => id.is_empty() || is_identifier(id),
                DetailValue::Ids(ids) => {
                    ids.len() <= MAX_IDS && ids.iter().all(|id| is_identifier(id))
                }
            };
            if !ok {
                return Err(Error::InvalidField("detail value"));
            }
        }
        Ok(())
    }
}

/// Lower-case dotted names: actions, object types, detail keys.
fn is_name(value: &str) -> bool {
    value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.'))
}

/// Ids, roles and setting or key names: no whitespace, bounded length.
pub fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(b, b'-' | b'_' | b'.' | b':' | b'/' | b'@' | b'+' | b'#')
        })
}

/// A stored entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Entry {
    pub seq: i64,
    pub ts: String,
    pub actor_id: String,
    pub role: String,
    pub workspace_id: String,
    pub action: String,
    pub object_type: String,
    pub object_id: String,
    pub source: String,
    pub device_id: String,
    /// JSON object of ids, counts and flags only.
    pub detail: String,
    pub prev_hash: String,
    pub hash: String,
}

impl Entry {
    /// The canonical form hashed into the chain: a JSON array of every field
    /// except the hashes, in a fixed order.
    pub fn canonical(&self) -> String {
        serde_json::Value::Array(vec![
            self.seq.into(),
            self.ts.as_str().into(),
            self.actor_id.as_str().into(),
            self.role.as_str().into(),
            self.workspace_id.as_str().into(),
            self.action.as_str().into(),
            self.object_type.as_str().into(),
            self.object_id.as_str().into(),
            self.source.as_str().into(),
            self.device_id.as_str().into(),
            self.detail.as_str().into(),
        ])
        .to_string()
    }

    /// `SHA-256(prev_hash ‖ canonical entry)`, lower-case hex.
    pub fn compute_hash(&self) -> String {
        chain_hash(&self.prev_hash, &self.canonical())
    }
}

pub fn chain_hash(prev_hash: &str, canonical: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(canonical.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_shaped_values_are_refused() {
        let ok = NewEntry::new("ask.query", "ask", "")
            .id("scope", "library")
            .ids("meeting_ids", vec!["m-1".into(), "m-2".into()])
            .count("results", 3);
        assert!(ok.validate().is_ok());

        let sentence = NewEntry::new("ask.query", "ask", "").id("q", "what did Bob say");
        assert!(matches!(
            sentence.validate(),
            Err(Error::InvalidField("detail value"))
        ));
        let title = NewEntry::new("meeting.viewed", "meeting", "Board meeting notes");
        assert!(matches!(
            title.validate(),
            Err(Error::InvalidField("object_id"))
        ));
        let action = NewEntry::new("Meeting Viewed", "meeting", "m");
        assert!(action.validate().is_err());
        let long = NewEntry::new("x.y", "meeting", "a".repeat(500));
        assert!(long.validate().is_err());
    }
}
