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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let (secret, _) = generate_keypair();
        let key = secret.to_bytes();
        let nonce = random_nonce();
        let plaintext = b"hello tinyvpn";
        let ciphertext = encrypt(&key, &nonce, plaintext).unwrap();
        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_decrypt_empty() {
        let (secret, _) = generate_keypair();
        let key = secret.to_bytes();
        let nonce = random_nonce();
        let ciphertext = encrypt(&key, &nonce, b"").unwrap();
        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn encrypt_decrypt_large() {
        let (secret, _) = generate_keypair();
        let key = secret.to_bytes();
        let nonce = random_nonce();
        let plaintext = vec![0xAB_u8; 10_000];
        let ciphertext = encrypt(&key, &nonce, &plaintext).unwrap();
        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let (secret1, _) = generate_keypair();
        let (secret2, _) = generate_keypair();
        let nonce = random_nonce();
        let ciphertext = encrypt(&secret1.to_bytes(), &nonce, b"secret").unwrap();
        assert!(decrypt(&secret2.to_bytes(), &nonce, &ciphertext).is_err());
    }

    #[test]
    fn wrong_nonce_fails() {
        let (secret, _) = generate_keypair();
        let key = secret.to_bytes();
        let nonce1 = random_nonce();
        let nonce2 = random_nonce();
        let ciphertext = encrypt(&key, &nonce1, b"secret").unwrap();
        assert!(decrypt(&key, &nonce2, &ciphertext).is_err());
    }
}
