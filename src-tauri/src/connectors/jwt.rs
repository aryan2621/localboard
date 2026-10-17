//! RS256 JWT signing for service-account style logins (Snowflake key pair, Google service accounts).

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePublicKey};
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn parse_key(pem: &str) -> Result<RsaPrivateKey, String> {
    let pem = pem.trim();
    RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
        .map_err(|_| "The private key must be an unencrypted RSA key in PEM format (PKCS#8 or PKCS#1)".to_owned())
}

pub fn sign_rs256(key: &RsaPrivateKey, claims: &Value) -> Result<String, String> {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
    let message = format!("{header}.{payload}");
    let signature = SigningKey::<Sha256>::new(key.clone()).sign(message.as_bytes());
    Ok(format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes())))
}

/// Snowflake's `SHA256:<base64>` fingerprint of the public key.
pub fn snowflake_fingerprint(key: &RsaPrivateKey) -> Result<String, String> {
    let der = key.to_public_key().to_public_key_der().map_err(|error| error.to_string())?;
    Ok(format!("SHA256:{}", STANDARD.encode(Sha256::digest(der.as_bytes()))))
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}
