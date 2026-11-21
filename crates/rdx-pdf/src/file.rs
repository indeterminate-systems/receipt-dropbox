//! Reading a PDF file: its objects, streams and pages.
//!
//! The reader scans the file for `n g obj` headers instead of trusting the cross-reference table,
//! which is often wrong in documents that were edited or produced carelessly. Later definitions
//! replace earlier ones, as incremental updates intend. Objects packed in object streams are read
//! too.

use std::collections::{BTreeMap, BTreeSet};

use crate::PdfError;
use crate::crypt::Security;
use crate::lex::{Dict, Lexer, Obj, Stream, is_delim, is_ws};

/// Decompressed streams larger than this are refused, so a small hostile file can't exhaust
/// memory.
const MAX_STREAM: usize = 64 << 20;
/// Page trees deeper than this are malformed.
const MAX_TREE_DEPTH: usize = 32;

pub(crate) struct Doc {
    objects: BTreeMap<u32, Obj>,
    /// Generation numbers of the objects written directly in the file, which encryption keys
    /// depend on.
    generations: BTreeMap<u32, u16>,
    trailer: Dict,
}

pub(crate) struct PageObj {
    pub resources: Dict,
    pub contents: Vec<u8>,
    /// `[x0 y0 x1 y1]` in points, from the page or the nearest page-tree node above it.
    pub media_box: Option<[f64; 4]>,
}

impl Doc {
    /// Reads a PDF, decrypting it with `password` (user or owner) if it is password-protected.
    /// An empty password opens files protected only against editing, as viewers do.
    pub fn load(bytes: &[u8], password: &str) -> Result<Doc, PdfError> {
        let head = &bytes[..bytes.len().min(1024)];
        if find(head, b"%PDF-", 0).is_none() {
            return Err(PdfError::NotPdf);
        }
        let mut doc = Doc {
            objects: BTreeMap::new(),
            generations: BTreeMap::new(),
            trailer: Dict::new(),
        };
        doc.scan_objects(bytes);
        doc.scan_trailers(bytes);
        // Object streams are encrypted as a whole, so they are decrypted before unpacking.
        doc.decrypt(password)?;
        doc.unpack_object_streams();
        Ok(doc)
    }

    fn decrypt(&mut self, password: &str) -> Result<(), PdfError> {
        let (dict, own_number) = match self.trailer.get(&b"Encrypt"[..]) {
            None => return Ok(()),
            Some(Obj::Ref(n, _)) => (self.get(*n).as_dict().cloned(), Some(*n)),
            Some(other) => (other.as_dict().cloned(), None),
        };
        let dict = dict.ok_or_else(|| {
            PdfError::Malformed("the trailer's /Encrypt is not a dictionary".into())
        })?;
        // Entries may be indirect; the crypt filter dictionaries in practice never are.
        let dict: Dict = dict
            .iter()
            .map(|(k, v)| (k.clone(), self.resolve(v).clone()))
            .collect();
        let id = match self.dict_get(&self.trailer, b"ID").as_array() {
            Some([Obj::Str(first), ..]) => first.clone(),
            _ => Vec::new(),
        };
        let security = Security::open(&dict, &id, password)?;
        for (num, obj) in self.objects.iter_mut() {
            // The encryption dictionary itself is never encrypted.
            if Some(*num) == own_number {
                continue;
            }
            let generation = self.generations.get(num).copied().unwrap_or(0);
            decrypt_obj(&security, *num, generation, obj);
        }
        Ok(())
    }

    fn scan_objects(&mut self, bytes: &[u8]) {
        let mut from = 0;
        while let Some(at) = find(bytes, b"obj", from) {
            from = at + 3;
            let Some((num, generation)) = object_header(bytes, at) else {
                continue;
            };
            if bytes
                .get(at + 3)
                .is_some_and(|b| !is_ws(*b) && !is_delim(*b))
            {
                continue;
            }
            let mut lx = Lexer::at(bytes, at + 3);
            let Some(obj) = lx.object() else { break };
            let obj = match obj {
                Obj::Dict(dict) => {
                    let save = lx.pos;
                    match lx.object() {
                        Some(k) if k.is_keyword(b"stream") => {
                            let (raw, end) = stream_data(bytes, lx.pos, &dict);
                            from = end;
                            Obj::Stream(Stream { dict, raw })
                        }
                        _ => {
                            lx.pos = save;
                            from = from.max(save);
                            Obj::Dict(dict)
                        }
                    }
                }
                other => {
                    from = from.max(lx.pos);
                    other
                }
            };
            // Cross-reference streams carry the trailer's keys.
            if let Some(d) = obj.as_dict()
                && d.get(&b"Type"[..]).and_then(Obj::as_name) == Some(b"XRef")
            {
                merge_trailer(&mut self.trailer, d);
            }
            self.objects.insert(num, obj);
            self.generations.insert(num, generation);
        }
    }

