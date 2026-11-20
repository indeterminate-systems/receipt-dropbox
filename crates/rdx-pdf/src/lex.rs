//! PDF objects and the tokenizer shared by the file reader and the content-stream interpreter.

use std::collections::BTreeMap;

pub(crate) type Dict = BTreeMap<Vec<u8>, Obj>;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Obj {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Name(Vec<u8>),
    Str(Vec<u8>),
    Array(Vec<Obj>),
    Dict(Dict),
    Stream(Stream),
    Ref(u32, u16),
    /// A bare keyword: a content-stream operator, `stream`, `endobj`, or a stray delimiter.
    Keyword(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Stream {
    pub dict: Dict,
    pub raw: Vec<u8>,
}

impl Obj {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Obj::Int(i) => Some(*i as f64),
            Obj::Real(r) => Some(*r),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Obj::Int(i) => Some(*i),
            Obj::Real(r) => Some(*r as i64),
            _ => None,
        }
    }

    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            Obj::Name(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Obj]> {
        match self {
            Obj::Array(a) => Some(a),
            _ => None,
        }
    }

    /// A dictionary, or a stream's dictionary.
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Obj::Dict(d) => Some(d),
            Obj::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    pub fn is_keyword(&self, k: &[u8]) -> bool {
        matches!(self, Obj::Keyword(w) if w == k)
    }
}

