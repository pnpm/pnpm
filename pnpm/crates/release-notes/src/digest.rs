use base64::Engine;
use sha2::{Digest, Sha256};

/// The `sha256-<base64>` digest a registry records as a note's
/// `initialDigest`, of the note's UTF-8 text.
#[must_use]
pub fn note_digest(text: &str) -> String {
    let hash = Sha256::digest(text.as_bytes());
    format!("sha256-{}", base64::engine::general_purpose::STANDARD.encode(hash))
}

#[cfg(test)]
mod tests;
