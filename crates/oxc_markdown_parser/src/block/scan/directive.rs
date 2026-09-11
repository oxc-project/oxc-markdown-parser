//! Container-directive (`:::`) fence start.
//!
//! The opening line is `:::` (3+) then anything that reads as a name or attribute list;
//! it is kept verbatim, the grammars the dialects disagree on:
//! - micromark-extension-directive: `:::name[label]{attrs}` (no space after `:::`)
//! - VitePress / markdown-it-container: `::: name title`
//! - Pandoc: `::: {.class}`
//!
//! See AGENTS.md "Dialects".

/// Opening container-directive fence:
/// 3+ `:`s, optional spaces / tabs, then a name-like start (alphanumeric, `_`, `{` or `[`)
/// and anything to the end of the line.
/// A bare run (a closing fence) or one followed by other punctuation (`:::)`) is not an opener.
/// Returns the fence length.
#[expect(clippy::cast_possible_truncation)] // fence runs fit in-line lengths
pub fn directive_fence_open(tail: &str) -> Option<u32> {
    let run = tail.bytes().take_while(|&b| b == b':').count();
    if run < 3 {
        return None;
    }
    tail[run..]
        .trim_start_matches([' ', '\t'])
        .starts_with(|c: char| c.is_alphanumeric() || matches!(c, '_' | '{' | '['))
        .then_some(run as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_open() {
        assert_eq!(directive_fence_open(":::note"), Some(3));
        assert_eq!(directive_fence_open(":::日本"), Some(3));
        assert_eq!(directive_fence_open("::::my-note[Label]{#id .cls}  "), Some(4));
        assert_eq!(directive_fence_open("::: tip Custom Title"), Some(3), "markdown-it-container");
        assert_eq!(directive_fence_open(":::tip Custom Title"), Some(3));
        assert_eq!(directive_fence_open("::: {.class}"), Some(3), "Pandoc fenced div");
        assert_eq!(directive_fence_open(":::_x"), Some(3));
        assert_eq!(directive_fence_open("::x"), None);
        assert_eq!(directive_fence_open(":::"), None, "a closing fence is not an opener");
        assert_eq!(directive_fence_open("::: "), None);
        assert_eq!(directive_fence_open(":::)"), None, "an emoticon is not an opener");
        assert_eq!(directive_fence_open("::: -x"), None);
    }
}
