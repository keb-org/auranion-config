use anyhow::{Context, Result};
use keyring::{Entry, Error};

const SERVICE: &str = "auranion";
const ACCOUNT: &str = "api-key";

pub(super) fn load() -> Result<Option<String>> {
    match entry()?.get_password() {
        Ok(key) if key.trim().is_empty() => Ok(None),
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(error).context("read Auranion API key from secure credential store"),
    }
}

pub(super) fn save(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    entry()?
        .set_password(key)
        .context("save Auranion API key in secure credential store")?;
    Ok(())
}

pub(super) fn delete() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("delete Auranion API key from secure credential store"),
    }
}

fn entry() -> Result<Entry> {
    Entry::new(SERVICE, ACCOUNT).context("access Auranion secure credential store")
}
