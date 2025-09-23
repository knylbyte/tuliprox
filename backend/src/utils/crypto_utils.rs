// Crypto utilities without OpenSSL.
// New default: XChaCha20-Poly1305 AEAD (v2 format), with auto-decrypt fallback
// to legacy AES-128-CBC+PKCS#7 (for old data).
//
// Format v2: "v2:" + base64url(no_pad, nonce(24) || ciphertext+tag)
//
// NOTE:
// - Public API keeps the same function names/signatures to avoid changing call sites.
// - Keys for v2 are derived from the provided 16-byte secret via HKDF-SHA256 to 32 bytes.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use rand::rngs::OsRng;
use rand::RngCore;

use shared::error::{TuliproxError, TuliproxErrorKind};

// ----- AEAD (preferred) -----
use aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

// ----- Legacy CBC (fallback-only) -----
use aes::Aes128;
use cbc::cipher::{block_padding::Pkcs7, KeyIvInit};
use cbc::{Decryptor, Encryptor};

// HKDF to expand 16-byte app secret to 32-byte AEAD key
use hkdf::Hkdf;
use sha2::Sha256;

fn encode_b64(data: &[u8]) -> String {
    B64.encode(data)
}

fn decode_b64(s: &str) -> Result<Vec<u8>, TuliproxError> {
    B64.decode(s).map_err(|e| TuliproxError::new(TuliproxErrorKind::Info, e.to_string()))
}

pub fn xor_bytes(secret: &[u8], data: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ secret[i % secret.len()])
        .collect()
}

pub fn obfuscate_text(secret: &[u8], text: &str) -> Result<String, String> {
    Ok(encode_b64(&xor_bytes(secret, text.as_bytes())))
}

pub fn deobfuscate_text(secret: &[u8], text: &str) -> Result<String, String> {
    let data = xor_bytes(secret, &B64.decode(text).unwrap_or_else(|_| text.as_bytes().to_vec()));
    String::from_utf8(data).map_err(|_| text.to_string())
}

// --- Key derivation: 16-byte app secret -> 32-byte AEAD key via HKDF-SHA256
fn derive_aead_key_from_secret(secret16: &[u8; 16]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(b"tuliprox-aead-v2"), secret16);
    let mut okm = [0u8; 32];
    hk.expand(b"chacha20poly1305 key", &mut okm).expect("HKDF expand");
    okm
}

// --- New encryption (v2: XChaCha20-Poly1305) ---
pub fn encrypt_text(secret: &[u8; 16], text: &str) -> Result<String, TuliproxError> {
    let aead_key = derive_aead_key_from_secret(secret);
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&aead_key));

    // 24-byte nonce
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);

    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), text.as_bytes())
        .map_err(|e| TuliproxError::new(TuliproxErrorKind::Info, e.to_string()))?;

    let mut blob = Vec::with_capacity(24 + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);

    Ok(format!("v2:{}", encode_b64(&blob)))
}

// --- Auto-decrypt: v2 → AEAD, else → legacy CBC ---
pub fn decrypt_text(secret: &[u8; 16], encrypted_text: &str) -> Result<String, TuliproxError> {
    if let Some(rest) = encrypted_text.strip_prefix("v2:") {
        // AEAD path
        let data = decode_b64(rest)?;
        if data.len() < 24 {
            return Err(TuliproxError::new(
                TuliproxErrorKind::Info,
                "ciphertext too short (v2)".to_string(),
            ));
        }
        let (nonce, ct) = data.split_at(24);

        let aead_key = derive_aead_key_from_secret(secret);
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&aead_key));

        let pt = cipher
            .decrypt(XNonce::from_slice(nonce), ct)
            .map_err(|e| TuliproxError::new(TuliproxErrorKind::Info, e.to_string()))?;

        return String::from_utf8(pt)
            .map_err(|e| TuliproxError::new(TuliproxErrorKind::Info, e.to_string()));
    }

    // Legacy CBC fallback: base64( IV(16) || CIPHERTEXT )
    let data = decode_b64(encrypted_text)?;
    if data.len() < 16 {
        return Err(TuliproxError::new(
            TuliproxErrorKind::Info,
            "ciphertext too short (legacy)".to_string(),
        ));
    }
    let (iv, ct) = data.split_at(16);

    let decryptor = Decryptor::<Aes128>::new(secret.into(), iv.into());
    let pt = decryptor
        .decrypt_padded_vec_mut::<Pkcs7>(ct)
        .map_err(|e| TuliproxError::new(TuliproxErrorKind::Info, e.to_string()))?;

    String::from_utf8(pt).map_err(|e| TuliproxError::new(TuliproxErrorKind::Info, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_roundtrip() {
        let mut s = [0u8; 16];
        OsRng.fill_bytes(&mut s);
        let msg = "hello chacha";
        let enc = encrypt_text(&s, msg).unwrap();
        assert!(enc.starts_with("v2:"));
        let dec = decrypt_text(&s, &enc).unwrap();
        assert_eq!(dec, msg);
    }

    #[test]
    fn legacy_roundtrip() {
        // Simulate legacy encryption (AES-128-CBC + PKCS7)
        let mut s = [0u8; 16];
        OsRng.fill_bytes(&mut s);

        let mut iv = [0u8; 16];
        OsRng.fill_bytes(&mut iv);
        let enc = {
            let e = Encryptor::<Aes128>::new(s.into(), iv.into());
            let ct = e.encrypt_padded_vec_mut::<Pkcs7>(b"legacy");
            let mut blob = iv.to_vec();
            blob.extend_from_slice(&ct);
            B64.encode(blob)
        };
        assert_eq!(decrypt_text(&s, &enc).unwrap(), "legacy");
    }
}
