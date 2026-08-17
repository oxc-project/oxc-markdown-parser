//! Autolink scanning: `<scheme:uri>` and `<email@example.com>`.

/// Scans an autolink at `pos` (which holds `<`).
/// Returns (position past `>`, is-email).
pub fn scan(t: &str, pos: usize) -> Option<(usize, bool)> {
    let bytes = t.as_bytes();
    debug_assert_eq!(bytes[pos], b'<');
    let inner_start = pos + 1;

    if let Some(end) = scan_uri(bytes, inner_start) {
        return Some((end, false));
    }
    scan_email(bytes, inner_start).map(|end| (end, true))
}

/// `scheme:` then anything but whitespace/controls/`<`/`>`, closed by `>`.
/// Scheme: a letter then 1–31 letters/digits/`+.-`.
fn scan_uri(bytes: &[u8], start: usize) -> Option<usize> {
    let first = *bytes.get(start)?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut i = start + 1;
    while let Some(&c) = bytes.get(i) {
        if c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-') {
            i += 1;
        } else {
            break;
        }
    }
    let scheme_len = i - start;
    if !(2..=32).contains(&scheme_len) || bytes.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    while let Some(&c) = bytes.get(i) {
        match c {
            b'>' => return Some(i + 1),
            b'<' | b' ' | b'\t' | b'\n' => return None,
            c if c < 0x20 || c == 0x7F => return None,
            _ => i += 1,
        }
    }
    None
}

/// The spec's email production, closed by `>`.
fn scan_email(bytes: &[u8], start: usize) -> Option<usize> {
    let is_local = |c: u8| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                b'.' | b'!'
                    | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'/'
                    | b'='
                    | b'?'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'{'
                    | b'|'
                    | b'}'
                    | b'~'
                    | b'-'
            )
    };
    let mut i = start;
    while bytes.get(i).copied().is_some_and(is_local) {
        i += 1;
    }
    if i == start || bytes.get(i) != Some(&b'@') {
        return None;
    }
    i += 1;
    // Domain: labels of alphanumerics/hyphens (≤63, no leading/trailing hyphen),
    // dot-separated.
    loop {
        let label_start = i;
        let mut last_hyphen = false;
        while let Some(&c) = bytes.get(i) {
            if c.is_ascii_alphanumeric() {
                last_hyphen = false;
                i += 1;
            } else if c == b'-' {
                last_hyphen = true;
                i += 1;
            } else {
                break;
            }
        }
        let len = i - label_start;
        if len == 0 || len > 63 || last_hyphen || bytes[label_start] == b'-' {
            return None;
        }
        match bytes.get(i) {
            Some(b'.') => i += 1,
            Some(b'>') => return Some(i + 1),
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scan;

    #[test]
    fn uri_autolinks() {
        assert_eq!(scan("<https://x.y/a?b>", 0), Some((17, false)));
        assert_eq!(scan("x <a+b-c.d:e>", 2), Some((13, false)));
        assert_eq!(scan("<a:b c>", 0), None, "whitespace");
        assert_eq!(scan("<a>", 0), None, "no scheme");
        assert_eq!(scan("<1a:b>", 0), None, "scheme must start with a letter");
        assert_eq!(scan("<a:b", 0), None);
    }

    #[test]
    fn email_autolinks() {
        assert_eq!(scan("<a.b+c@ex-ample.com>", 0), Some((20, true)));
        assert_eq!(scan("<a@b>", 0), Some((5, true)));
        assert_eq!(scan("<a@-b.c>", 0), None, "label starts with hyphen");
        assert_eq!(scan("<a@b-.c>", 0), None, "label ends with hyphen");
        assert_eq!(scan("<@b.c>", 0), None);
    }
}
