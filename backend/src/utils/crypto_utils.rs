use base64::{engine::general_purpose, Engine as _};
use aes::Aes128;
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher};
use rand::{RngCore, rngs::OsRng, TryRngCore};
use shared::error::{TuliproxError, TuliproxErrorKind};

pub fn encode_base64_hash(text: &str) -> String {
    let hash = blake3::hash(text.as_bytes());
    encode_base64_string(hash.as_bytes())
}

pub fn encode_base64_string(input: &[u8]) -> String {
    general_purpose::URL_SAFE_NO_PAD.encode(input)
}

pub fn decode_base64_string(input: &str) -> Vec<u8> {
    general_purpose::URL_SAFE_NO_PAD.decode(input).unwrap_or_else(|_| input.as_bytes().to_vec())
}

pub fn xor_bytes(secret: &[u8], data: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ secret[i % secret.len()])
        .collect()
}

pub fn obfuscate_text(secret: &[u8], text: &str) -> Result<String, String> {
    Ok(encode_base64_string(&xor_bytes(secret, text.as_bytes())))
}

pub fn deobfuscate_text(secret: &[u8], text: &str) -> Result<String, String> {
    let data = xor_bytes(secret, &decode_base64_string(text));
    if let Ok(result) = String::from_utf8(data) {
        Ok(result)
    } else {
        Err(text.to_string())
    }
}

pub fn obscure_text(secret: &[u8;16], url: &str) -> Result<String, TuliproxError> {
    let mut iv = [0u8; 16];
    if OsRng.try_fill_bytes(&mut iv).is_err() {
        rand::rng().fill_bytes(&mut iv);
    }

    // AES-CTR (compatible with OpenSSL's aes_128_ctr)
    type Aes128Ctr = Ctr128BE<Aes128>;
    let mut buf = url.as_bytes().to_vec();
    let mut cipher = Aes128Ctr::new_from_slices(secret, &iv)
        .map_err(|_err| TuliproxError::new(TuliproxErrorKind::Info, "Can't create cipher".to_string()))?;
    cipher.apply_keystream(&mut buf);

    // IV + Ciphertext → URL-safe Base64
    let mut out = Vec::with_capacity(iv.len() + buf.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(&buf);
    Ok(general_purpose::URL_SAFE_NO_PAD.encode(out))
}


pub fn deobscure_text(secret: &[u8;16], encoded: &str) -> Result<String, TuliproxError> {
    // Base64 decode
    let data = general_purpose::URL_SAFE_NO_PAD.decode(encoded).map_err(|_err| TuliproxError::new(TuliproxErrorKind::Info, "Can't decode base64".to_string()))?;

   if data.len() < 16 {
       return Err(TuliproxError::new(
           TuliproxErrorKind::Info,
           "Token too short to contain IV".to_string(),
       ));
   }

    let (iv, ciphertext) = data.split_at(16);

    // AES-CTR: encryption and decryption are identical (XOR with keystream).
    type Aes128Ctr = Ctr128BE<Aes128>;
    let mut buf = ciphertext.to_vec();
    let mut cipher = Aes128Ctr::new_from_slices(secret, iv)
        .map_err(|_err| TuliproxError::new(TuliproxErrorKind::Info, "Can't create decrypt cipher".to_string()))?;
    cipher.apply_keystream(&mut buf);

    String::from_utf8(buf).map_err(|_err| TuliproxError::new(TuliproxErrorKind::Info, "Can't create utf8 string from decrypted".to_string()))
}

#[cfg(test)]
mod tests {
    use crate::utils::crypto_utils::{obscure_text, deobscure_text, deobfuscate_text, obfuscate_text};
    use rand::{Rng};

    #[test]
    fn test_obscure() {
        let secret: [u8; 16] = rand::rng().random(); // Random IV (AES-CBC 16 Bytes)
        let plain = "hello world";
        let encrypted = obscure_text(&secret, plain).unwrap();
        let decrypted = deobscure_text(&secret, &encrypted).unwrap();

        assert_eq!(decrypted, plain);
    }
    #[test]
    fn test_obfuscate() {
        let secret: [u8; 16] = rand::rng().random(); // Random IV (AES-CBC 16 Bytes)
        let plain = "hello world";
        let encrypted = obfuscate_text(&secret, plain);
        let decrypted = deobfuscate_text(&secret, &encrypted.unwrap()).unwrap();

        assert_eq!(decrypted, plain);
    }
}
