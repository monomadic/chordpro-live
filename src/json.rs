//! A small JSON reader for the reference ChordPro's structured output, so
//! `cpl conform` needs no dependency. It reads all of RFC 8259; numbers stay
//! text, since conformance compares them as the files wrote them.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            Json::Number(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            _ => &[],
        }
    }

    /// A string, a number, or an array of them, as strings.
    pub fn strings(&self) -> Vec<String> {
        match self {
            Json::Array(items) => items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect(),
            other => other.as_str().map(str::to_owned).into_iter().collect(),
        }
    }
}

pub fn parse(text: &str) -> Result<Json, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = parser.value()?;
    parser.space();
    if parser.at != parser.bytes.len() {
        return Err(parser.error("trailing text"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> String {
        format!("JSON byte {}: {message}", self.at)
    }

    fn space(&mut self) {
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.at += 1;
        }
    }

    fn eat(&mut self, literal: &str) -> bool {
        if self.bytes[self.at..].starts_with(literal.as_bytes()) {
            self.at += literal.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.space();
        match self.bytes.get(self.at) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b'-' | b'0'..=b'9') => Ok(self.number()),
            _ if self.eat("null") => Ok(Json::Null),
            _ if self.eat("true") => Ok(Json::Bool(true)),
            _ if self.eat("false") => Ok(Json::Bool(false)),
            _ => Err(self.error("expected a value")),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.at += 1;
        let mut map = BTreeMap::new();
        self.space();
        if self.eat("}") {
            return Ok(Json::Object(map));
        }
        loop {
            self.space();
            if self.bytes.get(self.at) != Some(&b'"') {
                return Err(self.error("expected a key"));
            }
            let key = self.string()?;
            self.space();
            if !self.eat(":") {
                return Err(self.error("expected ':'"));
            }
            map.insert(key, self.value()?);
            self.space();
            if self.eat("}") {
                return Ok(Json::Object(map));
            }
            if !self.eat(",") {
                return Err(self.error("expected ',' or '}'"));
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.eat("]") {
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.space();
            if self.eat("]") {
                return Ok(Json::Array(items));
            }
            if !self.eat(",") {
                return Err(self.error("expected ',' or ']'"));
            }
        }
    }

    fn number(&mut self) -> Json {
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| matches!(b, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
        {
            self.at += 1;
        }
        Json::Number(String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u32::from_str_radix(d, 16).ok())
            .ok_or_else(|| self.error("bad \\u escape"))?;
        self.at += 4;
        Ok(digits)
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let Some(&byte) = self.bytes.get(self.at) else {
                return Err(self.error("unterminated string"));
            };
            self.at += 1;
            match byte {
                b'"' => {
                    return String::from_utf8(out).map_err(|_| self.error("invalid UTF-8"));
                }
                b'\\' => {
                    let Some(&escape) = self.bytes.get(self.at) else {
                        return Err(self.error("unterminated escape"));
                    };
                    self.at += 1;
                    let c = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let high = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&high) && self.eat("\\u") {
                                let low = self.hex4()?;
                                0x10000
                                    + ((high - 0xd800) << 10)
                                    + (low.wrapping_sub(0xdc00) & 0x3ff)
                            } else {
                                high
                            };
                            char::from_u32(code).unwrap_or('\u{fffd}')
                        }
                        _ => return Err(self.error("bad escape")),
                    };
                    let mut buf = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(byte),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_values() {
        let value = parse(
            r#"{"title": "Café \"Noir\"", "n": -1.5e3, "ok": true, "none": null,
                "list": ["a", 2, [], {}], "u": "é🎸\/"}"#,
        )
        .unwrap();
        assert_eq!(
            value.get("title").and_then(Json::as_str),
            Some("Café \"Noir\"")
        );
        assert_eq!(value.get("n").and_then(Json::as_str), Some("-1.5e3"));
        assert_eq!(value.get("ok"), Some(&Json::Bool(true)));
        assert_eq!(value.get("none"), Some(&Json::Null));
        assert_eq!(value.get("list").unwrap().strings(), ["a", "2"]);
        assert_eq!(value.get("u").and_then(Json::as_str), Some("é🎸/"));
    }

    #[test]
    fn rejects_malformed_text() {
        for bad in ["", "{", "[1,]", "{\"a\" 1}", "\"open", "[1] x", "nul"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
