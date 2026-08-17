//! Liquid template-tag delimiters: `{% … %}` and `{{ … }}`.
//! Shared by the flow (block) and text (inline) construct,
//! so the delimiter grammar exists exactly once.

/// The opening delimiter of a liquid tag: `{%` (closed by `%}`) or `{{` (closed by `}}`).
/// Returns the closer's first byte.
pub fn open(tail: &str) -> Option<u8> {
    match tail.as_bytes() {
        [b'{', b'%', ..] => Some(b'%'),
        [b'{', b'{', ..] => Some(b'}'),
        _ => None,
    }
}

/// Position past the first `<closer>}` sequence in `tail`, if any.
pub fn close(tail: &str, closer: u8) -> Option<usize> {
    tail.as_bytes().windows(2).position(|w| w == [closer, b'}']).map(|i| i + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimiters() {
        assert_eq!(open("{% t %}"), Some(b'%'));
        assert_eq!(open("{{ v }}"), Some(b'}'));
        assert_eq!(open("{ x }"), None);
        assert_eq!(close("t %} tail", b'%'), Some(4));
        assert_eq!(close("v }} }}", b'}'), Some(4));
        assert_eq!(close("t }}", b'%'), None);
    }
}
