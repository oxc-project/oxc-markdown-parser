//! Parser configuration.
//!
//! MD and MDX are "the same syntax with different meanings":
//! the syntactic difference reduces to which constructs are enabled,
//! so both dialects share one parser, one AST, and one API.
//! Downstream code never branches on a dialect enum, only on which nodes actually appear.

/// Which syntax constructs the parser recognizes.
///
/// CommonMark's core (paragraphs, headings, thematic breaks, blockquotes,
/// lists, fenced code, emphasis, links, hard breaks) is always on.
/// Everything listed here is a construct that some dialect turns off or bolts on.
///
/// The default set mirrors Prettier's parser composition:
/// GFM + math + wiki-link + liquid,
/// plus container directives (`:::`), which Prettier lacks and consequently corrupts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Constructs {
    /// `<https://example.com>` / `<user@example.com>`. Syntax error in MDX.
    pub autolink: bool,
    /// 4-space indented code blocks.
    /// In MDX the indent is an ordinary paragraph (continuation) instead.
    pub code_indented: bool,
    /// HTML blocks (CommonMark types 1–7).
    /// In MDX `<` at flow start is JSX.
    pub html_flow: bool,
    /// Inline HTML tags/comments.
    /// In MDX `<` in text is JSX.
    pub html_text: bool,

    /// GFM bare-URL links (`www.example.com`).
    /// Printed verbatim.
    pub gfm_autolink_literal: bool,
    /// GFM footnotes (`[^label]`).
    pub gfm_footnote: bool,
    /// GFM strikethrough (`~~x~~`).
    pub gfm_strikethrough: bool,
    /// Whether a single `~` also opens strikethrough.
    /// GitHub and Prettier (`singleTilde: false`) require `~~`;
    /// keep this off to leave subscript conventions like `H~2~O` alone.
    pub gfm_strikethrough_single_tilde: bool,
    /// GFM tables.
    pub gfm_table: bool,
    /// GFM task list items (`- [x]`).
    pub gfm_task_list_item: bool,

    /// `$$ … $$` display math.
    pub math_flow: bool,
    /// `$ … $` inline math, printed verbatim.
    pub math_text: bool,
    /// `{% … %}` / `{{ … }}` template tags, multi-line capable.
    /// Never interpreted, kept lossless.
    pub liquid: bool,
    /// `[[target]]` wiki links, alias-less minimal form, printed verbatim.
    pub wiki_link: bool,

    /// `::: … :::` container directives.
    /// Fence lines are kept verbatim (dialects disagree on their grammar);
    /// only children are markdown.
    pub container_directive: bool,

    /// MDX `import` / `export` statements at flow start.
    pub mdx_esm: bool,
    /// MDX `{expr}` as its own flow block.
    pub mdx_expression_flow: bool,
    /// MDX `{expr}` in text.
    pub mdx_expression_text: bool,
    /// MDX JSX elements at flow start.
    pub mdx_jsx_flow: bool,
    /// MDX JSX elements in text.
    pub mdx_jsx_text: bool,
}

impl Constructs {
    /// Markdown mode: CommonMark + GFM + Prettier's extensions + directives.
    pub fn markdown() -> Self {
        Self {
            autolink: true,
            code_indented: true,
            html_flow: true,
            html_text: true,
            gfm_autolink_literal: true,
            gfm_footnote: true,
            gfm_strikethrough: true,
            gfm_strikethrough_single_tilde: false,
            gfm_table: true,
            gfm_task_list_item: true,
            math_flow: true,
            math_text: true,
            liquid: true,
            wiki_link: true,
            container_directive: true,
            mdx_esm: false,
            mdx_expression_flow: false,
            mdx_expression_text: false,
            mdx_jsx_flow: false,
            mdx_jsx_text: false,
        }
    }

    /// MDX mode: `autolink` / `code_indented` / `html_flow` / `html_text` off, the `mdx_*` family on.
    /// Unlike markdown mode, parsing can emit diagnostics (MDX has syntax errors; CommonMark does not).
    ///
    /// TODO: Not usable yet: the `mdx_*` constructs are unimplemented,
    /// and disabling `code_indented` must also lift micromark's 4-space/3-column indent limits engine-wide (currently hardcoded),
    /// so this preset misparses e.g. `    # hi`.
    pub fn mdx() -> Self {
        Self {
            autolink: false,
            code_indented: false,
            html_flow: false,
            html_text: false,
            mdx_esm: true,
            mdx_expression_flow: true,
            mdx_expression_text: true,
            mdx_jsx_flow: true,
            mdx_jsx_text: true,
            ..Self::markdown()
        }
    }
}

impl Default for Constructs {
    fn default() -> Self {
        Self::markdown()
    }
}

/// Options for [`Parser`](crate::Parser).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParserOptions {
    pub constructs: Constructs,
}

impl ParserOptions {
    /// Markdown mode (the default): [`Constructs::markdown`].
    pub fn markdown() -> Self {
        Self { constructs: Constructs::markdown() }
    }

    /// MDX mode: [`Constructs::mdx`].
    pub fn mdx() -> Self {
        Self { constructs: Constructs::mdx() }
    }
}
