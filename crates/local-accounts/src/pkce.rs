//! PKCE (RFC 7636) and the anti-forgery `state` value.

use base64::Engine as _;
use sha2::{Digest, Sha256};

pub(crate) struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// 64 hex characters from two random v4 UUIDs: 244 random bits, inside the
/// 43–128 character range and using only unreserved characters.
fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub(crate) fn new_pkce() -> Pkce {
    let verifier = random_token();
    let challenge = challenge_for(&verifier);
    Pkce {
        verifier,
        challenge,
    }
}

pub(crate) fn challenge_for(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

pub(crate) fn new_state() -> String {
    random_token()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_rfc_7636_example() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifiers_are_long_unique_and_unreserved() {
        let a = new_pkce();
        let b = new_pkce();
        assert_ne!(a.verifier, b.verifier);
        assert_eq!(a.verifier.len(), 64);
        assert!(a.verifier.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(new_state(), new_state());
    }
}
