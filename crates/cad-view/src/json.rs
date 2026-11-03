//! A small JSON reader, for the display list's header and the session's
//! replies (`snap`, `pick`). Standard library only; no input reaches a
//! panic: nesting is capped and every access is checked.

/// A parsed JSON value. Objects keep their members in file order.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// Arrays and objects nested deeper than this are refused.
pub const MAX_DEPTH: usize = 64;

impl Json {
    /// Parse one JSON value; only whitespace may follow it.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser { text, i: 0 };
        p.ws();
        let v = p.value(0)?;
        p.ws();
        if p.i != text.len() {
            return Err(p.err("text after the value"));
        }
        Ok(v)
    }

    /// The member `key` of an object (the first, if it repeats).
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
}

struct Parser<'a> {
    text: &'a str,
    i: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("JSON: {what} at byte {}", self.i)
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self, w: &str, v: Json) -> Result<Json, String> {
        if self.text.get(self.i..).is_some_and(|r| r.starts_with(w)) {
            self.i += w.len();
            Ok(v)
        } else {
            Err(self.err("unexpected text"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("unexpected character")),
            None => Err(self.err("the text ends early")),
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        while matches!(
            self.peek(),
            Some(b'+' | b'-' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.i += 1;
        }
        let s = self.text.get(start..self.i).unwrap_or("");
        match s.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(Json::Num(n)),
            _ => Err(self.err("not a number")),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // the opening quote
        let mut out = String::new();
        loop {
            let start = self.i;
            while !matches!(self.peek(), Some(b'"' | b'\\') | None) {
                self.i += 1;
            }
            // Runs end at ASCII bytes, so they are whole characters.
            out.push_str(
                self.text
                    .get(start..self.i)
                    .ok_or_else(|| self.err("bad text"))?,
            );
            match self.peek() {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let c = self.peek().ok_or_else(|| self.err("the text ends early"))?;
                    self.i += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode()?),
                        _ => return Err(self.err("bad escape")),
                    }
                }
                _ => return Err(self.err("the string is not closed")),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let s = self
            .text
            .get(self.i..self.i + 4)
            .ok_or_else(|| self.err("short \\u escape"))?;
        let v = u32::from_str_radix(s, 16).map_err(|_| self.err("bad \\u escape"))?;
        self.i += 4;
        Ok(v)
    }

    /// After `\u`: one character, joining a surrogate pair; a lone surrogate
    /// becomes U+FFFD.
    fn unicode(&mut self) -> Result<char, String> {
        let hi = self.hex4()?;
        if (0xD800..0xDC00).contains(&hi)
            && self
                .text
                .get(self.i..)
                .is_some_and(|r| r.starts_with("\\u"))
        {
            let save = self.i;
            self.i += 2;
            let lo = self.hex4()?;
            if (0xDC00..0xE000).contains(&lo) {
                let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                return Ok(char::from_u32(c).unwrap_or('\u{FFFD}'));
            }
            self.i = save;
        }
        Ok(char::from_u32(hi).unwrap_or('\u{FFFD}'))
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        if depth >= MAX_DEPTH {
            return Err(self.err("nested too deeply"));
        }
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.eat(b']') {
            return Ok(Json::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value(depth + 1)?);
            self.ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b']') {
                return Ok(Json::Arr(items));
            }
            return Err(self.err("expected , or ]"));
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        if depth >= MAX_DEPTH {
            return Err(self.err("nested too deeply"));
        }
        self.i += 1;
        let mut members = Vec::new();
        self.ws();
        if self.eat(b'}') {
            return Ok(Json::Obj(members));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a member name"));
            }
            let k = self.string()?;
            self.ws();
            if !self.eat(b':') {
                return Err(self.err("expected :"));
            }
            self.ws();
            members.push((k, self.value(depth + 1)?));
            self.ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                return Ok(Json::Obj(members));
            }
            return Err(self.err("expected , or }"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_kinds_of_value() {
        let j = Json::parse(r#" {"a":[1,-2.5e1,true,null],"s":"x\nyé😀","o":{}} "#).unwrap();
        let a = j.get("a").unwrap().as_array().unwrap();
        assert_eq!(a[0].as_f64(), Some(1.0));
        assert_eq!(a[1].as_f64(), Some(-25.0));
        assert_eq!(a[2], Json::Bool(true));
        assert!(a[3].is_null());
        assert_eq!(j.get("s").unwrap().as_str(), Some("x\nyé😀"));
        assert_eq!(j.get("o"), Some(&Json::Obj(vec![])));
        assert_eq!(j.get("missing"), None);
    }

    #[test]
    fn a_lone_surrogate_is_replaced() {
        let j = Json::parse(r#""\ud83dx""#).unwrap();
        assert_eq!(j.as_str(), Some("\u{FFFD}x"));
    }

    #[test]
    fn refuses_what_is_not_json() {
        for bad in [
            "",
            "{",
            "[1,",
            r#"{"a"}"#,
            r#"{"a":1,}"#,
            "[1 2]",
            "tru",
            "1e999",
            r#""abc"#,
            r#""\q""#,
            "{} x",
            r#""\u12""#,
            "-",
            "[,]",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn deep_nesting_is_refused_not_overflowed() {
        let deep = "[".repeat(10_000);
        assert!(Json::parse(&deep).is_err());
        let ok = format!("{}1{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(Json::parse(&ok).is_ok());
    }

    #[test]
    fn every_prefix_of_a_document_is_handled() {
        let doc = r#"{"snaps":[{"kind":2,"name":"endpoint","x":1.5,"y":-2,"handle":"2F"}]}"#;
        for n in 0..doc.len() {
            if let Some(p) = doc.get(..n) {
                let _ = Json::parse(p);
            }
        }
        assert!(Json::parse(doc).is_ok());
    }
}
