//! Fonts, as far as text extraction needs them: how bytes split into character codes, what
//! Unicode text each code stands for, and how wide each glyph is.

use std::collections::HashMap;

use crate::file::Doc;
use crate::lex::{Dict, Lexer, Obj};

pub(crate) struct Font {
    /// Composite (Type0) fonts use multi-byte codes; simple fonts one byte per code.
    code_lengths: CodeLengths,
    to_unicode: HashMap<u32, String>,
    /// For simple fonts without a ToUnicode entry for a code.
    encoding: Option<[Option<char>; 256]>,
    widths: Widths,
    /// Glyph space to text space (1/1000 except for Type3 fonts).
    scale: f64,
}

enum CodeLengths {
    One,
    Two,
    /// From a CMap's `codespacerange`: (byte length, low, high) per range.
    Ranges(Vec<(usize, u32, u32)>),
}

enum Widths {
    Simple {
        first: u32,
        widths: Vec<f64>,
    },
    Cid {
        widths: HashMap<u32, f64>,
        default: f64,
    },
    Standard(&'static [u16; 95]),
    Mono,
}

impl Font {
    pub fn load(doc: &Doc, dict: &Dict) -> Font {
        let subtype = doc.dict_get(dict, b"Subtype").as_name().unwrap_or(b"");
        let to_unicode_cmap = match doc.dict_get(dict, b"ToUnicode") {
            Obj::Stream(s) => doc.decode(s).ok().map(|data| CMap::parse(&data)),
            _ => None,
        };
        let (to_unicode, ranges) = match to_unicode_cmap {
            Some(c) => (c.map, c.ranges),
            None => (HashMap::new(), Vec::new()),
        };
        if subtype == b"Type0" {
            let descendant = doc
                .dict_get(dict, b"DescendantFonts")
                .as_array()
                .and_then(|a| a.first())
                .map(|o| doc.resolve(o))
                .and_then(Obj::as_dict);
            let mut widths = HashMap::new();
            let mut default = 1000.0;
            if let Some(d) = descendant {
                default = doc.dict_get(d, b"DW").as_f64().unwrap_or(1000.0);
                if let Some(w) = doc.dict_get(d, b"W").as_array() {
                    cid_widths(doc, w, &mut widths);
                }
            }
            // An embedded encoding CMap says how codes split; Identity-H/V are two bytes.
            let code_lengths = match doc.dict_get(dict, b"Encoding") {
                Obj::Stream(s) => {
                    let c = doc.decode(s).map(|d| CMap::parse(&d)).ok();
                    match c {
                        Some(c) if !c.ranges.is_empty() => CodeLengths::Ranges(c.ranges),
                        _ => CodeLengths::Two,
                    }
                }
                _ if !ranges.is_empty() && ranges.iter().all(|r| r.0 == 2) => CodeLengths::Two,
                _ => CodeLengths::Two,
            };
            return Font {
                code_lengths,
                to_unicode,
                encoding: None,
                widths: Widths::Cid { widths, default },
                scale: 0.001,
            };
        }

        let base_font = doc.dict_get(dict, b"BaseFont").as_name().unwrap_or(b"");
        let first = doc
            .dict_get(dict, b"FirstChar")
            .as_i64()
            .unwrap_or(0)
            .max(0) as u32;
        let widths = match doc.dict_get(dict, b"Widths").as_array() {
            Some(w) if !w.is_empty() => Widths::Simple {
                first,
                widths: w
                    .iter()
                    .map(|o| doc.resolve(o).as_f64().unwrap_or(0.0))
                    .collect(),
            },
            _ if contains(base_font, b"Courier") || contains(base_font, b"Mono") => Widths::Mono,
            _ => Widths::Standard(&HELVETICA),
        };
        let scale = if subtype == b"Type3" {
            doc.dict_get(dict, b"FontMatrix")
                .as_array()
                .and_then(|m| m.first())
                .and_then(Obj::as_f64)
                .unwrap_or(0.001)
        } else {
            0.001
        };
        Font {
            code_lengths: CodeLengths::One,
            to_unicode,
            encoding: Some(simple_encoding(doc, dict)),
            widths,
            scale,
        }
    }

