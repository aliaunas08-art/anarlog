//! Account records and refresh tokens, kept in the macOS Keychain.
//!
//! The account list (no secrets: provider, email, name) and one refresh token
//! per account are separate keychain items under the same service name.
//! Access tokens only ever live in memory.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::Error;

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, Error>;
    fn set(&self, key: &str, value: &str) -> Result<(), Error>;
    fn delete(&self, key: &str) -> Result<(), Error>;
}

pub struct KeychainStore {
    service: String,
}

impl KeychainStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, Error> {
        keyring::Entry::new(&self.service, key).map_err(|e| Error::Keychain(e.to_string()))
    }
}

impl SecretStore for KeychainStore {
    fn get(&self, key: &str) -> Result<Option<String>, Error> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Keychain(e.to_string())),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), Error> {
        self.entry(key)?
            .set_password(value)
            .map_err(|e| Error::Keychain(e.to_string()))
    }

    fn delete(&self, key: &str) -> Result<(), Error> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Keychain(e.to_string())),
        }
    }
}

/// For tests.
#[derive(Default)]
pub struct MemoryStore {
    items: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, Error> {
        Ok(self.items.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), Error> {
        self.items
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), Error> {
        self.items.lock().unwrap().remove(key);
        Ok(())
    }
}

pub(crate) const INDEX_KEY: &str = "accounts";

pub(crate) fn refresh_key(account_id: &str) -> String {
    format!("refresh/{account_id}")
}
