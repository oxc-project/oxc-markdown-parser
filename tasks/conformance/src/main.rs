//! Conformance runner: the CommonMark spec.json suite (GFM constructs and
//! Prettier extensions off, exactly like micromark's own spec runs) and the
//! GFM spec.txt extension sections (Prettier extensions off — cmark-gfm has
//! none of them). GFM expectations come from cmark-gfm, so
//! `<input>` attribute order is canonicalized before comparing and the
//! remaining genuine micromark-vs-cmark-gfm differences stay pinned in the
//! snapshot.
//!
//! Usage:
//!
//! ```text
//! conformance          run all suites, regenerate the snapshots
//! conformance <N>      print input/expected/actual for CommonMark example N
//!                      (`g<N>` for a GFM example)
//! conformance --clone  fetch the suite files only, do not run
//! conformance --clean  remove the fetched suite files
//! ```
//!
//! The run fails (exit 1) when results change in either direction versus the
//! committed snapshot — a previously failing example that starts passing is
//! also a change that must be reviewed and committed.
//!
//! Suite files are pinned by URL (`SUITES`) and fetched with `curl` into
//! `tasks/conformance/repos/` on first run; a bumped pin refetches.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use cow_utils::CowUtils;

use oxc_markdown_parser::{Allocator, Constructs, Parser, ParserOptions};

/// An upstream suite file, pinned to a fixed version.
struct Suite {
    /// File name under `tasks/conformance/repos/`.
    file: &'static str,
    /// Pinned URL. Bump deliberately to ingest upstream changes.
    url: &'static str,
}

const SUITES: [Suite; 2] = [
    Suite { file: "commonmark-spec.json", url: "https://spec.commonmark.org/0.31.2/spec.json" },
    Suite {
        file: "gfm-spec.txt",
        url: "https://raw.githubusercontent.com/github/cmark-gfm/499789b49373bfa045d0e7547e5ee63444c77bca/test/spec.txt",
    },
];

fn repos_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("repos")
}

/// Fetch a suite file unless the one on disk already came from the pinned URL.
/// The URL is recorded in a `<file>.url` sidecar, so bumping the pin refetches.
fn ensure_suite(suite: &Suite) -> io::Result<bool> {
    let dir = repos_dir();
    let path = dir.join(suite.file);
    let url_path = dir.join(format!("{}.url", suite.file));
    if path.is_file() && std::fs::read_to_string(&url_path).is_ok_and(|u| u == suite.url) {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir)?;
    let output = Command::new("curl").args(["-fsSL", suite.url, "-o"]).arg(&path).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "`curl {}` failed: {}",
            suite.url,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    std::fs::write(&url_path, suite.url)?;
    Ok(true)
}

struct Example {
    number: u64,
    section: String,
    markdown: String,
    html: String,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let clean = args.iter().any(|a| a == "--clean");
    let clone_only = args.iter().any(|a| a == "--clone");
    let arg = args.iter().find(|a| !a.starts_with('-'));

    if clean {
        let dir = repos_dir();
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => println!("removed {}", dir.display()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => println!("nothing to remove"),
            Err(e) => eprintln!("failed to remove {}: {e}", dir.display()),
        }
        return;
    }

    let mut fetch_failed = false;
    for suite in &SUITES {
        print!("{:<22} ", suite.file);
        io::stdout().flush().ok();
        match ensure_suite(suite) {
            Ok(true) => println!("fetched"),
            Ok(false) => println!("up-to-date"),
            Err(e) => {
                println!("ERROR: {e}");
                fetch_failed = true;
            }
        }
    }
    if fetch_failed {
        eprintln!("\none or more fetches failed (network?); re-run to retry.");
        std::process::exit(1);
    }
    if clone_only {
        return;
    }

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |suite: &Suite| std::fs::read_to_string(repos_dir().join(suite.file)).unwrap();
    let examples = parse_spec(&read(&SUITES[0]));
    let gfm_examples = parse_spec_txt(&read(&SUITES[1]));

