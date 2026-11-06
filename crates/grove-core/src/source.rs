//! Reading source into declarations, and the identifier tools renames and
//! the graph are built on.
//!
//! Zero files (`.0`) are split into top-level declarations: functions,
//! constants, `use` lines, tests and types, each with the comments above it.
//! Any other text file is one declaration of kind `File`.

use crate::model::DeclKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub kind: DeclKind,
    pub source: String,
}

pub fn is_zero_file(path: &str) -> bool {
    path.ends_with(".0")
}

/// Split one file's bytes into declarations. Text goes through [`split`];
/// anything with a NUL byte or invalid UTF-8 is one `Binary` declaration
/// holding the bytes as base64, so it is stored, addressed, synced and
/// exported like everything else.
pub fn split_bytes(path: &str, bytes: &[u8]) -> Vec<Item> {
    match std::str::from_utf8(bytes) {
        Ok(text) if !bytes.contains(&0) => split(path, text),
        _ => vec![Item {
            name: path.to_string(),
            kind: DeclKind::Binary,
            source: encode_binary(bytes),
        }],
    }
}

/// A binary file's canonical text: base64, one final newline.
pub fn encode_binary(bytes: &[u8]) -> String {
    use base64::Engine;
    let mut s = base64::engine::general_purpose::STANDARD.encode(bytes);
    s.push('\n');
    s
}

pub fn decode_binary(source: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(source.trim_end())
        .unwrap_or_default()
}

/// Split one file into declarations.
pub fn split(path: &str, text: &str) -> Vec<Item> {
    if is_zero_file(path) {
        split_zero(text)
    } else {
        // Files that aren't Zero code are kept byte for byte: a version
        // control system never rewrites your files.
        let source = text.to_string();
        vec![Item {
            name: path.to_string(),
            kind: DeclKind::File,
            source,
        }]
    }
}

