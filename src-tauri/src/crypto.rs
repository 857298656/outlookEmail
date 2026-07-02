use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| AppError::Crypto(err.to_string()))
}

pub fn verify_password(password: &str, hash: &str) -> AppResult<bool> {
    let parsed = PasswordHash::new(hash).map_err(|err| AppError::Crypto(err.to_string()))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn derive_key(password: &str, salt: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(b":outlook-email-desktop:");
    hasher.update(salt.as_bytes());
    hasher.finalize().into()
}

pub fn random_salt() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    STANDARD.encode(bytes)
}

pub fn encrypt_text(value: &str, key: &[u8; 32]) -> AppResult<String> {
    if value.is_empty() {
        return Ok(String::new());
    }

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|err| AppError::Crypto(err.to_string()))?;
    let mut nonce_bytes = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, value.as_bytes())
        .map_err(|err| AppError::Crypto(err.to_string()))?;

    Ok(format!(
        "v1:{}:{}",
        STANDARD.encode(nonce_bytes),
        STANDARD.encode(ciphertext)
    ))
}

pub fn decrypt_text(value: &str, key: &[u8; 32]) -> AppResult<String> {
    if value.is_empty() {
        return Ok(String::new());
    }
    let Some(payload) = value.strip_prefix("v1:") else {
        return Ok(value.to_string());
    };
    let mut parts = payload.splitn(2, ':');
    let nonce = parts
        .next()
        .ok_or_else(|| AppError::Crypto("missing nonce".to_string()))
        .and_then(|part| STANDARD.decode(part).map_err(|err| AppError::Crypto(err.to_string())))?;
    let ciphertext = parts
        .next()
        .ok_or_else(|| AppError::Crypto("missing ciphertext".to_string()))
        .and_then(|part| STANDARD.decode(part).map_err(|err| AppError::Crypto(err.to_string())))?;

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|err| AppError::Crypto(err.to_string()))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|err| AppError::Crypto(err.to_string()))?;

    String::from_utf8(plaintext).map_err(|err| AppError::Crypto(err.to_string()))
}
