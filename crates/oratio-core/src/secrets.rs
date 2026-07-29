//! API keys in the OS credential store (macOS Keychain, Windows Credential
//! Manager, Secret Service / KWallet on Linux). settings.json then only holds
//! a placeholder, never the key itself.

const SERVICE: &str = "Oratio";

pub fn get(id: &str) -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, id).ok()?;
    entry.get_password().ok()
}

pub fn set(id: &str, secret: &str) -> bool {
    match keyring::Entry::new(SERVICE, id).and_then(|e| e.set_password(secret)) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("credential store unavailable for '{id}': {e}");
            false
        }
    }
}

pub fn delete(id: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, id) {
        let _ = entry.delete_credential();
    }
}