/// Split Zero source into top-level declarations.
///
/// An item starts at the first non-blank character at brace depth zero
/// (comments directly above it belong to it). It ends where its braces
/// close, or at the end of its line if it opens none (`use`, `const`).
/// Strings and `//` comments are skipped when counting braces.
pub fn split_zero(text: &str) -> Vec<Item> {
    let text = text.replace("\r\n", "\n");
    let b = text.as_bytes();
    let n = b.len();
    let mut items = vec![];
    let mut start: Option<usize> = None;
    let mut depth: i32 = 0;
    let mut saw_brace = false;
    let mut line_has_code = false;
    let mut i = 0;

    let push = |s: usize, e: usize, items: &mut Vec<Item>| {
        let raw = &text[s..e];
        let source = normalize(raw);
        if source.trim().is_empty() {
            return;
        }
        let (name, kind) = classify(&source);
        items.push(Item { name, kind, source });
    };

    while i < n {
        let c = b[i];
        if c == b'/' && i + 1 < n && b[i + 1] == b'/' {
            if start.is_none() {
                start = Some(i);
                saw_brace = false;
            }
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'\n' {
            if let Some(s) = start {
                if depth == 0 && line_has_code && !saw_brace {
                    push(s, i, &mut items);
                    start = None;
                }
            }
            line_has_code = false;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if start.is_none() {
            start = Some(i);
            saw_brace = false;
        }
        line_has_code = true;
        match c {
            b'"' => {
                i += 1;
                while i < n {
                    if b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == b'"' {
                        break;
                    }
                    i += 1;
                }
            }
            b'{' => {
                depth += 1;
                saw_brace = true;
            }
            b'}' => {
                depth -= 1;
                if depth <= 0 && saw_brace {
                    depth = 0;
                    let mut j = i + 1;
                    while j < n && b[j] != b'\n' {
                        j += 1;
                    }
                    push(start.unwrap(), j, &mut items);
                    start = None;
                    saw_brace = false;
                    line_has_code = false;
                    i = j;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if let Some(s) = start {
        push(s, n, &mut items);
    }
    items
}

/// Trailing spaces off every line, no blank lines at either end, one final
/// newline. This is the canonical text a declaration is addressed by.
pub fn normalize(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().map(str::trim_end).collect();
    let first = lines
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(lines.len());
    let last = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map(|p| p + 1)
        .unwrap_or(first);
    let mut s = lines[first..last].join("\n");
    s.push('\n');
    s
}

/// Name and kind from a declaration's first line of code.
pub fn classify(source: &str) -> (String, DeclKind) {
    let header = source
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("//"))
        .unwrap_or("");
    let h = header.strip_prefix("pub ").unwrap_or(header);
    if let Some(r) = h.strip_prefix("fn ") {
        return (ident_prefix(r), DeclKind::Fn);
    }
    if let Some(r) = h.strip_prefix("const ") {
        return (ident_prefix(r), DeclKind::Const);
    }
    if let Some(r) = h.strip_prefix("use ") {
        return (format!("use {}", r.trim()), DeclKind::Use);
    }
    if let Some(r) = h.strip_prefix("test ") {
        let label = r.trim().trim_end_matches('{').trim().trim_matches('"');
        return (format!("test {label}"), DeclKind::Test);
    }
    for kw in ["shape ", "enum ", "choice ", "type "] {
        if let Some(r) = h.strip_prefix(kw) {
            return (ident_prefix(r), DeclKind::Type);
        }
    }
    (header.to_string(), DeclKind::Other)
}

fn ident_prefix(r: &str) -> String {
    r.trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// An identifier token in a piece of text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tok {
    pub start: usize,
    pub end: usize,
    /// Preceded by a dot: a field, method or module path segment, never a
    /// reference to a top-level declaration.
    pub after_dot: bool,
}

/// Identifier tokens. In code, strings and `//` comments are skipped.
pub fn idents(text: &str, code: bool) -> Vec<Tok> {
    let b = text.as_bytes();
    let n = b.len();
    let mut out = vec![];
    let mut i = 0;
    while i < n {
        let c = b[i];
        if code && c == b'"' {
            i += 1;
            while i < n {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if b[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if code && c == b'/' && i + 1 < n && b[i + 1] == b'/' {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let s = i;
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok {
                start: s,
                end: i,
                after_dot: s > 0 && b[s - 1] == b'.',
            });
            continue;
        }
        if c.is_ascii_digit() {
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    out
}

/// Spans inside `backticks`, where prose names code.
fn backtick_spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut open: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c == '`' {
            match open {
                None => open = Some(i + 1),
                Some(s) => {
                    out.push((s, i));
                    open = None;
                }
            }
        }
    }
    out
}

/// Names a piece of text refers to. Code: every identifier not after a dot.
/// Prose: identifiers inside backticks only, so ordinary words don't count.
pub fn references(text: &str, code: bool) -> Vec<String> {
    if code {
        idents(text, true)
            .into_iter()
            .filter(|t| !t.after_dot)
            .map(|t| text[t.start..t.end].to_string())
            .collect()
    } else {
        backtick_spans(text)
            .into_iter()
            .flat_map(|(s, e)| {
                let span = &text[s..e];
                idents(span, false)
                    .into_iter()
                    .filter(|t| !t.after_dot)
                    .map(move |t| span[t.start..t.end].to_string())
            })
            .collect()
    }
}

/// Replace references to `from` with `to`, leaving strings, comments, field
/// access and (in prose) everything outside backticks alone.
pub fn rename_refs(text: &str, from: &str, to: &str, code: bool) -> String {
    let spans: Vec<(usize, usize)> = if code {
        idents(text, true)
            .into_iter()
            .filter(|t| !t.after_dot)
            .map(|t| (t.start, t.end))
            .collect()
    } else {
        backtick_spans(text)
            .into_iter()
            .flat_map(|(s, e)| {
                idents(&text[s..e], false)
                    .into_iter()
                    .filter(|t| !t.after_dot)
                    .map(move |t| (s + t.start, s + t.end))
            })
            .collect()
    };
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (s, e) in spans {
        if &text[s..e] == from {
            out.push_str(&text[last..s]);
            out.push_str(to);
            last = e;
        }
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"use std.mem

// The limit for one request.
const limit: i32 = 64

pub fn main(world: World) -> Void raises {
    let x: i32 = add(1, 2)
    check world.out.write("add { not a brace }\n")
}

fn add(l: i32, r: i32) -> i32 {
    return mul(l, r) + limit
}

test "add works" {
    expect add(1, 1) == 66
}
"#;

    #[test]
    fn splits_declarations_with_their_comments() {
        let items = split_zero(SRC);
        let names: Vec<_> = items.iter().map(|i| (i.name.as_str(), i.kind)).collect();
        assert_eq!(
            names,
            vec![
                ("use std.mem", DeclKind::Use),
                ("limit", DeclKind::Const),
                ("main", DeclKind::Fn),
                ("add", DeclKind::Fn),
                ("test add works", DeclKind::Test),
            ]
        );
        assert!(items[1].source.starts_with("// The limit"));
        assert!(items[2].source.contains("not a brace"));
    }

    #[test]
    fn references_skip_strings_comments_and_fields() {
        let refs = references("let a = add(x) // mul\n s.limit + \"sub\"", true);
        assert_eq!(refs, vec!["let", "a", "add", "x", "s"]);
        assert_eq!(references("Call `add(1)` to add.", false), vec!["add"]);
    }

    #[test]
    fn rename_touches_only_real_references() {
        let out = rename_refs("return add(x) + s.add // add\n\"add\"", "add", "sum", true);
        assert_eq!(out, "return sum(x) + s.add // add\n\"add\"");
        assert_eq!(
            rename_refs("Use `add` to add.", "add", "sum", false),
            "Use `sum` to add."
        );
    }
}
