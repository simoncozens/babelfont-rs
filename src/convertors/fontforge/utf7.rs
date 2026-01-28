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
                let decoded_bytes = base64_decode(&b64);
                if let Ok(decoded_str) = String::from_utf8(decoded_bytes) {
                    result.push_str(&decoded_str);
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

fn base64_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut v: Vec<u8> = Vec::new();

    // Process in chunks of 4 characters
    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            // Incomplete chunk, skip it
            continue;
        }

        // Look up each character's value
        let n0 = INVERSE_LOOKUP[chunk[0] as usize];
        let n1 = INVERSE_LOOKUP[chunk[1] as usize];
        let n2 = INVERSE_LOOKUP[chunk[2] as usize];
        let n3 = INVERSE_LOOKUP[chunk[3] as usize];

        // Skip chunk if any character is invalid
        if n0 == 255 || n1 == 255 || n2 == 255 || n3 == 255 {
            continue;
        }

        // Combine the 4 6-bit values into 3 8-bit values
        let n = ((n0 as u32) << 18) | ((n1 as u32) << 12) | ((n2 as u32) << 6) | (n3 as u32);

        v.push(((n >> 16) & 0xFF) as u8);
        v.push(((n >> 8) & 0xFF) as u8);
        v.push((n & 0xFF) as u8);
    }
    v
}