    if let Some(arg) = arg {
        // Accept the snapshot's `#N` spelling as well as bare `N`.
        let arg = arg.trim_start_matches('#');
        if let Some(number) =
            arg.strip_prefix('g').and_then(|n| n.trim_start_matches('#').parse::<u64>().ok())
        {
            let example =
                gfm_examples.iter().find(|e| e.number == number).expect("unknown GFM example");
            debug_example(example, gfm_options(), canonicalize_gfm);
            return;
        }
        if let Ok(number) = arg.parse::<u64>() {
            let example = examples.iter().find(|e| e.number == number).expect("unknown example");
            debug_example(example, commonmark_options(), as_is);
            return;
        }
    }

    let snap_dir = manifest.join("snapshots");
    std::fs::create_dir_all(&snap_dir).unwrap();
    let mut failed = false;
    failed |= run_suite(
        "commonmark spec",
        &examples,
        commonmark_options(),
        &snap_dir.join("commonmark.snap"),
        as_is,
    );
    failed |= run_suite(
        "gfm spec (extensions)",
        &gfm_examples,
        gfm_options(),
        &snap_dir.join("gfm.snap"),
        canonicalize_gfm,
    );
    if failed {
        std::process::exit(1);
    }
}

fn debug_example(example: &Example, options: ParserOptions, normalize: fn(&str) -> Cow<'_, str>) {
    let allocator = Allocator::default();
    let actual = run_with(&allocator, &example.markdown, options);
    println!("=== markdown ===\n{}", example.markdown);
    println!("=== expected ===\n{}", example.html);
    println!("=== actual ===\n{actual}");
    let ok = normalize(&actual) == normalize(&example.html);
    println!("=== {} ===", if ok { "PASS" } else { "FAIL" });
}

/// Runs one suite, rewrites its snapshot, and reports deltas. Returns
/// whether the run should fail (bidirectional pin: any delta versus the
/// committed snapshot, including newly passing examples).
fn run_suite(
    title: &str,
    examples: &[Example],
    options: ParserOptions,
    snap_path: &Path,
    normalize: fn(&str) -> Cow<'_, str>,
) -> bool {
    // Section order as encountered (sections are contiguous):
    // (name, passed count, failed example numbers).
    let mut sections: Vec<(String, usize, Vec<u64>)> = Vec::new();
    let mut allocator = Allocator::default();
    for example in examples {
        let actual = run_with(&allocator, &example.markdown, options);
        allocator.reset();
        if sections.last().is_none_or(|(name, ..)| *name != example.section) {
            sections.push((example.section.clone(), 0, Vec::new()));
        }
        let entry = sections.last_mut().unwrap();
        if normalize(&actual) == normalize(&example.html) {
            entry.1 += 1;
        } else {
            entry.2.push(example.number);
        }
    }

    // One format for the snapshot and stdout: suite total, indented
    // sections, failed examples as `#N` (the published spec's example
    // anchors, so they read as references rather than counts).
    let total = examples.len();
    let passed: usize = sections.iter().map(|(_, p, _)| p).sum();
    let mut snap = String::new();
    let _ = writeln!(snap, "{title}: {passed}/{total}");
    for (name, pass, fail) in &sections {
        let _ = write!(snap, "  {name}: {pass}/{}", pass + fail.len());
        for (i, n) in fail.iter().enumerate() {
            let _ = write!(snap, "{}#{n}", if i == 0 { " (failed: " } else { ", " });
        }
        if !fail.is_empty() {
            snap.push(')');
        }
        snap.push('\n');
    }
    let old = std::fs::read_to_string(snap_path).ok();
    std::fs::write(snap_path, &snap).unwrap();
    println!("{snap}");

    let Some(old) = old else { return false };
    // The old side genuinely is text; the new side comes from `sections`
    // directly, so a snapshot format change cannot blind the delta check.
    let old_failed = failed_set(&old);
    let new_failed: BTreeSet<u64> =
        sections.iter().flat_map(|(_, _, f)| f.iter().copied()).collect();
    let newly_failing: Vec<u64> = new_failed.difference(&old_failed).copied().collect();
    let newly_passing: Vec<u64> = old_failed.difference(&new_failed).copied().collect();
    if !newly_failing.is_empty() {
        println!("REGRESSED: {newly_failing:?}");
    }
    if !newly_passing.is_empty() {
        println!("NEWLY PASSING (review & commit the snapshot): {newly_passing:?}");
    }
    !newly_failing.is_empty() || !newly_passing.is_empty()
}

