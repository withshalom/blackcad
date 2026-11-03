//! A small JSON reader for the display list's header. Hostile input is an
//! error, never a panic: depth and size are bounded, nothing indexes blindly.

use crate::Error;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

const MAX_DEPTH: usize = 32;

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.get(key),
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
    pub fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Result<Json, Error> {
    let mut p = P { b: text.as_bytes(), i: 0 };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(bad("trailing text after the header"));
    }
    Ok(v)
}

fn bad(m: &str) -> Error {
    Error::Header(m.to_string())
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), Error> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(bad("unexpected character in the header"))
        }
    }
    fn lit(&mut self, word: &str, v: Json) -> Result<Json, Error> {
        if self.b.get(self.i..self.i + word.len()) == Some(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(bad("unexpected word in the header"))
        }
    }
    fn value(&mut self, depth: usize) -> Result<Json, Error> {
        if depth > MAX_DEPTH {
            return Err(bad("header nested too deeply"));
        }
        self.ws();
        match self.peek() {
            Some(b'n') => self.lit("null", Json::Null),
            Some(b't') => self.lit("true", Json::Bool(true)),
            Some(b'f') => self.lit("false", Json::Bool(false)),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Json::Arr(v));
                }
                loop {
                    v.push(self.value(depth + 1)?);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(v));
                        }
                        _ => return Err(bad("unterminated array in the header")),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut m = BTreeMap::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(m));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    self.eat(b':')?;
                    let v = self.value(depth + 1)?;
                    m.insert(k, v);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(m));
                        }
                        _ => return Err(bad("unterminated object in the header")),
                    }
                }
            }
            Some(c) if c == b'-' || c.is_ascii_digit() => {
                let start = self.i;
                while matches!(self.peek(), Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')) {
                    self.i += 1;
                }
                let s = std::str::from_utf8(self.b.get(start..self.i).unwrap_or_default()).unwrap_or_default();
                s.parse::<f64>().map(Json::Num).map_err(|_| bad("bad number in the header"))
            }
            _ => Err(bad("unexpected character in the header")),
        }
    }
    fn string(&mut self) -> Result<String, Error> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or_else(|| bad("unterminated string in the header"))?;
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.peek().ok_or_else(|| bad("unterminated escape in the header"))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.b.get(self.i..self.i + 4).ok_or_else(|| bad("short \\u escape"))?;
                            let n = u32::from_str_radix(std::str::from_utf8(hex).unwrap_or(""), 16)
                                .map_err(|_| bad("bad \\u escape"))?;
                            self.i += 4;
                            out.push(char::from_u32(n).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(bad("unknown escape in the header")),
                    }
                }
                _ => {
                    // A UTF-8 sequence: copy its bytes whole.
                    let start = self.i - 1;
                    let len = match c {
                        0..=0x7F => 1,
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    let seg = self.b.get(start..start + len).ok_or_else(|| bad("bad UTF-8 in the header"))?;
                    out.push_str(std::str::from_utf8(seg).map_err(|_| bad("bad UTF-8 in the header"))?);
                    self.i = start + len;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_header_shape() {
        let j = parse(r##"{"bounds":[0,-1.5,10e0,2],"styles":[{"screen":"#ffffff","px":1,"layer":"A\"B\u00e9","linetype":""}],"items":3,"skipped":{}}"##)
            .unwrap();
        assert_eq!(j.get("bounds").unwrap().as_arr().unwrap()[1].as_f64(), Some(-1.5));
        let s = &j.get("styles").unwrap().as_arr().unwrap()[0];
        assert_eq!(s.get("layer").unwrap().as_str(), Some("A\"Bé"));
        assert_eq!(j.get("items").unwrap().as_f64(), Some(3.0));
    }

    #[test]
    fn hostile_text_is_an_error() {
        for t in ["", "{", "[1,", "\"abc", "{\"a\"}", "nul", "[1 2]", "{\"a\":1,}", "\"\\u12\"", "1 2", "\"\\x\""] {
            assert!(parse(t).is_err(), "{t:?}");
        }
        let deep = "[".repeat(100);
        assert!(parse(&deep).is_err());
        let _ = parse("\"\u{e9}\u{4e2d}\u{1f600}\"").unwrap();
    }
}
