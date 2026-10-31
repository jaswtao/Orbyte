//! Host-key fingerprints and comparison.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

use crate::error::SshError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedKey {
    pub algorithm: String,
    pub public_key: Vec<u8>,
    pub fingerprint: String,
}

impl PresentedKey {
    pub fn new(algorithm: impl Into<String>, public_key: Vec<u8>) -> Self {
        let fingerprint = fingerprint_sha256(&public_key);
        Self { algorithm: algorithm.into(), public_key, fingerprint }
    }
}

pub fn fingerprint_sha256(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    let mut encoded = STANDARD.encode(digest);
    while encoded.ends_with('=') {
        encoded.pop();
    }
    format!("SHA256:{encoded}")
}

pub fn keys_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b;
    }
    diff == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyVerdict {
    Trusted,
    FirstUse,
    Mismatch,
}

pub fn judge(stored: Option<&[u8]>, presented: &[u8]) -> HostKeyVerdict {
    match stored {
        None => HostKeyVerdict::FirstUse,
        Some(stored) if keys_equal(stored, presented) => HostKeyVerdict::Trusted,
        Some(_) => HostKeyVerdict::Mismatch,
    }
}

pub fn mismatch(saved_fingerprint: String, presented: String) -> SshError {
    SshError::HostKeyChanged { saved: saved_fingerprint, presented }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_algorithm_requires_the_same_public_key_bytes() {
        let key = PresentedKey::new("ssh-ed25519", b"key-a".to_vec());
        assert_eq!(judge(None, &key.public_key), HostKeyVerdict::FirstUse);
        assert_eq!(judge(Some(&key.public_key), &key.public_key), HostKeyVerdict::Trusted);
        assert_eq!(judge(Some(b"key-b"), &key.public_key), HostKeyVerdict::Mismatch);
        assert!(key.fingerprint.starts_with("SHA256:"));
        assert_ne!(fingerprint_sha256(b"key-a"), fingerprint_sha256(b"key-b"));
    }
}
