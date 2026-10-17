//! Connection secrets live only in the OS keychain. They are never persisted elsewhere,
//! logged, or returned to the frontend.

use crate::error::{AppError, AppResult};

const SERVICE: &str = "localboard";

fn entry(reference: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, reference).map_err(|error| AppError::Secret(error.to_string()))
}

pub fn save(reference: &str, secret: &str) -> AppResult<()> {
    entry(reference)?.set_password(secret).map_err(|error| AppError::Secret(error.to_string()))
}

pub fn get(reference: &str) -> AppResult<String> {
    entry(reference)?.get_password().map_err(|error| AppError::Secret(error.to_string()))
}

pub fn delete(reference: &str) -> AppResult<()> {
    match entry(reference)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(AppError::Secret(error.to_string())),
    }
}