pub(crate) fn is_ws(b: u8) -> bool {
    matches!(b, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

pub(crate) fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Nesting deeper than this is malformed or hostile; the parser stops descending.
const MAX_DEPTH: usize = 64;

pub(crate) struct Lexer<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
}

enum Tok {
    Obj(Obj),
    ArrayStart,
    ArrayEnd,
    DictStart,
    DictEnd,
    Eof,
}

impl<'a> Lexer<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Lexer { buf, pos: 0 }
    }

    pub fn at(buf: &'a [u8], pos: usize) -> Self {
        Lexer { buf, pos }
    }

    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    pub fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if is_ws(b) {
                self.pos += 1;
            } else if b == b'%' {
                while let Some(b) = self.peek() {
                    if b == b'\n' || b == b'\r' {
                        break;
                    }
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn token(&mut self) -> Tok {
        self.skip_ws();
        let Some(b) = self.peek() else {
            return Tok::Eof;
        };
        match b {
            b'[' => {
                self.pos += 1;
                Tok::ArrayStart
            }
            b']' => {
                self.pos += 1;
                Tok::ArrayEnd
            }
            b'<' if self.buf.get(self.pos + 1) == Some(&b'<') => {
                self.pos += 2;
                Tok::DictStart
            }
            b'>' if self.buf.get(self.pos + 1) == Some(&b'>') => {
                self.pos += 2;
                Tok::DictEnd
            }
            b'<' => Tok::Obj(Obj::Str(self.hex_string())),
            b'(' => Tok::Obj(Obj::Str(self.literal_string())),
            b'/' => Tok::Obj(Obj::Name(self.name())),
            b'{' | b'}' | b')' | b'>' => {
                self.pos += 1;
                Tok::Obj(Obj::Keyword(vec![b]))
            }
            _ => {
                let start = self.pos;
                while let Some(b) = self.peek() {
                    if is_ws(b) || is_delim(b) {
                        break;
                    }
                    self.pos += 1;
                }
                if self.pos == start {
                    // Unreachable in practice (every delimiter is handled above), but never loop.
                    self.pos += 1;
                }
                Tok::Obj(regular(&self.buf[start..self.pos]))
            }
        }
    }

    fn hex_string(&mut self) -> Vec<u8> {
        self.pos += 1; // <
        let mut out = Vec::new();
        let mut high: Option<u8> = None;
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == b'>' {
                break;
            }
            let Some(v) = (b as char).to_digit(16) else {
                continue;
            };
            match high.take() {
                Some(h) => out.push(h << 4 | v as u8),
                None => high = Some(v as u8),
            }
        }
        if let Some(h) = high {
            out.push(h << 4);
        }
        out
    }

    fn literal_string(&mut self) -> Vec<u8> {
        self.pos += 1; // (
        let mut out = Vec::new();
        let mut depth = 1usize;
        while let Some(b) = self.peek() {
            self.pos += 1;
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(b);
                }
                b'\\' => {
                    let Some(e) = self.peek() else { break };
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'0'..=b'7' => {
                            let mut v = (e - b'0') as u32;
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + (d - b'0') as u32;
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        // A backslash before a line end continues the string on the next line.
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        other => out.push(other),
                    }
                }
                _ => out.push(b),
            }
        }
        out
    }

    fn name(&mut self) -> Vec<u8> {
        self.pos += 1; // /
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            if is_ws(b) || is_delim(b) {
                break;
            }
            self.pos += 1;
            if b == b'#' {
                let hex = self.buf.get(self.pos..self.pos + 2);
                if let Some(v) = hex
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    out.push(v);
                    self.pos += 2;
                    continue;
                }
            }
            out.push(b);
        }
        out
    }

    /// The next complete object, with `n g R` read as a reference. `None` only at the end of input.
    pub fn object(&mut self) -> Option<Obj> {
        self.object_at_depth(0)
    }

    fn object_at_depth(&mut self, depth: usize) -> Option<Obj> {
        match self.token() {
            Tok::Eof => None,
            Tok::Obj(Obj::Int(n)) => {
                let save = self.pos;
                if let Tok::Obj(Obj::Int(g)) = self.token()
                    && let Tok::Obj(Obj::Keyword(k)) = self.token()
                    && k == b"R"
                    && n >= 0
                    && (0..=u16::MAX as i64).contains(&g)
                {
                    return Some(Obj::Ref(n as u32, g as u16));
                }
                self.pos = save;
                Some(Obj::Int(n))
            }
            Tok::Obj(o) => Some(o),
            Tok::ArrayEnd => Some(Obj::Keyword(b"]".to_vec())),
            Tok::DictEnd => Some(Obj::Keyword(b">>".to_vec())),
            Tok::ArrayStart => {
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    match self.peek() {
                        None => break,
                        Some(b']') => {
                            self.pos += 1;
                            break;
                        }
                        _ => {}
                    }
                    if depth >= MAX_DEPTH {
                        self.pos += 1;
                        continue;
                    }
                    match self.object_at_depth(depth + 1) {
                        Some(o) if o.is_keyword(b">>") => {}
                        Some(o) => items.push(o),
                        None => break,
                    }
                }
                Some(Obj::Array(items))
            }
            Tok::DictStart => {
                let mut dict = Dict::new();
                loop {
                    self.skip_ws();
                    match self.peek() {
                        None => break,
                        Some(b'>') if self.buf.get(self.pos + 1) == Some(&b'>') => {
                            self.pos += 2;
                            break;
                        }
                        _ => {}
                    }
                    if depth >= MAX_DEPTH {
                        self.pos += 1;
                        continue;
                    }
                    let Some(key) = self.object_at_depth(depth + 1) else {
                        break;
                    };
                    let Obj::Name(key) = key else {
                        // Junk where a key should be: skip it and resynchronise on the next name.
                        continue;
                    };
                    self.skip_ws();
                    if self.buf[self.pos..].starts_with(b">>") {
                        dict.insert(key, Obj::Null);
                        continue;
                    }
                    match self.object_at_depth(depth + 1) {
                        Some(v) => {
                            dict.insert(key, v);
                        }
                        None => break,
                    }
                }
                Some(Obj::Dict(dict))
            }
        }
    }

    /// After an inline image's `ID`, skips its data up to and including `EI`.
    pub fn skip_inline_image(&mut self) {
        // One whitespace byte separates `ID` from the data.
        self.pos += 1;
        while self.pos + 2 <= self.buf.len() {
            if &self.buf[self.pos..self.pos + 2] == b"EI"
                && self.pos > 0
                && is_ws(self.buf[self.pos - 1])
                && self.buf.get(self.pos + 2).is_none_or(|b| is_ws(*b))
            {
                self.pos += 2;
                return;
            }
            self.pos += 1;
        }
        self.pos = self.buf.len();
    }
}

