pub(crate) mod format;

pub(crate) use format::mint_token;
pub use format::{PERSONAL_TOKEN_LEN, PERSONAL_TOKEN_PREFIX, has_valid_personal_token_checksum};
pub(super) use pnpm_crypto_hash::create_hex_hash_bytes as sha256_hex;
pub use record::{NewToken, TokenKind, TokenRecord};

mod record;

use super::{RegistryError, Result, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------
// crypto helpers
// ---------------------------------------------------------------

/// Build a freshly-randomized secret for a token or an account ID.
/// Pulls 32 bytes from the OS CSPRNG (`getrandom` → `/dev/urandom`
/// on Linux, `BCryptGenRandom` on Windows, `getentropy` on macOS).
/// We refuse to start the server if the OS RNG is unavailable
/// rather than fall back to weaker entropy — token unguessability
/// is the whole reason this exists.
pub(super) fn fresh_secret() -> [u8; 32] {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).expect("OS CSPRNG must be available");
    secret
}

/// Current application time in milliseconds since the Unix epoch.
#[must_use]
pub fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}
