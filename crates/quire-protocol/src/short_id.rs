//! Compact, BPE-friendly identifiers shared by every Quire service.

use uuid::Uuid;

/// Default number of Crockford Base32 characters after the prefix.
pub const DEFAULT_SHORT_ID_LEN: usize = 12;

/// Lowercase Crockford Base32: no `i`, `l`, `o` or `u` to avoid visual and
/// tokenization ambiguity.
const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
const BITS_PER_CHAR: u32 = 5;
const BITS_PER_UUID: u32 = 128;

/// 256 common, short (3-6 char) lowercase English adjectives that tokenize cleanly in BPE.
const ADJECTIVES: [&str; 256] = [
    "able", "aged", "airy", "ajar", "akin", "alert", "alive", "allied", "alone", "amber", "ample",
    "angry", "apt", "arch", "arid", "armed", "ash", "astute", "atomic", "avid", "aware", "awed",
    "azure", "back", "bad", "baked", "bald", "bare", "base", "basic", "bent", "best", "big",
    "bitter", "black", "bland", "blank", "bleak", "blind", "blond", "blue", "blunt", "bold",
    "bony", "born", "boss", "bound", "boxy", "brash", "brave", "brief", "bright", "brisk", "broad",
    "bronze", "brown", "brutal", "bulk", "busy", "calm", "candid", "cape", "card", "caring",
    "casual", "cheap", "chief", "chill", "choice", "civil", "clean", "clear", "clever", "close",
    "cloudy", "coarse", "cold", "cool", "coral", "core", "cosy", "coy", "cozy", "crafty", "crisp",
    "cross", "cruel", "crude", "curly", "curved", "cute", "cyan", "damp", "dark", "dated", "dead",
    "deaf", "dear", "decent", "deep", "deft", "dense", "dim", "direct", "dirty", "divine", "dizzy",
    "double", "drab", "drawn", "dread", "dried", "dry", "dual", "dull", "dumb", "dusky", "dusty",
    "eager", "early", "earned", "east", "easy", "edgy", "eight", "elder", "elite", "empty",
    "ended", "entire", "equal", "even", "exact", "extra", "faded", "faint", "fair", "fake",
    "false", "fancy", "far", "fast", "fat", "fatal", "faulty", "fierce", "fifth", "fifty", "final",
    "fine", "firm", "first", "fit", "five", "fixed", "flat", "fleet", "fluid", "flush", "focal",
    "foggy", "fond", "formal", "forty", "foul", "found", "four", "frank", "free", "fresh", "front",
    "frosty", "full", "fumed", "funny", "furry", "fuzzy", "game", "gaunt", "gentle", "gilded",
    "glad", "glass", "glum", "gold", "golden", "good", "grand", "grave", "gray", "great", "green",
    "grey", "grim", "gross", "grown", "gruff", "guided", "guilty", "hale", "half", "handy",
    "happy", "hard", "hardy", "harsh", "hasty", "hazel", "head", "heavy", "hefty", "hidden",
    "high", "hoarse", "hollow", "holy", "home", "honest", "hot", "huge", "human", "humid",
    "hungry", "husky", "icy", "ideal", "idle", "ill", "inner", "intact", "iron", "ivory", "jade",
    "jaded", "jaunty", "jazzy", "joint", "jolly", "joyful", "juicy", "just", "keen", "key", "kind",
    "known", "lame", "lank", "large", "last", "late", "latter", "lawful", "lazy", "lead", "lean",
    "left",
];

