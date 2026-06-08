pub fn short_hash(value: &str) -> String {
    let mut h1 = 0xdead_beefu32;
    let mut h2 = 0x41c6_ce57u32;
    for code_unit in value.encode_utf16() {
        let ch = code_unit as u32;
        h1 = (h1 ^ ch).wrapping_mul(2_654_435_761);
        h2 = (h2 ^ ch).wrapping_mul(1_597_334_677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507)
        ^ (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507)
        ^ (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
    format!("{}{}", to_base36(h2), to_base36(h1))
}

fn to_base36(mut value: u32) -> String {
    if value == 0 {
        return "0".to_string();
    }
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut output = Vec::new();
    while value > 0 {
        output.push(DIGITS[(value % 36) as usize] as char);
        value /= 36;
    }
    output.iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hash_is_deterministic_and_utf16_compatible() {
        assert_eq!(short_hash("hello"), "1h6qa0qrowduu");
        assert_eq!(short_hash("Hello 🙈 World"), "11begrz17n9aby");
        assert_ne!(short_hash("hello"), short_hash("world"));
    }
}
