use aead::{Aead, AeadInPlace, Error as CryptoError, NewAead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, Tag};
use rand_chacha::ChaCha20Rng;
use rand_core::{RngCore, SeedableRng};

pub fn dencrypted(packet: &[u8], sequence: u64, private_key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);

    let key = Key::from_slice(private_key);
    let cipher = ChaCha20Poly1305::new(key);
    let payload = Payload { msg: packet, aad };

    cipher.decrypt(&nonce, payload)
}

pub fn dencrypted_in_place(
    buffer: &mut [u8],
    sequence: u64,
    private_key: &[u8; 32],
    aad: &[u8],
    tag: &Tag,
) -> Result<(), CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);

    let key = Key::from_slice(private_key);
    let cipher = ChaCha20Poly1305::new(key);

    cipher.decrypt_in_place_detached(&nonce, aad, buffer, tag)
}

pub fn encrypt(packet: &[u8], sequence: u64, key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);

    let key = Key::from_slice(key);
    let cipher = ChaCha20Poly1305::new(key);
    let payload = Payload { msg: packet, aad };

    cipher.encrypt(&nonce, payload)
}

pub fn encrypt_in_place(buffer: &mut [u8], sequence: u64, key: &[u8; 32], aad: &[u8]) -> Result<Tag, CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);

    let key = Key::from_slice(key);
    let cipher = ChaCha20Poly1305::new(key);
    cipher.encrypt_in_place_detached(&nonce, aad, buffer)
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
    fn test_encrypt_decrypt_in_place() {
        let key = b"an example very very secret key."; // 32-bytes
        let sequence = 2;
        let aad = b"test";

        let mut data = b"some packet data".to_vec();

        let tag = encrypt_in_place(&mut data, sequence, &key, aad).unwrap();
        dencrypted_in_place(&mut data, sequence, &key, aad, &tag).unwrap();
        assert_eq!(&data, b"some packet data");
    }

    #[test]
    fn test_encrypt_decrypt() {
        let key = b"an example very very secret key."; // 32-bytes
        let sequence = 2;
        let aad = b"test";

        let data = b"some packet data";

        let encrypted = encrypt(data, sequence, &key, aad).unwrap();
        let decrypted = dencrypted(&encrypted, sequence, &key, aad).unwrap();
        assert_eq!(data.len() + crate::NETCODE_MAC_BYTES, encrypted.len());
        assert_eq!(data.to_vec(), decrypted);
    }
}
