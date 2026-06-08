pub fn sanitize_surrogates(text: &str) -> String {
    text.chars().collect()
}

pub fn sanitize_utf16_surrogates(units: &[u16]) -> String {
    let mut output = String::new();
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if (0xd800..=0xdbff).contains(&unit) {
            if let Some(next) = units.get(index + 1).copied() {
                if (0xdc00..=0xdfff).contains(&next) {
                    if let Some(ch) = char::from_u32(
                        0x10000 + (((unit as u32 - 0xd800) << 10) | (next as u32 - 0xdc00)),
                    ) {
                        output.push(ch);
                    }
                    index += 2;
                    continue;
                }
            }
            index += 1;
            continue;
        }
        if (0xdc00..=0xdfff).contains(&unit) {
            index += 1;
            continue;
        }
        if let Some(ch) = char::from_u32(unit as u32) {
            output.push(ch);
        }
        index += 1;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_unpaired_surrogates_from_utf16_units() {
        let monkey = [0xd83d, 0xde48];
        assert_eq!(
            sanitize_utf16_surrogates(&[b'H' as u16, monkey[0], monkey[1]]),
            "H🙈"
        );
        assert_eq!(
            sanitize_utf16_surrogates(&[b'a' as u16, 0xd83d, b'b' as u16, 0xde48, b'c' as u16]),
            "abc"
        );
        assert_eq!(sanitize_surrogates("Hello 🙈"), "Hello 🙈");
    }
}
