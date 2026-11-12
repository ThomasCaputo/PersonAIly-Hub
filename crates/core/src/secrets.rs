//! Chiavi API nel portachiavi del sistema operativo (su Windows: Gestore
//! credenziali). Le chiavi non vengono mai scritte in file dell'app.

use keyring::Entry;

use crate::error::{Error, Result};

const SERVICE: &str = "progetto-ai";

fn entry(provider_id: &str) -> Result<Entry> {
    Entry::new(SERVICE, &format!("provider:{provider_id}")).map_err(to_error)
}

fn to_error(e: keyring::Error) -> Error {
    Error::msg(format!("portachiavi di sistema: {e}"))
}

pub fn set_api_key(provider_id: &str, key: &str) -> Result<()> {
    entry(provider_id)?.set_password(key).map_err(to_error)
}

/// La chiave salvata per un provider, se c'è.
pub fn api_key(provider_id: &str) -> Result<Option<String>> {
    match entry(provider_id)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(to_error(e)),
    }
}

pub fn delete_api_key(provider_id: &str) -> Result<()> {
    match entry(provider_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(to_error(e)),
    }
}