    /// Splits a shown string into character codes.
    pub fn codes(&self, bytes: &[u8]) -> Vec<(u32, usize)> {
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let len = match &self.code_lengths {
                CodeLengths::One => 1,
                CodeLengths::Two => 2,
                CodeLengths::Ranges(ranges) => ranges
                    .iter()
                    .find(|(len, lo, hi)| {
                        i + len <= bytes.len() && {
                            let c = be(&bytes[i..i + len]);
                            (*lo..=*hi).contains(&c)
                        }
                    })
                    .map(|r| r.0)
                    .unwrap_or(1),
            };
            let len = len.min(bytes.len() - i);
            out.push((be(&bytes[i..i + len]), len));
            i += len;
        }
        out
    }

    pub fn text(&self, code: u32) -> String {
        if let Some(s) = self.to_unicode.get(&code) {
            return s.clone();
        }
        if let Some(enc) = &self.encoding
            && let Some(Some(c)) = enc.get(code as usize)
        {
            return c.to_string();
        }
        '\u{FFFD}'.to_string()
    }

    /// Advance width in text space units (multiply by the font size).
    pub fn width(&self, code: u32) -> f64 {
        let w = match &self.widths {
            Widths::Simple { first, widths } => code
                .checked_sub(*first)
                .and_then(|i| widths.get(i as usize))
                .copied()
                .unwrap_or(0.0),
            Widths::Cid { widths, default } => widths.get(&code).copied().unwrap_or(*default),
            Widths::Standard(table) => match code {
                32..=126 => table[(code - 32) as usize] as f64,
                _ => 556.0,
            },
            Widths::Mono => 600.0,
        };
        w * self.scale
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn be(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |acc, b| (acc << 8) | *b as u32)
}

fn cid_widths(doc: &Doc, w: &[Obj], out: &mut HashMap<u32, f64>) {
    let items: Vec<&Obj> = w.iter().map(|o| doc.resolve(o)).collect();
    let mut i = 0;
    while i < items.len() {
        let Some(first) = items[i].as_i64() else {
            i += 1;
            continue;
        };
        match items.get(i + 1) {
            Some(Obj::Array(ws)) => {
                for (k, wv) in ws.iter().enumerate() {
                    if let Some(v) = doc.resolve(wv).as_f64() {
                        out.insert(first as u32 + k as u32, v);
                    }
                }
                i += 2;
            }
            Some(last) => {
                let (Some(last), Some(v)) =
                    (last.as_i64(), items.get(i + 2).and_then(|o| o.as_f64()))
                else {
                    break;
                };
                // Guard against absurd ranges in hostile files.
                for c in first..=last.min(first + 65_535) {
                    out.insert(c as u32, v);
                }
                i += 3;
            }
            None => break,
        }
    }
}

fn simple_encoding(doc: &Doc, dict: &Dict) -> [Option<char>; 256] {
    let mut table = win_ansi();
    let enc = doc.dict_get(dict, b"Encoding");
    // MacRoman differs from WinAnsi only above 127; statements rarely use it, so those codes
    // fall back to WinAnsi (add the table if a real layout needs it).
    if let Obj::Dict(d) = enc
        && let Some(diffs) = doc.dict_get(d, b"Differences").as_array()
    {
        let mut code = 0usize;
        for item in diffs {
            match doc.resolve(item) {
                Obj::Int(n) => code = (*n).clamp(0, 255) as usize,
                Obj::Name(name) => {
                    if code < 256 {
                        table[code] = glyph_name(name);
                    }
                    code += 1;
                }
                _ => {}
            }
        }
    }
    table
}

fn win_ansi() -> [Option<char>; 256] {
    let mut t = [None; 256];
    for (i, slot) in t.iter_mut().enumerate() {
        *slot = match i {
            0x20..=0x7e | 0xa0..=0xff => char::from_u32(i as u32),
            _ => None,
        };
    }
    const HIGH: [(usize, char); 27] = [
        (0x80, '€'),
        (0x82, '‚'),
        (0x83, 'ƒ'),
        (0x84, '„'),
        (0x85, '…'),
        (0x86, '†'),
        (0x87, '‡'),
        (0x88, 'ˆ'),
        (0x89, '‰'),
        (0x8a, 'Š'),
        (0x8b, '‹'),
        (0x8c, 'Œ'),
        (0x8e, 'Ž'),
        (0x91, '‘'),
        (0x92, '’'),
        (0x93, '“'),
        (0x94, '”'),
        (0x95, '•'),
        (0x96, '–'),
        (0x97, '—'),
        (0x98, '˜'),
        (0x99, '™'),
        (0x9a, 'š'),
        (0x9b, '›'),
        (0x9c, 'œ'),
        (0x9e, 'ž'),
        (0x9f, 'Ÿ'),
    ];
    for (i, c) in HIGH {
        t[i] = Some(c);
    }
    t
}

/// Unicode for an Adobe glyph name: the names simple fonts use in `/Differences`.
fn glyph_name(name: &[u8]) -> Option<char> {
    let name = std::str::from_utf8(name).ok()?;
    // Variants like `a.sc` or `one.oldstyle` stand for the base glyph.
    let base = name.split(['.', '_']).next().unwrap_or(name);
    if base.len() == 1 {
        return base.chars().next();
    }
    if let Some(hex) = base.strip_prefix("uni").or_else(|| base.strip_prefix('u'))
        && (4..=6).contains(&hex.len())
        && let Ok(v) = u32::from_str_radix(&hex[..hex.len().min(6)], 16)
    {
        return char::from_u32(v);
    }
    let c = match base {
        "space" | "nbspace" => ' ',
        "exclam" => '!',
        "quotedbl" => '"',
        "numbersign" => '#',
        "dollar" => '$',
        "percent" => '%',
        "ampersand" => '&',
        "quotesingle" => '\'',
        "quoteright" => '’',
        "quoteleft" => '‘',
        "parenleft" => '(',
        "parenright" => ')',
        "asterisk" => '*',
        "plus" => '+',
        "comma" => ',',
        "hyphen" | "minus" => '-',
        "period" => '.',
        "slash" => '/',
        "zero" => '0',
        "one" => '1',
        "two" => '2',
        "three" => '3',
        "four" => '4',
        "five" => '5',
        "six" => '6',
        "seven" => '7',
        "eight" => '8',
        "nine" => '9',
        "colon" => ':',
        "semicolon" => ';',
        "less" => '<',
        "equal" => '=',
        "greater" => '>',
        "question" => '?',
        "at" => '@',
        "bracketleft" => '[',
        "backslash" => '\\',
        "bracketright" => ']',
        "asciicircum" => '^',
        "underscore" => '_',
        "grave" => '`',
        "braceleft" => '{',
        "bar" => '|',
        "braceright" => '}',
        "asciitilde" => '~',
        "endash" => '–',
        "emdash" => '—',
        "bullet" => '•',
        "Euro" => '€',
        "sterling" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "degree" => '°',
        "section" => '§',
        "copyright" => '©',
        "registered" => '®',
        "trademark" => '™',
        "ellipsis" => '…',
        "quotedblleft" => '“',
        "quotedblright" => '”',
        "exclamdown" => '¡',
        "questiondown" => '¿',
        "ordfeminine" => 'ª',
        "ordmasculine" => 'º',
        "aacute" => 'á',
        "eacute" => 'é',
        "iacute" => 'í',
        "oacute" => 'ó',
        "uacute" => 'ú',
        "Aacute" => 'Á',
        "Eacute" => 'É',
        "Iacute" => 'Í',
        "Oacute" => 'Ó',
        "Uacute" => 'Ú',
        "ntilde" => 'ñ',
        "Ntilde" => 'Ñ',
        "udieresis" => 'ü',
        "Udieresis" => 'Ü',
        "agrave" => 'à',
        "egrave" => 'è',
        "ccedilla" => 'ç',
        "Ccedilla" => 'Ç',
        "adieresis" => 'ä',
        "odieresis" => 'ö',
        "germandbls" => 'ß',
        "fi" => 'ﬁ',
        "fl" => 'ﬂ',
        _ => return None,
    };
    Some(c)
}

/// Helvetica's advance widths for codes 32 to 126, for standard fonts written without `/Widths`.
static HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278,
    278, // space../
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0-9
    278, 278, 584, 584, 584, 556, 1015, // :;<=>?@
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, // A-M
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // N-Z
    278, 278, 278, 469, 556, 333, // [\]^_`
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, // a-m
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, // n-z
    334, 260, 334, 584, // {|}~
];