/// 256 common, short (3-6 char) lowercase English nouns that tokenize cleanly in BPE.
const NOUNS: [&str; 256] = [
    "acorn", "acre", "act", "age", "agent", "aid", "aim", "air", "aisle", "album", "alder",
    "alley", "alloy", "almond", "amber", "anchor", "angel", "angle", "ankle", "ant", "anvil",
    "apex", "apple", "apron", "arc", "arch", "area", "arena", "arm", "armor", "army", "arrow",
    "art", "ash", "aspen", "asset", "atlas", "atom", "attic", "audio", "audit", "aura", "author",
    "auto", "award", "axis", "badge", "bag", "bail", "bait", "baker", "bale", "ball", "balm",
    "band", "bank", "bar", "bard", "barge", "bark", "barn", "baron", "base", "basin", "basis",
    "basket", "bass", "batch", "bath", "bay", "beach", "bead", "beak", "beam", "bean", "bear",
    "beard", "beast", "beat", "bed", "bee", "beech", "beef", "beet", "bell", "belt", "bench",
    "bend", "berry", "berth", "bevel", "bias", "bike", "bill", "bin", "birch", "bird", "birth",
    "bit", "bite", "blade", "blame", "blank", "blast", "blaze", "blend", "bless", "blimp", "blink",
    "bliss", "block", "bloom", "blot", "blow", "bluff", "board", "boat", "body", "bog", "bolt",
    "bomb", "bond", "bone", "book", "boom", "boot", "booth", "border", "boss", "bot", "bound",
    "bow", "bowl", "box", "brace", "braid", "brain", "brake", "branch", "brand", "brass", "bread",
    "break", "breed", "brick", "bride", "bridge", "brief", "brim", "brine", "brink", "brook",
    "broom", "broth", "brow", "brush", "buck", "bud", "budget", "buff", "bug", "bulb", "bulk",
    "bull", "bump", "bunch", "bunk", "buoy", "burn", "burst", "bus", "bush", "bust", "butte",
    "buyer", "cabin", "cable", "cache", "cadet", "cafe", "cage", "cake", "calf", "call", "calm",
    "camel", "camp", "canal", "canary", "candle", "candy", "cane", "canoe", "canon", "canopy",
    "cape", "cap", "card", "care", "cargo", "carp", "cart", "case", "cash", "cask", "cast",
    "castle", "cat", "catch", "cause", "cave", "cedar", "cell", "cent", "chain", "chair", "chalk",
    "champ", "chance", "change", "chant", "chaos", "chap", "chart", "chase", "chat", "check",
    "cheek", "cheer", "chef", "cherry", "chess", "chest", "chick", "chief", "child", "chill",
    "chime", "chin", "chip", "choir", "chord", "chore", "chunk", "churn", "cider", "cigar",
    "cinch", "circle", "city", "claim", "clam", "clamp", "clan", "clang", "clap",
];

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

/// Generate a BPE-friendly two-word pair (`adj_noun` or `adj-noun`) from the static wordlists.
pub fn bpe_word_pair(sep: char) -> String {
    let raw = Uuid::new_v4().as_u128();
    let adj = ADJECTIVES
        .get((raw & 0xff) as usize)
        .copied()
        .unwrap_or("able");
    let noun = NOUNS
        .get(((raw >> 8) & 0xff) as usize)
        .copied()
        .unwrap_or("acorn");
    format!("{adj}{sep}{noun}")
}

/// Generate a compact, BPE-friendly identifier (`{prefix}_{adj}_{noun}`, ~3 tokens)
/// suitable for document-scoped entities (comment threads, chart previews).
pub fn compact_id(prefix: &str) -> String {
    format!("{prefix}_{}", bpe_word_pair('_'))
}

/// Generate a prefixed identifier (`{prefix}_{adj}_{noun}_{4char}`) with ~68B entropy space
/// and ~4-5 BPE tokens, suitable for globally unique session/run/message/user IDs.
pub fn short_id(prefix: &str) -> String {
    format!("{prefix}_{}_{}", bpe_word_pair('_'), short_token(4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn wordlists_are_clean_ascii_without_duplicates() {
        let adj_set: HashSet<&str> = ADJECTIVES.into_iter().collect();
        let noun_set: HashSet<&str> = NOUNS.into_iter().collect();
        assert_eq!(adj_set.len(), 256);
        assert_eq!(noun_set.len(), 256);
        for word in ADJECTIVES.into_iter().chain(NOUNS) {
            assert!(!word.is_empty() && word.len() <= 6);
            assert!(word.bytes().all(|b| b.is_ascii_lowercase()));
        }
    }

    #[test]
    fn compact_and_short_ids_have_expected_shapes() {
        let cid = compact_id("th");
        let cparts: Vec<&str> = cid.split('_').collect();
        assert_eq!(cparts.len(), 3);
        assert_eq!(cparts[0], "th");

        let sid = short_id("sess");
        let sparts: Vec<&str> = sid.split('_').collect();
        assert_eq!(sparts.len(), 4);
        assert_eq!(sparts[0], "sess");
        assert_eq!(sparts[3].len(), 4);
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