    fn scan_trailers(&mut self, bytes: &[u8]) {
        let mut from = 0;
        while let Some(at) = find(bytes, b"trailer", from) {
            from = at + 7;
            if let Some(Obj::Dict(d)) = Lexer::at(bytes, at + 7).object() {
                merge_trailer(&mut self.trailer, &d);
            }
        }
    }

    fn unpack_object_streams(&mut self) {
        let streams: Vec<Stream> = self
            .objects
            .values()
            .filter_map(|o| match o {
                Obj::Stream(s)
                    if s.dict.get(&b"Type"[..]).and_then(Obj::as_name) == Some(b"ObjStm") =>
                {
                    Some(s.clone())
                }
                _ => None,
            })
            .collect();
        for s in streams {
            let Ok(data) = decode(&s) else { continue };
            let n = s.dict.get(&b"N"[..]).and_then(Obj::as_i64).unwrap_or(0);
            let first = s.dict.get(&b"First"[..]).and_then(Obj::as_i64).unwrap_or(0);
            if n <= 0 || first < 0 {
                continue;
            }
            let mut header = Lexer::new(&data);
            for _ in 0..n {
                let (Some(Obj::Int(num)), Some(Obj::Int(off))) = (header.object(), header.object())
                else {
                    break;
                };
                let Ok(num) = u32::try_from(num) else {
                    continue;
                };
                let pos = first as usize + off.max(0) as usize;
                if pos >= data.len() {
                    continue;
                }
                // An object written directly in the file wins over a packed one.
                if self.objects.contains_key(&num) {
                    continue;
                }
                if let Some(obj) = Lexer::at(&data, pos).object() {
                    self.objects.insert(num, obj);
                }
            }
        }
    }

    pub fn get(&self, num: u32) -> &Obj {
        self.objects.get(&num).unwrap_or(&Obj::Null)
    }

    /// Follows references until it reaches a direct object (or a missing one, which is null).
    pub fn resolve<'a>(&'a self, mut obj: &'a Obj) -> &'a Obj {
        for _ in 0..16 {
            match obj {
                Obj::Ref(n, _) => obj = self.get(*n),
                _ => return obj,
            }
        }
        &Obj::Null
    }

    pub fn dict_get<'a>(&'a self, dict: &'a Dict, key: &[u8]) -> &'a Obj {
        dict.get(key).map(|o| self.resolve(o)).unwrap_or(&Obj::Null)
    }

    pub fn pages(&self) -> Vec<PageObj> {
        let mut out = Vec::new();
        let root = self.dict_get(&self.trailer, b"Root");
        let catalog = match root.as_dict() {
            Some(d) => Some(d),
            None => self
                .objects
                .values()
                .filter_map(Obj::as_dict)
                .find(|d| d.get(&b"Type"[..]).and_then(Obj::as_name) == Some(b"Catalog")),
        };
        if let Some(catalog) = catalog {
            let mut seen = BTreeSet::new();
            let pages = catalog.get(&b"Pages"[..]).unwrap_or(&Obj::Null);
            self.walk(pages, &Dict::new(), None, &mut seen, 0, &mut out);
        }
        if out.is_empty() {
            // No usable page tree: take every page object in object-number order.
            for obj in self.objects.values() {
                if let Some(d) = obj.as_dict()
                    && d.get(&b"Type"[..]).and_then(Obj::as_name) == Some(b"Page")
                {
                    let resources = self
                        .dict_get(d, b"Resources")
                        .as_dict()
                        .cloned()
                        .unwrap_or_default();
                    out.push(self.page(d, resources, None));
                }
            }
        }
        out
    }

