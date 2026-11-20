//! Compact, URL-friendly identifiers shared by every Quire service.

use uuid::Uuid;

/// Default number of Crockford Base32 characters after the prefix.
pub const DEFAULT_SHORT_ID_LEN: usize = 12;

/// Lowercase Crockford Base32: no `i`, `l`, `o` or `u` to avoid visual and
/// tokenization ambiguity.
const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
const BITS_PER_CHAR: u32 = 5;
const BITS_PER_UUID: u32 = 128;

/// Generate a lowercase Crockford Base32 token with `len` characters.
///
/// Each character consumes five random bits. A single UUID provides 25 full
/// characters; longer tokens transparently draw from another UUID.
pub fn short_token(len: usize) -> String {
    let mut value = Uuid::new_v4().as_u128();
    let mut available_bits = BITS_PER_UUID;
    let mut out = String::with_capacity(len);

    for _ in 0..len {
        if available_bits < BITS_PER_CHAR {
            value = Uuid::new_v4().as_u128();
            available_bits = BITS_PER_UUID;
        }
        let index = (value & 0x1f) as usize;
        let byte = ALPHABET.get(index).copied().unwrap_or(b'0');
        out.push(char::from(byte));
        value >>= BITS_PER_CHAR;
        available_bits -= BITS_PER_CHAR;
    }

    out
}

/// Generate a prefixed short identifier such as `th_7k3m9p2x4q8n`.
pub fn short_id(prefix: &str) -> String {
    format!("{prefix}_{}", short_token(DEFAULT_SHORT_ID_LEN))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn short_id_has_prefixed_twelve_character_token() {
        let id = short_id("th");
        assert_eq!(id.len(), 15);
        assert!(id.starts_with("th_"));
        assert_eq!(id.split_once('_').map(|(_, token)| token.len()), Some(12));
    }

    #[test]
    fn tokens_use_lowercase_crockford_alphabet_only() {
        for _ in 0..1_000 {
            let token = short_token(DEFAULT_SHORT_ID_LEN);
            assert_eq!(token.len(), DEFAULT_SHORT_ID_LEN);
            for ch in token.chars() {
                assert!(ALPHABET.contains(&(ch as u8)));
                assert!(!matches!(ch, 'i' | 'l' | 'o' | 'u'));
                assert!(!ch.is_ascii_uppercase());
            }
        }
        assert_eq!(short_token(26).len(), 26);
    }

    #[test]
    fn ten_thousand_ids_have_no_collisions() {
        let mut seen = HashSet::with_capacity(10_000);
        for _ in 0..10_000 {
            assert!(seen.insert(short_id("th")));
        }
    }
}
