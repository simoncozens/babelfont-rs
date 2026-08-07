//! Fontforge UTF7 decoder
//!
//! Guess what, this isn't actual UTF-7. Fontforge uses a modified version of UTF-7.

pub fn decode_utf7(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch == '+' {
            chars.next(); // consume '+'
            if let Some(&next_ch) = chars.peek() {
                if next_ch == '-' {
                    result.push('+');
                    chars.next(); // consume '-'
                    continue;
                }
            }
            let mut b64 = String::new();
            while let Some(&b64_ch) = chars.peek() {
                if b64_ch == '-' {
                    chars.next(); // consume '-'
                    break;
                }
                b64.push(b64_ch);
                chars.next(); // consume base64 char
            }
            if !b64.is_empty() {
                // The base64 payload is UTF-16BE, not UTF-8. Decoding it as
                // UTF-8 happens to "work" for ASCII -- "This" encodes to
                // 00 54 00 68 00 69 00 73, which IS valid UTF-8 -- but every
                // character arrives preceded by a NUL.
                for unit in char::decode_utf16(decode_modified_base64(&b64)) {
                    result.push(unit.unwrap_or(char::REPLACEMENT_CHARACTER));
                }
            }
        } else {
            result.push(ch);
            chars.next(); // consume normal char
        }
    }
    result
}

// Inverse lookup table: maps ASCII char values to base64 values (0-63)
// 255 indicates an invalid base64 character
const INVERSE_LOOKUP: [u8; 256] = {
    let mut table = [255u8; 256];
    // A-Z: 0-25
    table[b'A' as usize] = 0;
    table[b'B' as usize] = 1;
    table[b'C' as usize] = 2;
    table[b'D' as usize] = 3;
    table[b'E' as usize] = 4;
    table[b'F' as usize] = 5;
    table[b'G' as usize] = 6;
    table[b'H' as usize] = 7;
    table[b'I' as usize] = 8;
    table[b'J' as usize] = 9;
    table[b'K' as usize] = 10;
    table[b'L' as usize] = 11;
    table[b'M' as usize] = 12;
    table[b'N' as usize] = 13;
    table[b'O' as usize] = 14;
    table[b'P' as usize] = 15;
    table[b'Q' as usize] = 16;
    table[b'R' as usize] = 17;
    table[b'S' as usize] = 18;
    table[b'T' as usize] = 19;
    table[b'U' as usize] = 20;
    table[b'V' as usize] = 21;
    table[b'W' as usize] = 22;
    table[b'X' as usize] = 23;
    table[b'Y' as usize] = 24;
    table[b'Z' as usize] = 25;
    // a-z: 26-51
    table[b'a' as usize] = 26;
    table[b'b' as usize] = 27;
    table[b'c' as usize] = 28;
    table[b'd' as usize] = 29;
    table[b'e' as usize] = 30;
    table[b'f' as usize] = 31;
    table[b'g' as usize] = 32;
    table[b'h' as usize] = 33;
    table[b'i' as usize] = 34;
    table[b'j' as usize] = 35;
    table[b'k' as usize] = 36;
    table[b'l' as usize] = 37;
    table[b'm' as usize] = 38;
    table[b'n' as usize] = 39;
    table[b'o' as usize] = 40;
    table[b'p' as usize] = 41;
    table[b'q' as usize] = 42;
    table[b'r' as usize] = 43;
    table[b's' as usize] = 44;
    table[b't' as usize] = 45;
    table[b'u' as usize] = 46;
    table[b'v' as usize] = 47;
    table[b'w' as usize] = 48;
    table[b'x' as usize] = 49;
    table[b'y' as usize] = 50;
    table[b'z' as usize] = 51;
    // 0-9: 52-61
    table[b'0' as usize] = 52;
    table[b'1' as usize] = 53;
    table[b'2' as usize] = 54;
    table[b'3' as usize] = 55;
    table[b'4' as usize] = 56;
    table[b'5' as usize] = 57;
    table[b'6' as usize] = 58;
    table[b'7' as usize] = 59;
    table[b'8' as usize] = 60;
    table[b'9' as usize] = 61;
    // +: 62, /: 63
    table[b'+' as usize] = 62;
    table[b'/' as usize] = 63;
    // =: treat as padding (value 0)
    table[b'=' as usize] = 0;
    table
};

/// Decode modified-base64 into the UTF-16 code units it actually carries.
///
/// This is a BIT STREAM, not a stream of 4-character groups. A run encodes a
/// whole number of 16-bit code units padded up to a multiple of 6 bits, so the
/// group count is 3, 6, 8, 11 ... and a "short" trailing group is normal
/// rather than something to discard: `+AA0A-` is four characters, 24 bits,
/// carrying one code unit (U+000D) plus 8 bits of zero padding. Reading it in
/// fixed 4-character chunks and skipping the remainder loses real characters.
///
/// Leftover bits that cannot complete a code unit are padding and are dropped.
fn decode_modified_base64(input: &str) -> Vec<u16> {
    let mut units = Vec::new();
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;

    for byte in input.bytes() {
        let value = INVERSE_LOOKUP[byte as usize];
        if value == 255 {
            continue; // not a base64 character; ignore it
        }
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 16 {
            bits -= 16;
            units.push(((acc >> bits) & 0xFFFF) as u16);
        }
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(decode_utf7("Hello"), "Hello");
        assert_eq!(decode_utf7(""), "");
    }

    #[test]
    fn plus_minus_is_a_literal_plus() {
        assert_eq!(decode_utf7("a+-b"), "a+b");
    }

    #[test]
    fn payload_is_utf16_not_utf8() {
        // "This" as UTF-16BE is 00 54 00 68 00 69 00 73, which is *valid
        // UTF-8* -- a UTF-8 decode yields "\0T\0h\0i\0s".
        let decoded = decode_utf7("+AFQAaABpAHM-");
        assert_eq!(decoded, "This");
        assert!(
            !decoded.contains('\u{0}'),
            "no NUL may survive: got {decoded:?}"
        );
    }

    #[test]
    fn short_trailing_group_is_not_discarded() {
        // Four base64 characters = 24 bits = one code unit plus 8 bits of
        // padding. Reading in fixed 4-character chunks and skipping the
        // remainder would lose these.
        assert_eq!(decode_utf7("+AA0A-"), "\r");
        // Three characters = 18 bits = one code unit plus 2 bits of padding.
        assert_eq!(decode_utf7("+AGE-"), "a");
    }

    #[test]
    fn real_corpus_strings() {
        // librefonts/corben's LangName.
        assert_eq!(decode_utf7("Corben.+AAoACgAA-This"), "Corben.\n\n\u{0}This");
        // librefonts/aguafinascript's OFL description, CR-separated.
        assert_eq!(decode_utf7("License,+AA0A-Version"), "License,\rVersion");
    }

    #[test]
    fn astral_plane_survives_as_a_surrogate_pair() {
        // U+1D11E MUSICAL SYMBOL G CLEF = D834 DD1E in UTF-16.
        assert_eq!(decode_utf7("+2DTdHg-"), "\u{1D11E}");
    }

    #[test]
    fn lone_surrogate_becomes_the_replacement_character() {
        // A high surrogate with no low surrogate must not panic.
        assert_eq!(decode_utf7("+2DQ-"), "\u{FFFD}");
    }
}