/// The identity normalizer, for suites whose oracle needs no canonicalizing.
fn as_is(s: &str) -> Cow<'_, str> {
    Cow::Borrowed(s)
}

/// cmark-gfm (the GFM spec's oracle) and micromark (ours) emit `<input>`
/// checkboxes with different attribute orders and self-closing styles;
/// canonicalize those so only real parse differences remain.
fn canonicalize_gfm(html: &str) -> Cow<'_, str> {
    if !html.contains("<input ") {
        return Cow::Borrowed(html);
    }
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("<input ") {
        let Some(close) = rest[at..].find('>') else { break };
        out.push_str(&rest[..at]);
        let tag = &rest[at + "<input ".len()..at + close];
        let mut attrs: Vec<&str> = tag.trim_end_matches('/').split_whitespace().collect();
        attrs.sort_unstable();
        out.push_str("<input ");
        out.push_str(&attrs.join(" "));
        out.push('>');
        rest = &rest[at + close + 1..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Parses the extension examples out of cmark-gfm's `spec.txt`. Examples
/// are numbered over the whole file (so numbers match the published spec);
/// only the extension sections are kept, minus "Disallowed Raw HTML"
/// (an HTML output filter, not parsing).
fn parse_spec_txt(text: &str) -> Vec<Example> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut number = 0u64;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some(heading) = line.strip_prefix("## ") {
            section = heading.to_string();
            continue;
        }
        if !(line.starts_with("````") && line.contains(" example")) {
            continue;
        }
        number += 1;
        let mut markdown = String::new();
        let mut html = String::new();
        let mut in_html = false;
        for body in lines.by_ref() {
            if body.starts_with("````") {
                break;
            }
            if body == "." {
                in_html = true;
                continue;
            }
            let target = if in_html { &mut html } else { &mut markdown };
            target.push_str(&body.cow_replace('→', "\t"));
            target.push('\n');
        }
        if section.contains("(extension)") && !section.contains("Disallowed") {
            out.push(Example { number, section: section.clone(), markdown, html });
        }
    }
    out
}

fn failed_set(snap: &str) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    for line in snap.lines() {
        if let Some((_, list)) = line.split_once("(failed: ") {
            for n in list.trim_end_matches(')').split(", ") {
                // The trim also accepts pre-`#N`-format snapshots.
                if let Ok(n) = n.trim_start_matches('#').parse() {
                    out.insert(n);
                }
            }
        }
    }
    out
}

/// The CommonMark suite expects plain CommonMark: the GFM constructs and the
/// Prettier extensions (on by default, matching the Prettier composition)
/// would supersede some of its expected outputs, exactly as they do in
/// micromark.
fn commonmark_options() -> ParserOptions {
    ParserOptions {
        constructs: Constructs {
            gfm_autolink_literal: false,
            gfm_footnote: false,
            gfm_strikethrough: false,
            gfm_table: false,
            gfm_task_list_item: false,
            ..gfm_options().constructs
        },
    }
}

/// The GFM suite expects GFM without the Prettier extensions (its oracle is
/// cmark-gfm, which has no math/liquid/wiki-link/directive).
fn gfm_options() -> ParserOptions {
    ParserOptions {
        constructs: Constructs {
            math_flow: false,
            math_text: false,
            liquid: false,
            wiki_link: false,
            container_directive: false,
            ..Constructs::markdown()
        },
    }
}

fn run_with(allocator: &Allocator, markdown: &str, options: ParserOptions) -> String {
    let ret = Parser::with_options(allocator, markdown, options).parse();
    render::render(markdown, &ret.root)
}

fn parse_spec(json: &str) -> Vec<Example> {
    let value: serde_json::Value = serde_json::from_str(json).expect("invalid spec.json");
    value
        .as_array()
        .expect("spec.json is an array")
        .iter()
        .map(|e| Example {
            number: e["example"].as_u64().unwrap(),
            section: e["section"].as_str().unwrap().to_string(),
            markdown: e["markdown"].as_str().unwrap().to_string(),
            html: e["html"].as_str().unwrap().to_string(),
        })
        .collect()
}
