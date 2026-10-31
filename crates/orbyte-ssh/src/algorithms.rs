//! Explicit host-key, key-exchange, and MAC preferences.
//!
//! SHA-1 host keys, key exchange, and MACs are rejected before any password is sent.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use ssh2::{MethodType, Session};

use crate::error::{SshError, SshResult};

pub const ALLOWED_HOSTKEYS: &[&str] = &["ssh-ed25519", "rsa-sha2-512", "rsa-sha2-256"];
const KEX: &str = "curve25519-sha256,curve25519-sha256@libssh.org,ecdh-sha2-nistp256,ecdh-sha2-nistp384,ecdh-sha2-nistp521,diffie-hellman-group-exchange-sha256,diffie-hellman-group16-sha512,diffie-hellman-group18-sha512,diffie-hellman-group14-sha256";
const MAC: &str = "hmac-sha2-256-etm@openssh.com,hmac-sha2-512-etm@openssh.com,hmac-sha2-256,hmac-sha2-512";
const CIPHER: &str = "aes256-gcm@openssh.com,aes128-gcm@openssh.com,aes256-ctr,aes192-ctr,aes128-ctr";
const SIGN: &str = "ssh-ed25519,rsa-sha2-512,rsa-sha2-256";

/// Saved algorithms are offered first. Unsaved algorithms stay in the list so a
/// first-time algorithm can still be challenged, but a negotiated algorithm that
/// is not one of the saved ones is rejected later.
pub fn hostkey_preference(saved: &[String]) -> String {
    let mut ordered = Vec::new();
    for algorithm in saved {
        if ALLOWED_HOSTKEYS.contains(&algorithm.as_str()) && !ordered.contains(algorithm) {
            ordered.push(algorithm.clone());
        }
    }
    for algorithm in ALLOWED_HOSTKEYS {
        if !ordered.iter().any(|saved| saved == algorithm) {
            ordered.push((*algorithm).to_owned());
        }
    }
    ordered.join(",")
}

pub fn apply_preferences(session: &Session, saved_hostkeys: &[String]) -> SshResult<()> {
    session.method_pref(MethodType::Kex, KEX).map_err(|_| SshError::WeakAlgorithm)?;
    session.method_pref(MethodType::HostKey, &hostkey_preference(saved_hostkeys)).map_err(|_| SshError::WeakAlgorithm)?;
    session.method_pref(MethodType::MacCs, MAC).map_err(|_| SshError::WeakAlgorithm)?;
    session.method_pref(MethodType::MacSc, MAC).map_err(|_| SshError::WeakAlgorithm)?;
    session.method_pref(MethodType::CryptCs, CIPHER).map_err(|_| SshError::WeakAlgorithm)?;
    session.method_pref(MethodType::CryptSc, CIPHER).map_err(|_| SshError::WeakAlgorithm)?;
    session.method_pref(MethodType::SignAlgo, SIGN).map_err(|_| SshError::WeakAlgorithm)?;
    Ok(())
}

pub fn negotiated_is_acceptable(hostkey: &str, kex: &str, mac_cs: &str, mac_sc: &str, cipher_cs: &str, cipher_sc: &str) -> bool {
    ALLOWED_HOSTKEYS.contains(&hostkey)
        && !kex.is_empty()
        && !kex.to_ascii_lowercase().contains("sha1")
        && mac_is_acceptable(mac_cs, cipher_cs)
        && mac_is_acceptable(mac_sc, cipher_sc)
}

fn mac_is_acceptable(mac: &str, cipher: &str) -> bool {
    if mac.to_ascii_lowercase().contains("sha1") {
        return false;
    }
    if mac.is_empty() {
        let cipher = cipher.to_ascii_lowercase();
        return cipher.contains("gcm") || cipher.contains("chacha20");
    }
    true
}

/// Saved algorithms must win. Landing on a different, not-yet-trusted algorithm is a downgrade.
pub fn allows_negotiated_hostkey(saved: &[String], negotiated: &str) -> Result<(), SshError> {
    if saved.is_empty() || saved.iter().any(|algorithm| algorithm == negotiated) {
        Ok(())
    } else {
        Err(SshError::AlgorithmDowngrade)
    }
}

pub fn negotiated_names(session: &Session) -> Option<Negotiated> {
    Some(Negotiated {
        hostkey: session.methods(MethodType::HostKey)?.to_owned(),
        kex: session.methods(MethodType::Kex)?.to_owned(),
        mac_cs: session.methods(MethodType::MacCs).unwrap_or("").to_owned(),
        mac_sc: session.methods(MethodType::MacSc).unwrap_or("").to_owned(),
        cipher_cs: session.methods(MethodType::CryptCs).unwrap_or("").to_owned(),
        cipher_sc: session.methods(MethodType::CryptSc).unwrap_or("").to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Negotiated {
    pub hostkey: String,
    pub kex: String,
    pub mac_cs: String,
    pub mac_sc: String,
    pub cipher_cs: String,
    pub cipher_sc: String,
}

impl Negotiated {
    pub fn acceptable(&self) -> bool {
        negotiated_is_acceptable(&self.hostkey, &self.kex, &self.mac_cs, &self.mac_sc, &self.cipher_cs, &self.cipher_sc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_hostkeys_are_offered_first_and_weak_names_fail() {
        assert_eq!(hostkey_preference(&["rsa-sha2-256".into()]), "rsa-sha2-256,ssh-ed25519,rsa-sha2-512");
        assert!(negotiated_is_acceptable("ssh-ed25519", "curve25519-sha256", "hmac-sha2-256", "hmac-sha2-256", "aes256-ctr", "aes256-ctr"));
        assert!(!negotiated_is_acceptable("ssh-rsa", "curve25519-sha256", "hmac-sha2-256", "hmac-sha2-256", "aes256-ctr", "aes256-ctr"));
        assert!(!negotiated_is_acceptable("ssh-ed25519", "diffie-hellman-group14-sha1", "hmac-sha2-256", "hmac-sha2-256", "aes256-ctr", "aes256-ctr"));
        assert!(!negotiated_is_acceptable("ssh-ed25519", "curve25519-sha256", "hmac-sha1", "hmac-sha2-256", "aes256-ctr", "aes256-ctr"));
        assert!(allows_negotiated_hostkey(&["ssh-ed25519".into()], "ssh-ed25519").is_ok());
        assert!(allows_negotiated_hostkey(&["ssh-ed25519".into()], "rsa-sha2-512").is_err());
        assert!(allows_negotiated_hostkey(&[], "ssh-ed25519").is_ok());
    }
}
