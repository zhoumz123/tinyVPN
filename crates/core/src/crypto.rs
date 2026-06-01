use anyhow::Result;
use rand::rngs::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};

/// Generate a new X25519 keypair for WireGuard
pub fn generate_keypair() -> (StaticSecret, PublicKey) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret, public)
}

/// Encrypt a message with ChaCha20-Poly1305
pub fn encrypt(key: &[u8; 32], nonce_bytes: &[u8; 12], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = Nonce::from_slice(nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("encryption failed: {}", e))?;
    Ok(ciphertext)
}

/// Decrypt a message with ChaCha20-Poly1305
pub fn decrypt(key: &[u8; 32], nonce_bytes: &[u8; 12], ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed: {}", e))?;
    Ok(plaintext)
}

/// Generate a random nonce
pub fn random_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut OsRng, &mut nonce);
    nonce
}
