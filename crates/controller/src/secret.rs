//! Random secrets and the hashes the database keeps instead of them.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// A random, URL-safe secret with 256 bits from the operating system.
pub fn token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the operating system has no random source");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The SHA-256 of a secret. The database stores this instead of the secret.
pub fn hash(secret: &str) -> Vec<u8> {
    Sha256::digest(secret.as_bytes()).to_vec()
}
