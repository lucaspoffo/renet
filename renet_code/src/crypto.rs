use aead::{Aead, Error as CryptoError, NewAead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand_chacha::ChaCha20Rng;
use rand_core::{RngCore, SeedableRng};

// TODO: use decrypt_in_place
pub fn dencrypted(packet: &[u8], sequence: u64, private_key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);

    let key = Key::from_slice(private_key);
    let cipher = ChaCha20Poly1305::new(key);
    let payload = Payload { msg: packet, aad };

    cipher.decrypt(&nonce, payload)
}

// TODO: use encrypt_in_place
pub fn encrypt(packet: &[u8], sequence: u64, key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);

    let key = Key::from_slice(key);
    let cipher = ChaCha20Poly1305::new(key);
    dbg!(packet);
    let payload = Payload { msg: packet, aad };

    cipher.encrypt(&nonce, payload)
}

pub fn generate_random_bytes<const N: usize>() -> [u8; N] {
    let mut rng = ChaCha20Rng::from_entropy();
    let mut bytes = [0; N];
    rng.fill_bytes(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let key = b"an example very very secret key."; // 32-bytes
        let sequence = 2;
        let aad = b"test";

        let data = b"some packet data";

        let encrypted = encrypt(data, sequence, &key, aad).unwrap();
        let decrypted = dencrypted(&encrypted, sequence, &key, aad).unwrap();
        assert_eq!(data.to_vec(), decrypted);
    }
}