    fn walk(
        &self,
        node: &Obj,
        inherited: &Dict,
        inherited_box: Option<[f64; 4]>,
        seen: &mut BTreeSet<u32>,
        depth: usize,
        out: &mut Vec<PageObj>,
    ) {
        if depth > MAX_TREE_DEPTH {
            return;
        }
        if let Obj::Ref(n, _) = node
            && !seen.insert(*n)
        {
            return;
        }
        let Some(dict) = self.resolve(node).as_dict() else {
            return;
        };
        let resources = match self.dict_get(dict, b"Resources").as_dict() {
            Some(r) => r.clone(),
            None => inherited.clone(),
        };
        let media_box = self.media_box(dict).or(inherited_box);
        let kids = self.dict_get(dict, b"Kids");
        let is_tree = dict.get(&b"Type"[..]).and_then(Obj::as_name) == Some(b"Pages");
        if let (true, Some(kids)) = (is_tree || kids.as_array().is_some(), kids.as_array()) {
            for kid in kids {
                self.walk(kid, &resources, media_box, seen, depth + 1, out);
            }
        } else {
            out.push(self.page(dict, resources, media_box));
        }
    }

    fn media_box(&self, dict: &Dict) -> Option<[f64; 4]> {
        let a = self.dict_get(dict, b"MediaBox").as_array()?;
        let v: Vec<f64> = a.iter().filter_map(|o| self.resolve(o).as_f64()).collect();
        (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
    }

    fn page(&self, dict: &Dict, resources: Dict, inherited_box: Option<[f64; 4]>) -> PageObj {
        let mut contents = Vec::new();
        let parts: Vec<&Obj> = match self.dict_get(dict, b"Contents") {
            Obj::Array(items) => items.iter().map(|o| self.resolve(o)).collect(),
            single => vec![single],
        };
        for part in parts {
            if let Obj::Stream(s) = part
                && let Ok(data) = decode(s)
            {
                contents.extend_from_slice(&data);
                // Content streams may split anywhere between tokens, but not inside one.
                contents.push(b'\n');
            }
        }
        PageObj {
            resources,
            contents,
            media_box: self.media_box(dict).or(inherited_box),
        }
    }

    /// A stream's decoded bytes, resolving an indirect `/Filter` or `/DecodeParms`.
    pub fn decode(&self, s: &Stream) -> Result<Vec<u8>, PdfError> {
        let mut direct = s.clone();
        for key in [&b"Filter"[..], b"DecodeParms"] {
            if let Some(v) = s.dict.get(key) {
                direct.dict.insert(key.to_vec(), self.resolve(v).clone());
            }
        }
        decode(&direct)
    }
}

fn merge_trailer(trailer: &mut Dict, d: &Dict) {
    for key in [&b"Root"[..], b"Encrypt", b"Info", b"ID"] {
        if let Some(v) = d.get(key) {
            trailer.insert(key.to_vec(), v.clone());
        }
    }
}

pub(crate) fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Decrypts an object's strings and, for a stream, its data. Cross-reference streams are never
/// encrypted, and metadata streams aren't when the handler says so.
fn decrypt_obj(security: &Security, num: u32, generation: u16, obj: &mut Obj) {
    match obj {
        Obj::Str(s) => *s = security.decrypt_string(num, generation, s),
        Obj::Array(items) => items
            .iter_mut()
            .for_each(|o| decrypt_obj(security, num, generation, o)),
        Obj::Dict(d) => d
            .values_mut()
            .for_each(|o| decrypt_obj(security, num, generation, o)),
        Obj::Stream(s) => {
            let kind = s.dict.get(&b"Type"[..]).and_then(Obj::as_name);
            if kind == Some(b"XRef") {
                return;
            }
            if !(kind == Some(b"Metadata") && !security.encrypt_metadata) {
                s.raw = security.decrypt_stream(num, generation, &s.raw);
            }
            s.dict
                .values_mut()
                .for_each(|o| decrypt_obj(security, num, generation, o));
        }
        _ => {}
    }
}

/// For `obj` at `at`, the object and generation numbers of a well-formed `n g obj` header
/// before it.
fn object_header(bytes: &[u8], at: usize) -> Option<(u32, u16)> {
    let mut i = at;
    let skip_ws = |i: &mut usize| {
        let start = *i;
        while *i > 0 && is_ws(bytes[*i - 1]) {
            *i -= 1;
        }
        *i < start
    };
    let digits = |i: &mut usize| {
        let end = *i;
        while *i > 0 && bytes[*i - 1].is_ascii_digit() {
            *i -= 1;
        }
        (*i < end).then(|| std::str::from_utf8(&bytes[*i..end]).ok())?
    };
    if !skip_ws(&mut i) {
        return None;
    }
    let generation = digits(&mut i)?;
    if !skip_ws(&mut i) {
        return None;
    }
    let num = digits(&mut i)?;
    if i > 0 && !is_ws(bytes[i - 1]) && !is_delim(bytes[i - 1]) {
        return None;
    }
    Some((num.parse().ok()?, generation.parse().ok()?))
}

/// A stream's raw bytes starting after the `stream` keyword, and where parsing resumes.
fn stream_data(bytes: &[u8], mut start: usize, dict: &Dict) -> (Vec<u8>, usize) {
    // The keyword is followed by CRLF or LF (some producers write a lone CR).
    if bytes.get(start) == Some(&b'\r') {
        start += 1;
    }
    if bytes.get(start) == Some(&b'\n') {
        start += 1;
    }
    if let Some(len) = dict.get(&b"Length"[..]).and_then(|o| match o {
        Obj::Int(n) => usize::try_from(*n).ok(),
        _ => None,
    }) && let Some(end) = start.checked_add(len)
        && end <= bytes.len()
    {
        let mut k = end;
        while k < bytes.len() && is_ws(bytes[k]) {
            k += 1;
        }
        if bytes[k..].starts_with(b"endstream") {
            return (bytes[start..end].to_vec(), k + 9);
        }
    }
    // Indirect or wrong /Length: the data ends at the next `endstream`.
    match find(bytes, b"endstream", start) {
        Some(at) => {
            let mut end = at;
            if end > start && bytes[end - 1] == b'\n' {
                end -= 1;
            }
            if end > start && bytes[end - 1] == b'\r' {
                end -= 1;
            }
            (bytes[start..end].to_vec(), at + 9)
        }
        None => (bytes[start..].to_vec(), bytes.len()),
    }
}

/// Applies a stream's filters. Only the filters text needs are supported; image filters are
/// reported as unsupported, which callers treat as "no text here".
pub(crate) fn decode(s: &Stream) -> Result<Vec<u8>, PdfError> {
    let filters: Vec<&[u8]> = match s.dict.get(&b"Filter"[..]) {
        Some(Obj::Name(n)) => vec![n.as_slice()],
        Some(Obj::Array(a)) => a.iter().filter_map(Obj::as_name).collect(),
        _ => vec![],
    };
    let parms: Vec<Option<&Dict>> = match s.dict.get(&b"DecodeParms"[..]) {
        Some(Obj::Dict(d)) => vec![Some(d)],
        Some(Obj::Array(a)) => a.iter().map(Obj::as_dict).collect(),
        _ => vec![],
    };
    let mut data = s.raw.clone();
    for (i, f) in filters.iter().enumerate() {
        data = match *f {
            b"FlateDecode" | b"Fl" => {
                let inflated = inflate(&data)?;
                match parms.get(i).copied().flatten() {
                    Some(p) => predict(inflated, p)?,
                    None => inflated,
                }
            }
            b"ASCIIHexDecode" | b"AHx" => {
                let mut wrapped = Vec::with_capacity(data.len() + 2);
                wrapped.push(b'<');
                wrapped.extend_from_slice(&data);
                wrapped.push(b'>');
                match Lexer::new(&wrapped).object() {
                    Some(Obj::Str(s)) => s,
                    _ => Vec::new(),
                }
            }
            b"ASCII85Decode" | b"A85" => ascii85(&data)?,
            other => {
                return Err(PdfError::Unsupported(format!(
                    "stream filter `{}`",
                    String::from_utf8_lossy(other)
                )));
            }
        };
        if data.len() > MAX_STREAM {
            return Err(PdfError::Malformed(
                "a stream decompresses too large".into(),
            ));
        }
    }
    Ok(data)
}

fn inflate(data: &[u8]) -> Result<Vec<u8>, PdfError> {
    use miniz_oxide::inflate::{decompress_to_vec_with_limit, decompress_to_vec_zlib_with_limit};
    match decompress_to_vec_zlib_with_limit(data, MAX_STREAM) {
        Ok(v) => Ok(v),
        // Some producers write a bad checksum or truncate the stream; take what inflates.
        Err(e) if !e.output.is_empty() => Ok(e.output),
        Err(_) if data.len() > 2 => {
            decompress_to_vec_with_limit(&data[2..], MAX_STREAM).or_else(|e| {
                if e.output.is_empty() {
                    Err(PdfError::Malformed(
                        "a compressed stream does not inflate".into(),
                    ))
                } else {
                    Ok(e.output)
                }
            })
        }
        Err(_) => Err(PdfError::Malformed(
            "a compressed stream does not inflate".into(),
        )),
    }
}

/// PNG predictors (`/Predictor` 10 to 15), as cross-reference and object streams use them.
fn predict(data: Vec<u8>, parms: &Dict) -> Result<Vec<u8>, PdfError> {
    let int = |k: &[u8], d: i64| parms.get(k).and_then(Obj::as_i64).unwrap_or(d);
    let predictor = int(b"Predictor", 1);
    if predictor < 10 {
        return Ok(data);
    }
    let colors = int(b"Colors", 1).clamp(1, 32) as usize;
    let bpc = int(b"BitsPerComponent", 8).clamp(1, 16) as usize;
    let columns = int(b"Columns", 1).clamp(1, 1 << 20) as usize;
    let bpp = (colors * bpc).div_ceil(8);
    let row = (colors * bpc * columns).div_ceil(8);
    let mut out = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; row];
    for chunk in data.chunks(row + 1) {
        if chunk.len() < 2 {
            break;
        }
        let kind = chunk[0];
        let mut cur = chunk[1..].to_vec();
        cur.resize(row, 0);
        for i in 0..row {
            let left = if i >= bpp { cur[i - bpp] } else { 0 };
            let up = prev[i];
            let up_left = if i >= bpp { prev[i - bpp] } else { 0 };
            cur[i] = match kind {
                1 => cur[i].wrapping_add(left),
                2 => cur[i].wrapping_add(up),
                3 => cur[i].wrapping_add(((left as u16 + up as u16) / 2) as u8),
                4 => cur[i].wrapping_add(paeth(left, up, up_left)),
                _ => cur[i],
            };
        }
        out.extend_from_slice(&cur);
        prev = cur;
    }
    Ok(out)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = (
        (p - a as i16).abs(),
        (p - b as i16).abs(),
        (p - c as i16).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn ascii85(data: &[u8]) -> Result<Vec<u8>, PdfError> {
    let mut out = Vec::new();
    let mut group = [0u32; 5];
    let mut n = 0;
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        i += 1;
        match b {
            b'~' => break,
            b'z' if n == 0 => out.extend_from_slice(&[0, 0, 0, 0]),
            b'!'..=b'u' => {
                group[n] = (b - b'!') as u32;
                n += 1;
                if n == 5 {
                    let v = group.iter().fold(0u64, |acc, d| acc * 85 + *d as u64);
                    out.extend_from_slice(&(v as u32).to_be_bytes());
                    n = 0;
                }
            }
            _ if is_ws(b) => {}
            _ => return Err(PdfError::Malformed("bad ASCII85 data".into())),
        }
    }
    if n > 1 {
        for g in group.iter_mut().skip(n) {
            *g = 84;
        }
        let v = group.iter().fold(0u64, |acc, d| acc * 85 + *d as u64);
        out.extend_from_slice(&(v as u32).to_be_bytes()[..n - 1]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_headers_need_two_numbers() {
        let b = b"xx 12 0 obj";
        assert_eq!(object_header(b, 8), Some((12, 0)));
        assert_eq!(object_header(b"endobj", 3), None);
        assert_eq!(object_header(b"a12 0 obj", 6), None);
    }

    #[test]
    fn ascii85_round_trip_sample() {
        // "Man " in ASCII85.
        assert_eq!(ascii85(b"9jqo^~>").unwrap(), b"Man ");
    }
}