/// Helvetica's width table, shared with the fixture writer so the two agree on layout.
#[cfg(feature = "write")]
pub(crate) fn helvetica_width(c: char) -> f64 {
    match c as u32 {
        32..=126 => HELVETICA[(c as u32 - 32) as usize] as f64 / 1000.0,
        _ => 0.556,
    }
}

/// The parts of a CMap text extraction uses.
struct CMap {
    map: HashMap<u32, String>,
    ranges: Vec<(usize, u32, u32)>,
}

impl CMap {
    fn parse(data: &[u8]) -> CMap {
        let mut map = HashMap::new();
        let mut ranges = Vec::new();
        let mut lx = Lexer::new(data);
        let mut operands: Vec<Obj> = Vec::new();
        while let Some(obj) = lx.object() {
            match obj {
                Obj::Keyword(k) => {
                    match k.as_slice() {
                        b"endcodespacerange" => {
                            for pair in operands.chunks(2) {
                                if let [Obj::Str(lo), Obj::Str(hi)] = pair
                                    && !lo.is_empty()
                                    && lo.len() <= 4
                                {
                                    ranges.push((lo.len(), be(lo), be(hi)));
                                }
                            }
                        }
                        b"endbfchar" => {
                            for pair in operands.chunks(2) {
                                if let [Obj::Str(src), dst] = pair
                                    && src.len() <= 4
                                    && let Some(s) = cmap_dst(dst)
                                {
                                    map.insert(be(src), s);
                                }
                            }
                        }
                        b"endbfrange" => {
                            for triple in operands.chunks(3) {
                                let [Obj::Str(lo), Obj::Str(hi), dst] = triple else {
                                    continue;
                                };
                                if lo.len() > 4 {
                                    continue;
                                }
                                let (lo, hi) = (be(lo), be(hi));
                                if hi < lo || hi - lo > 65_535 {
                                    continue;
                                }
                                match dst {
                                    Obj::Str(start) => {
                                        let units = utf16(start);
                                        for (k, code) in (lo..=hi).enumerate() {
                                            let mut u = units.clone();
                                            if let Some(last) = u.last_mut() {
                                                *last = last.wrapping_add(k as u16);
                                            }
                                            map.insert(code, String::from_utf16_lossy(&u));
                                        }
                                    }
                                    Obj::Array(items) => {
                                        for (code, item) in (lo..=hi).zip(items) {
                                            if let Some(s) = cmap_dst(item) {
                                                map.insert(code, s);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                    operands.clear();
                }
                other => operands.push(other),
            }
        }
        CMap { map, ranges }
    }
}

fn utf16(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .map(|c| {
            if c.len() == 2 {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                c[0] as u16
            }
        })
        .collect()
}

fn cmap_dst(dst: &Obj) -> Option<String> {
    match dst {
        Obj::Str(s) => Some(String::from_utf16_lossy(&utf16(s))),
        Obj::Name(n) => glyph_name(n).map(|c| c.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmap_bfchar_and_bfrange() {
        let cmap = CMap::parse(
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
              2 beginbfchar <0003> <0020> <0024> <00410042> endbfchar\n\
              1 beginbfrange <0010> <0012> <0061> endbfrange\n\
              1 beginbfrange <0020> <0021> [<00E9> <00F1>] endbfrange",
        );
        assert_eq!(cmap.ranges, vec![(2, 0, 0xffff)]);
        assert_eq!(cmap.map[&3], " ");
        assert_eq!(cmap.map[&0x24], "AB");
        assert_eq!(cmap.map[&0x11], "b");
        assert_eq!(cmap.map[&0x21], "ñ");
    }

    #[test]
    fn glyph_names() {
        assert_eq!(glyph_name(b"eacute"), Some('é'));
        assert_eq!(glyph_name(b"uni00F1"), Some('ñ'));
        assert_eq!(glyph_name(b"A"), Some('A'));
        assert_eq!(glyph_name(b"one.oldstyle"), Some('1'));
        assert_eq!(glyph_name(b"g123"), None);
    }
}
