//! The content-stream interpreter: runs a page's drawing operators far enough to know where each
//! glyph of text lands.

use std::collections::HashMap;
use std::rc::Rc;

use crate::file::Doc;
use crate::font::Font;
use crate::lex::{Dict, Lexer, Obj};

/// Form XObjects nested deeper than this are not followed.
const MAX_FORM_DEPTH: usize = 8;

type Matrix = [f64; 6];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a × b`, PDF's row-vector convention: apply `a`, then `b`.
fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

/// One glyph as placed on the page, in default user space (points, origin bottom left).
#[derive(Debug, Clone)]
pub(crate) struct Glyph {
    pub x: f64,
    pub y: f64,
    /// Advance width on the page.
    pub width: f64,
    /// Rendered font size on the page.
    pub size: f64,
    pub text: String,
}

#[derive(Clone)]
struct State {
    ctm: Matrix,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
    font_size: f64,
    font: Option<Rc<Font>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            ctm: IDENTITY,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            font_size: 0.0,
            font: None,
        }
    }
}

pub(crate) struct Interpreter<'d> {
    doc: &'d Doc,
    fonts: HashMap<u32, Rc<Font>>,
    pub glyphs: Vec<Glyph>,
}

impl<'d> Interpreter<'d> {
    pub fn new(doc: &'d Doc) -> Self {
        Interpreter {
            doc,
            fonts: HashMap::new(),
            glyphs: Vec::new(),
        }
    }

    pub fn run_page(&mut self, content: &[u8], resources: &Dict) {
        self.run(content, resources, State::default(), 0);
    }

    fn font(&mut self, resources: &Dict, name: &[u8]) -> Option<Rc<Font>> {
        let fonts = self.doc.dict_get(resources, b"Font").as_dict()?;
        let entry = fonts.get(name)?;
        if let Obj::Ref(n, _) = entry
            && let Some(f) = self.fonts.get(n)
        {
            return Some(f.clone());
        }
        let dict = self.doc.resolve(entry).as_dict()?;
        let font = Rc::new(Font::load(self.doc, dict));
        if let Obj::Ref(n, _) = entry {
            self.fonts.insert(*n, font.clone());
        }
        Some(font)
    }

    fn run(&mut self, content: &[u8], resources: &Dict, mut gs: State, depth: usize) {
        let mut stack: Vec<State> = Vec::new();
        let mut tm = IDENTITY;
        let mut tlm = IDENTITY;
        let mut operands: Vec<Obj> = Vec::new();
        let mut lx = Lexer::new(content);
        while let Some(obj) = lx.object() {
            let Obj::Keyword(op) = obj else {
                operands.push(obj);
                continue;
            };
            let num = |i: usize| operands.get(i).and_then(Obj::as_f64).unwrap_or(0.0);
            let nums = |n: usize| -> Option<Vec<f64>> {
                (operands.len() >= n).then(|| {
                    operands[operands.len() - n..]
                        .iter()
                        .map(|o| o.as_f64().unwrap_or(0.0))
                        .collect()
                })
            };
            match op.as_slice() {
                b"q" => stack.push(gs.clone()),
                b"Q" => {
                    if let Some(s) = stack.pop() {
                        gs = s;
                    }
                }
                b"cm" => {
                    if let Some(m) = nums(6) {
                        gs.ctm = mul(&[m[0], m[1], m[2], m[3], m[4], m[5]], &gs.ctm);
                    }
                }
                b"BT" => {
                    tm = IDENTITY;
                    tlm = IDENTITY;
                }
                b"Tf" => {
                    if let (Some(Obj::Name(name)), Some(size)) =
                        (operands.first(), operands.get(1).and_then(Obj::as_f64))
                    {
                        gs.font = self.font(resources, name);
                        gs.font_size = size;
                    }
                }
                b"Tc" => gs.char_spacing = num(0),
                b"Tw" => gs.word_spacing = num(0),
                b"Tz" => gs.h_scale = num(0) / 100.0,
                b"TL" => gs.leading = num(0),
                b"Ts" => gs.rise = num(0),
                b"Td" | b"TD" => {
                    let (tx, ty) = (num(0), num(1));
                    if op == b"TD" {
                        gs.leading = -ty;
                    }
                    tlm = mul(&[1.0, 0.0, 0.0, 1.0, tx, ty], &tlm);
                    tm = tlm;
                }
                b"Tm" => {
                    if let Some(m) = nums(6) {
                        tlm = [m[0], m[1], m[2], m[3], m[4], m[5]];
                        tm = tlm;
                    }
                }
                b"T*" => {
                    tlm = mul(&[1.0, 0.0, 0.0, 1.0, 0.0, -gs.leading], &tlm);
                    tm = tlm;
                }
                b"Tj" | b"'" | b"\"" => {
                    if op == b"\"" {
                        gs.word_spacing = num(0);
                        gs.char_spacing = num(1);
                    }
                    if op != b"Tj" {
                        tlm = mul(&[1.0, 0.0, 0.0, 1.0, 0.0, -gs.leading], &tlm);
                        tm = tlm;
                    }
                    if let Some(Obj::Str(s)) = operands.last() {
                        let s = s.clone();
                        self.show(&s, &gs, &mut tm);
                    }
                }
                b"TJ" => {
                    if let Some(Obj::Array(items)) = operands.last() {
                        let items = items.clone();
                        for item in items {
                            match item {
                                Obj::Str(s) => self.show(&s, &gs, &mut tm),
                                other => {
                                    let adj = other.as_f64().unwrap_or(0.0);
                                    let tx = -adj / 1000.0 * gs.font_size * gs.h_scale;
                                    tm = mul(&[1.0, 0.0, 0.0, 1.0, tx, 0.0], &tm);
                                }
                            }
                        }
                    }
                }
                b"Do" if depth < MAX_FORM_DEPTH => {
                    if let Some(Obj::Name(name)) = operands.first() {
                        self.form(resources, name, &gs, depth);
                    }
                }
                b"ID" => lx.skip_inline_image(),
                _ => {}
            }
            operands.clear();
        }
    }

    fn form(&mut self, resources: &Dict, name: &[u8], gs: &State, depth: usize) {
        let doc = self.doc;
        let Some(xobjects) = doc.dict_get(resources, b"XObject").as_dict() else {
            return;
        };
        let Some(Obj::Stream(s)) = xobjects.get(name).map(|o| doc.resolve(o)) else {
            return;
        };
        if doc.dict_get(&s.dict, b"Subtype").as_name() != Some(b"Form") {
            return;
        }
        let Ok(data) = doc.decode(s) else { return };
        let matrix = match doc.dict_get(&s.dict, b"Matrix").as_array() {
            Some(m) if m.len() == 6 => {
                let v: Vec<f64> = m.iter().map(|o| o.as_f64().unwrap_or(0.0)).collect();
                [v[0], v[1], v[2], v[3], v[4], v[5]]
            }
            _ => IDENTITY,
        };
        let res = doc
            .dict_get(&s.dict, b"Resources")
            .as_dict()
            .cloned()
            .unwrap_or_else(|| resources.clone());
        let mut inner = gs.clone();
        inner.ctm = mul(&matrix, &gs.ctm);
        self.run(&data, &res, inner, depth + 1);
    }

    fn show(&mut self, bytes: &[u8], gs: &State, tm: &mut Matrix) {
        let Some(font) = gs.font.clone() else {
            return;
        };
        for (code, len) in font.codes(bytes) {
            let w0 = font.width(code);
            let trm = mul(
                &[
                    gs.font_size * gs.h_scale,
                    0.0,
                    0.0,
                    gs.font_size,
                    0.0,
                    gs.rise,
                ],
                &mul(tm, &gs.ctm),
            );
            let text = font.text(code);
            // Rotated or skewed text (margin codes, watermarks) is not part of any table row.
            let horizontal = trm[1].abs() <= 0.01 * trm[0].abs() && trm[0] > 0.0;
            if horizontal {
                self.glyphs.push(Glyph {
                    x: trm[4],
                    y: trm[5],
                    width: w0 * trm[0],
                    size: (trm[2] * trm[2] + trm[3] * trm[3]).sqrt(),
                    text,
                });
            }
            let word = if len == 1 && code == 32 {
                gs.word_spacing
            } else {
                0.0
            };
            let tx = (w0 * gs.font_size + gs.char_spacing + word) * gs.h_scale;
            *tm = mul(&[1.0, 0.0, 0.0, 1.0, tx, 0.0], tm);
        }
    }
}
