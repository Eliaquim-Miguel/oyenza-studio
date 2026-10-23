//! Minimal RFC 3986 percent-encoding for query values.

pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(char::from_digit((b >> 4) as u32, 16).unwrap().to_ascii_uppercase());
                out.push(char::from_digit((b & 0xf) as u32, 16).unwrap().to_ascii_uppercase());
            }
        }
    }
    out
}

/// Build `base?k1=v1&k2=v2` with encoded values, skipping empty values.
pub fn with_query(base: &str, params: &[(&str, &str)]) -> String {
    let mut url = String::from(base);
    let mut first = true;
    for (k, v) in params {
        if v.is_empty() {
            continue;
        }
        url.push(if first { '?' } else { '&' });
        first = false;
        url.push_str(k);
        url.push('=');
        url.push_str(&encode(v));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_and_unicode() {
        assert_eq!(encode("a b&c"), "a%20b%26c");
        assert_eq!(encode("é"), "%C3%A9");
        assert_eq!(encode("AZaz09-._~"), "AZaz09-._~");
    }

    #[test]
    fn builds_query_skipping_empty() {
        let u = with_query("https://x/api", &[("a", "1"), ("b", ""), ("c", "x y")]);
        assert_eq!(u, "https://x/api?a=1&c=x%20y");
    }
}
