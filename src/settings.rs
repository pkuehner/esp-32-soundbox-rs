pub fn parse_value(bytes: &[u8], max: u32) -> Option<u32> {
    if bytes.is_empty() || bytes.len() > 7 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(bytes)
        .ok()?
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_setting_boundaries_and_input() {
        assert_eq!(parse_value(b"0", 600), Some(0));
        assert_eq!(parse_value(b"600", 600), Some(600));
        assert_eq!(parse_value(b"601", 600), None);
        assert_eq!(parse_value(b"100", 100), Some(100));
        assert_eq!(parse_value(b"101", 100), None);
        for invalid in [b"".as_slice(), b"-1", b"1.0", b" 1", b"12345678", b"\xff"] {
            assert_eq!(parse_value(invalid, 600), None);
        }
    }
}
