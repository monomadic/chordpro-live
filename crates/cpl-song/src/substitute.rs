//! `%{…}` metadata substitution, as the reference ChordPro applies it to
//! metadata values, comments, labels, lyrics and annotations:
//!
//! - `%{name}` is the item's value, several values joined by `"; "`;
//!   `%{name.2}` picks the second and `%{name.-1}` the last.
//! - `%{name|yes|no}` and `%{name|yes}` choose by whether the item has a
//!   non-empty value; `%{name=value|yes|no}` by whether it equals `value`.
//! - Inside a branch, `%{}` is the controlling item's value and `\|`, `\{`,
//!   `\}`, `\\` are literal.
//!
//! An unknown item is empty. Anything that is not well-formed (a space in
//! the name, no closing brace) and anything escaped with `\` outside a
//! branch stays as written.

use std::collections::BTreeMap;

pub type Metadata = BTreeMap<String, Vec<String>>;

/// `text` with every `%{…}` replaced from `meta`.
pub fn substitute(text: &str, meta: &Metadata) -> String {
    if !text.contains("%{") {
        return text.to_owned();
    }
    expand(text, meta, None)
}

fn expand(text: &str, meta: &Metadata, current: Option<&str>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['\\', '%']) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with('\\') {
            // An escape outside a branch is kept, with what it escapes.
            let width = rest[1..].chars().next().map_or(0, char::len_utf8);
            out.push_str(&rest[..1 + width]);
            rest = &rest[1 + width..];
            continue;
        }
        let Some(body) = rest.strip_prefix("%{") else {
            out.push('%');
            rest = &rest[1..];
            continue;
        };
        match closing(body).and_then(|end| Some((end, replace(&body[..end], meta, current)?))) {
            Some((end, replaced)) => {
                out.push_str(&replaced);
                rest = &body[end + 1..];
            }
            None => {
                out.push_str("%{");
                rest = body;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The byte offset of the `}` that closes a `%{`, past nested ones and
/// escapes.
fn closing(body: &str) -> Option<usize> {
    let mut depth = 0;
    let mut chars = body.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '%' if chars.peek().is_some_and(|&(_, next)| next == '{') => {
                chars.next();
                depth += 1;
            }
            '}' if depth == 0 => return Some(at),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Splits at the `|`s that are neither escaped nor inside a nested `%{…}`.
fn branches(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    let mut chars = body.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '%' if chars.peek().is_some_and(|&(_, next)| next == '{') => {
                chars.next();
                depth += 1;
            }
            '}' if depth > 0 => depth -= 1,
            '|' if depth == 0 => {
                parts.push(&body[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

/// What one `%{body}` becomes, or `None` when it is not well-formed.
fn replace(body: &str, meta: &Metadata, current: Option<&str>) -> Option<String> {
    let parts = branches(body);
    let (name, test) = match parts[0].split_once('=') {
        Some((name, test)) => (name, Some(test)),
        None => (parts[0], None),
    };
    let value = if name.is_empty() {
        current.unwrap_or("").to_owned()
    } else {
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return None;
        }
        lookup(name, meta)
    };
    if parts.len() == 1 && test.is_none() {
        return Some(value);
    }
    let chosen = match test {
        Some(test) => value == test,
        None => !value.is_empty(),
    };
    let branch = if chosen { parts.get(1) } else { parts.get(2) };
    Some(unescape(&expand(
        branch.copied().unwrap_or(""),
        meta,
        Some(&value),
    )))
}

fn lookup(name: &str, meta: &Metadata) -> String {
    let (item, index) = match name.rsplit_once('.') {
        Some((item, index)) => match index.parse::<i64>() {
            Ok(index) => (item, Some(index)),
            Err(_) => (name, None),
        },
        None => (name, None),
    };
    let Some(values) = meta.get(item) else {
        return String::new();
    };
    match index {
        None => values.join("; "),
        Some(index) => {
            let at = if index < 0 {
                values.len() as i64 + index
            } else {
                index - 1
            };
            usize::try_from(at)
                .ok()
                .and_then(|at| values.get(at))
                .cloned()
                .unwrap_or_default()
        }
    }
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(next @ ('|' | '{' | '}' | '\\')) => out.push(next),
                Some(next) => {
                    out.push('\\');
                    out.push(next);
                }
                None => out.push('\\'),
            },
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(items: &[(&str, &[&str])]) -> Metadata {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    /// Each expectation is what chordpro 6.101.0 printed for the same text.
    #[test]
    fn matches_the_reference() {
        let m = meta(&[
            ("title", &["Title"]),
            ("artist", &["Ada"]),
            ("album", &["Yes"]),
            ("tag", &["x", "y"]),
            ("year", &["0"]),
        ]);
        for (text, expected) in [
            ("%{title}", "Title"),
            ("%{album|Album: %{}|none}", "Album: Yes"),
            ("%{key|K|noK}", "noK"),
            ("%{artist=Ada|isAda|notAda}", "isAda"),
            ("%{artist=Bo|isBo|notBo}", "notBo"),
            ("%{key=|E|NE}", "E"),
            ("%{artist=|E|NE}", "NE"),
            ("%{tag}", "x; y"),
            ("%{tag.1} %{tag.2} %{tag.-1} (%{tag.3})", "x y y ()"),
            ("%{album.-1}", "Yes"),
            ("%{year|Y|noY}", "Y"),
            ("(%{artist|yes}) (%{key|yes})", "(yes) ()"),
            ("%{ artist }", "%{ artist }"),
            ("%{artist|a\\|b|c}", "a|b"),
            ("\\%{title} a\\|b", "\\%{title} a\\|b"),
            ("%{TITLE}%{nothing}x", "x"),
            ("50% off %{title", "50% off %{title"),
            ("%{title|%{}|}", "Title"),
            ("no substitution", "no substitution"),
        ] {
            assert_eq!(substitute(text, &m), expected, "{text}");
        }
    }
}
