//! The player's Anthropic API key for the AI coach. It is kept in Windows
//! Credential Manager (encrypted for the Windows account), never in a file.
//! Without a saved key, `ANTHROPIC_API_KEY` from the environment is used.

use serde::Serialize;

#[cfg(windows)]
const SERVICE: &str = "Build Doctor";
#[cfg(windows)]
const USER: &str = "Anthropic API key";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Saved,
    Environment,
}

/// The key to use, and where it came from.
pub fn get() -> Option<(String, Source)> {
    saved().map(|k| (k, Source::Saved)).or_else(|| {
        std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .map(|k| (k, Source::Environment))
    })
}

#[cfg(windows)]
fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, USER).map_err(|e| format!("Windows Credential Manager: {e}"))
}

#[cfg(windows)]
fn saved() -> Option<String> {
    entry().ok()?.get_password().ok()
}

#[cfg(not(windows))]
fn saved() -> Option<String> {
    None
}

/// Saves `key`, replacing any saved one.
pub fn save(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return Err("Paste the whole key, without spaces.".into());
    }
    #[cfg(windows)]
    {
        entry()?
            .set_password(key)
            .map_err(|e| format!("Could not save the key: {e}"))
    }
    #[cfg(not(windows))]
    {
        Err("Saving a key needs Windows. Set ANTHROPIC_API_KEY instead.".into())
    }
}

/// Removes the saved key, if any.
pub fn remove() -> Result<(), String> {
    #[cfg(windows)]
    {
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Could not remove the key: {e}")),
        }
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}
