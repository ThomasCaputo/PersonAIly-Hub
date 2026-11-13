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

/// Token dell'API locale (SI-4). Si crea al primo uso e resta lo stesso fra
/// un avvio e l'altro, così i programmi configurati a mano continuano a
/// funzionare.
pub fn api_token() -> Result<String> {
    let entry = Entry::new(SERVICE, "api-token").map_err(to_error)?;
    match entry.get_password() {
        Ok(token) if !token.is_empty() => Ok(token),
        Ok(_) | Err(keyring::Error::NoEntry) => {
            let token = crate::instance::new_token()?;
            entry.set_password(&token).map_err(to_error)?;
            Ok(token)
        }
        Err(e) => Err(to_error(e)),
    }
}

/// Sostituisce il token dell'API locale: quello vecchio smette di valere.
pub fn regenerate_api_token() -> Result<String> {
    let token = crate::instance::new_token()?;
    Entry::new(SERVICE, "api-token")
        .map_err(to_error)?
        .set_password(&token)
        .map_err(to_error)?;
    Ok(token)
}

/// Le variabili d'ambiente dei server MCP (spesso chiavi API), una voce del
/// portachiavi per variabile: così nessun valore finisce nell'archivio SQLite
/// e non si urta il limite di dimensione di una singola credenziale.
pub struct KeyringVault;

fn mcp_entry(server_id: &str, name: &str) -> Result<Entry> {
    Entry::new(SERVICE, &format!("mcp:{server_id}:{name}")).map_err(to_error)
}

impl crate::mcp::EnvVault for KeyringVault {
    fn get(&self, server_id: &str, name: &str) -> Result<Option<String>> {
        match mcp_entry(server_id, name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(to_error(e)),
        }
    }

    fn set(&self, server_id: &str, name: &str, value: &str) -> Result<()> {
        mcp_entry(server_id, name)?
            .set_password(value)
            .map_err(to_error)
    }

    fn delete(&self, server_id: &str, name: &str) -> Result<()> {
        match mcp_entry(server_id, name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(to_error(e)),
        }
    }
}