fn regular(word: &[u8]) -> Obj {
    match word {
        b"true" => return Obj::Bool(true),
        b"false" => return Obj::Bool(false),
        b"null" => return Obj::Null,
        _ => {}
    }
    let numeric = !word.is_empty()
        && word
            .iter()
            .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.'))
        && word.iter().any(|b| b.is_ascii_digit());
    if numeric {
        let s = std::str::from_utf8(word).unwrap_or("0");
        if !s.contains('.')
            && let Ok(i) = s.parse::<i64>()
        {
            return Obj::Int(i);
        }
        // Lenient: producers write things like `-.5`, `5.` and even `--5`.
        let cleaned: String = {
            let neg = s.starts_with('-');
            let body = s.trim_start_matches(['+', '-']);
            let body = if body.starts_with('.') {
                format!("0{body}")
            } else {
                body.to_string()
            };
            let body = body.trim_end_matches('.');
            let body = body.replace(['+', '-'], "");
            if neg { format!("-{body}") } else { body }
        };
        return Obj::Real(cleaned.parse().unwrap_or(0.0));
    }
    Obj::Keyword(word.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Obj {
        Lexer::new(s.as_bytes()).object().unwrap()
    }

    #[test]
    fn numbers_names_strings_and_refs() {
        assert_eq!(parse("42"), Obj::Int(42));
        assert_eq!(parse("-.5"), Obj::Real(-0.5));
        assert_eq!(parse("/A#20B"), Obj::Name(b"A B".to_vec()));
        assert_eq!(parse("(a\\(b\\)\\101)"), Obj::Str(b"a(b)A".to_vec()));
        assert_eq!(
            parse("(nested (parens))"),
            Obj::Str(b"nested (parens)".to_vec())
        );
        assert_eq!(parse("<48 65 6c6c 6f7>"), Obj::Str(b"Hello\x70".to_vec()));
        assert_eq!(parse("12 0 R"), Obj::Ref(12, 0));
        assert_eq!(
            parse("[1 2 0 R 3]"),
            Obj::Array(vec![Obj::Int(1), Obj::Ref(2, 0), Obj::Int(3)])
        );
    }

    #[test]
    fn dictionaries_nest_and_skip_comments() {
        let d = parse("<< /Type /Page % a comment\n /Kids [ 1 0 R ] /Sub << /X true >> >>");
        let d = d.as_dict().unwrap();
        assert_eq!(d[&b"Type"[..]], Obj::Name(b"Page".to_vec()));
        assert_eq!(d[&b"Kids"[..]], Obj::Array(vec![Obj::Ref(1, 0)]));
        assert_eq!(
            d[&b"Sub"[..]].as_dict().unwrap()[&b"X"[..]],
            Obj::Bool(true)
        );
    }

    #[test]
    fn operators_come_back_as_keywords() {
        let mut lx = Lexer::new(b"1 0 0 1 72 700 Tm (Hi) Tj");
        let mut objs = Vec::new();
        while let Some(o) = lx.object() {
            objs.push(o);
        }
        assert_eq!(objs.len(), 9);
        assert!(objs[6].is_keyword(b"Tm"));
        assert!(objs[8].is_keyword(b"Tj"));
    }

    #[test]
    fn hostile_nesting_terminates() {
        let deep = "[".repeat(10_000);
        assert!(Lexer::new(deep.as_bytes()).object().is_some());
        let unterminated = "<< /A (unterminated";
        assert!(Lexer::new(unterminated.as_bytes()).object().is_some());
    }
}
