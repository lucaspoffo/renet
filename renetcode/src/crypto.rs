use chacha20poly1305::aead::{rand_core::RngCore, OsRng};
use chacha20poly1305::{AeadInPlace, ChaCha20Poly1305, Error as CryptoError, Key, KeyInit, Nonce, Tag, XChaCha20Poly1305, XNonce};

use crate::{ENCODED_PACKET_TAG_BYTES, NETCODE_MAC_BYTES};

pub fn decode_and_check_buffer(buffer: &[u8], protocol_id: u64) -> Result<(), ()> {
    let (_, buffer_tag) = buffer.split_at(buffer.len() - ENCODED_PACKET_TAG_BYTES);
    let protocol_tag = u64::from_le_bytes(buffer_tag.try_into().unwrap());

    if protocol_tag != protocol_id {
        return Err(());
    }

    Ok(())
}

pub fn dencrypted_in_place(buffer: &mut [u8], sequence: u64, private_key: &[u8; 32], aad: &[u8]) -> Result<(), CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);
    let (buffer, tag) = buffer.split_at_mut(buffer.len() - NETCODE_MAC_BYTES);
    let tag = Tag::from_slice(tag);

    let key = Key::from_slice(private_key);
    let cipher = ChaCha20Poly1305::new(key);

    cipher.decrypt_in_place_detached(&nonce, aad, buffer, tag)
}

pub fn dencrypted_in_place_xnonce(buffer: &mut [u8], xnonce: &[u8; 24], private_key: &[u8; 32], aad: &[u8]) -> Result<(), CryptoError> {
    let xnonce = XNonce::from_slice(xnonce);
    let (buffer, tag) = buffer.split_at_mut(buffer.len() - NETCODE_MAC_BYTES);
    let tag = Tag::from_slice(tag);

    let key = Key::from_slice(private_key);
    let cipher = XChaCha20Poly1305::new(key);

    cipher.decrypt_in_place_detached(xnonce, aad, buffer, tag)
}

pub fn encode_in_place(buffer: &mut [u8], protocol_id: u64) {
    let (_, buffer_tag) = buffer.split_at_mut(buffer.len() - ENCODED_PACKET_TAG_BYTES);
    buffer_tag.copy_from_slice(&protocol_id.to_le_bytes());
}

pub fn encrypt_in_place(buffer: &mut [u8], sequence: u64, key: &[u8; 32], aad: &[u8]) -> Result<(), CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);
    let (buffer, buffer_tag) = buffer.split_at_mut(buffer.len() - NETCODE_MAC_BYTES);

    let key = Key::from_slice(key);
    let cipher = ChaCha20Poly1305::new(key);
    let tag = cipher.encrypt_in_place_detached(&nonce, aad, buffer)?;
    buffer_tag.copy_from_slice(&tag);

    Ok(())
}

pub fn encrypt_in_place_xnonce(buffer: &mut [u8], xnonce: &[u8; 24], key: &[u8; 32], aad: &[u8]) -> Result<(), CryptoError> {
    let (buffer, buffer_tag) = buffer.split_at_mut(buffer.len() - NETCODE_MAC_BYTES);

    let xnonce = XNonce::from_slice(xnonce);
    let key = Key::from_slice(key);
    let cipher = XChaCha20Poly1305::new(key);
    let tag = cipher.encrypt_in_place_detached(xnonce, aad, buffer)?;
    buffer_tag.copy_from_slice(&tag);

    Ok(())
}

/// Generate a buffer with random bytes using randomness from the operating system.
///
/// The implementation is provided by the `getrandom` crate. Refer to
/// `getrandom` documentation for details.
pub fn generate_random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_in_place() {
        let protocol_id = 42;

        let mut data = b"some packet data".to_vec();
        let data_len = data.len();
        data.extend_from_slice(&[0u8; ENCODED_PACKET_TAG_BYTES]);

        encode_in_place(&mut data, protocol_id);
        decode_and_check_buffer(&data, protocol_id).unwrap();
        assert_eq!(&data[..data_len], b"some packet data");
    }

    #[test]
    fn test_encrypt_decrypt_in_place() {
        let key = b"an example very very secret key."; // 32-bytes
        let sequence = 2;
        let aad = b"test";

        let mut data = b"some packet data".to_vec();
        let data_len = data.len();
        data.extend_from_slice(&[0u8; NETCODE_MAC_BYTES]);

        encrypt_in_place(&mut data, sequence, key, aad).unwrap();
        dencrypted_in_place(&mut data, sequence, key, aad).unwrap();
        assert_eq!(&data[..data_len], b"some packet data");
    }
}
