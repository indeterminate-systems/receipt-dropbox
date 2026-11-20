//! Just enough RFC 5322 and MIME to get a receipt out of an email: headers (folded, RFC 2047
//! encoded words), multipart bodies, nested `message/rfc822`, quoted-printable and base64, and
//! UTF-8, ASCII and Latin-1 charsets.

use rdx_model::Digest;

use crate::{Attachment, html, scan};

#[derive(Debug, Default)]
pub struct Email {
    /// `From` display name, or None for a bare address.
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub date: Option<String>,
    /// The readable body: text parts, and HTML parts converted to text.
    pub text: String,
    /// XML parts and attachments (CFDI), as text.
    pub xml: Vec<String>,
    /// Everything else: PDFs, images.
    pub attachments: Vec<Attachment>,
    /// The sender of a forwarded `message/rfc822` part, if any.
    forwarded_from: Option<String>,
}

const FORWARD_PREFIXES: &[&str] = &["fwd:", "fw:", "rv:", "tr:", "wg:", "enc:"];
const FORWARD_MARKERS: &[&str] = &[
    "forwarded message",
    "original message",
    "mensaje reenviado",
    "mensaje original",
    "begin forwarded message",
];

impl Email {
    /// Who the receipt is from: the original sender of a forwarded email (supporters forward
    /// receipts to the intake mailbox), otherwise the `From` display name.
    pub fn merchant_sender(&self) -> Option<String> {
        if let Some(f) = &self.forwarded_from {
            return Some(f.clone());
        }
        let folded = scan::fold(&self.text);
        let forwarded = self.subject.as_deref().is_some_and(|s| {
            FORWARD_PREFIXES
                .iter()
                .any(|p| scan::fold(s).starts_with(p))
        }) || FORWARD_MARKERS.iter().any(|m| folded.contains(m));
        if !forwarded {
            return self.from_name.clone();
        }
        // The forwarded header block: the first `From:` or `De:` line after the marker.
        let start = FORWARD_MARKERS
            .iter()
            .filter_map(|m| folded.find(m))
            .min()
            .unwrap_or(0);
        // Folding keeps chars one for one, not bytes.
        let start = scan::byte_at(&self.text, folded[..start].chars().count());
        self.text[start..].lines().find_map(|line| {
            let line = line.trim_start();
            let folded = scan::fold(line);
            let label = ["from:", "de:"]
                .into_iter()
                .find(|l| folded.starts_with(l))?;
            // Labels are ASCII, so their length is the same in the original.
            display_name(line[label.len()..].trim())
        })
    }
}

pub fn parse(raw: &str) -> Email {
    let raw = raw.replace("\r\n", "\n");
    let mut email = Email::default();
    let (headers, body) = split_entity(&raw);
    email.from_name = header(&headers, "from").and_then(|v| display_name(&v));
    email.subject = header(&headers, "subject");
    email.date = header(&headers, "date");
    walk(&headers, body, &mut email, 0);
    email
}

fn split_entity(raw: &str) -> (Vec<(String, String)>, &str) {
    let (head, body) = match raw.find("\n\n") {
        Some(i) => (&raw[..i], &raw[i + 2..]),
        None if raw.starts_with('\n') => ("", &raw[1..]),
        None => (raw, ""),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = headers.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    (headers, body)
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| decode_words(v))
}

/// `"Paperleaf Supply" <billing@paperleaf.example>` to `Paperleaf Supply`.
fn display_name(value: &str) -> Option<String> {
    let name = {
        let i = value.find('<')?;
        &value[..i]
    };
    let name = name.trim().trim_matches('"').trim();
    (!name.is_empty() && !name.contains('@')).then(|| name.to_owned())
}

/// The value before `;` and its parameters, lowercased names.
fn params(value: &str) -> (String, Vec<(String, String)>) {
    let mut parts = value.split(';');
    let main = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let ps = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| {
            (
                k.trim().to_ascii_lowercase(),
                v.trim().trim_matches('"').to_owned(),
            )
        })
        .collect();
    (main, ps)
}

fn param<'a>(ps: &'a [(String, String)], name: &str) -> Option<&'a str> {
    ps.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn walk(headers: &[(String, String)], body: &str, email: &mut Email, depth: u8) {
    // Real receipts nest two or three levels; the limit only stops hostile input.
    if depth > 8 {
        return;
    }
    let (ctype, cparams) = header(headers, "content-type")
        .map(|v| params(&v))
        .unwrap_or_else(|| ("text/plain".into(), Vec::new()));
    let (disposition, dparams) = header(headers, "content-disposition")
        .map(|v| params(&v))
        .unwrap_or_default();
    let file_name = param(&dparams, "filename")
        .or_else(|| param(&cparams, "name"))
        .map(decode_words);

    if let Some(boundary) = ctype
        .starts_with("multipart/")
        .then(|| param(&cparams, "boundary"))
        .flatten()
    {
        let parts = split_multipart(body, boundary);
        if ctype == "multipart/alternative" {
            // Prefer plain text when it says something; otherwise the HTML.
            let mut plain = Email::default();
            let mut rich = Email::default();
            for part in &parts {
                let (h, b) = split_entity(part);
                let is_html =
                    header(&h, "content-type").is_some_and(|v| params(&v).0 == "text/html");
                walk(
                    &h,
                    b,
                    if is_html { &mut rich } else { &mut plain },
                    depth + 1,
                );
            }
            let chosen = if plain.text.trim().is_empty() {
                rich
            } else {
                plain
            };
            merge(email, chosen);
        } else {
            for part in &parts {
                let (h, b) = split_entity(part);
                walk(&h, b, email, depth + 1);
            }
        }
        return;
    }

    if ctype == "message/rfc822" {
        let (h, b) = split_entity(body);
        if email.forwarded_from.is_none() {
            email.forwarded_from = header(&h, "from").and_then(|v| display_name(&v));
        }
        walk(&h, b, email, depth + 1);
        return;
    }

    let bytes = decode_transfer(
        body,
        header(headers, "content-transfer-encoding").as_deref(),
    );
    let is_xml = ctype.ends_with("/xml")
        || file_name
            .as_deref()
            .is_some_and(|n| n.to_ascii_lowercase().ends_with(".xml"));
    let attachment = disposition == "attachment";
    if is_xml {
        email
            .xml
            .push(decode_charset(&bytes, param(&cparams, "charset")));
    } else if (ctype == "text/plain" || ctype == "text/html") && !attachment {
        let text = decode_charset(&bytes, param(&cparams, "charset"));
        let text = if ctype == "text/html" {
            html::to_text(&text)
        } else {
            text
        };
        if !email.text.is_empty() && !email.text.ends_with('\n') {
            email.text.push('\n');
        }
        email.text.push_str(&text);
    } else {
        email.attachments.push(Attachment {
            media_type: ctype,
            file_name,
            digest: Digest::of_bytes(&bytes),
            size: bytes.len(),
        });
    }
}

fn merge(into: &mut Email, from: Email) {
    if !into.text.is_empty() && !into.text.ends_with('\n') {
        into.text.push('\n');
    }
    into.text.push_str(&from.text);
    into.xml.extend(from.xml);
    into.attachments.extend(from.attachments);
    if into.forwarded_from.is_none() {
        into.forwarded_from = from.forwarded_from;
    }
}

fn split_multipart<'a>(body: &'a str, boundary: &str) -> Vec<&'a str> {
    let delimiter = format!("--{boundary}");
    let mut parts = Vec::new();
    let mut current: Option<usize> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed == delimiter || trimmed == format!("{delimiter}--") {
            if let Some(start) = current {
                parts.push(body[start..offset].trim_end_matches('\n'));
            }
            if trimmed.ends_with("--") && trimmed != delimiter {
                return parts;
            }
            current = Some(offset + line.len());
        }
        offset += line.len();
    }
    if let Some(start) = current {
        parts.push(&body[start..]);
    }
    parts
}

fn decode_transfer(body: &str, encoding: Option<&str>) -> Vec<u8> {
    match encoding.map(|e| e.trim().to_ascii_lowercase()).as_deref() {
        Some("base64") => base64(body),
        Some("quoted-printable") => quoted_printable(body),
        _ => body.as_bytes().to_vec(),
    }
}

fn base64(text: &str) -> Vec<u8> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for v in text.bytes().filter_map(value) {
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

fn quoted_printable(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    while i < bytes.len() {
        if bytes[i] == b'=' {
            // Soft line break.
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if let (Some(h), Some(l)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            ) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn decode_charset(bytes: &[u8], charset: Option<&str>) -> String {
    match charset.map(str::to_ascii_lowercase).as_deref() {
        Some("iso-8859-1" | "latin1" | "latin-1" | "windows-1252" | "cp1252" | "iso-8859-15") => {
            // Latin-1 maps bytes to the first 256 code points. Windows-1252's 0x80 to 0x9f
            // punctuation is rare in receipts and comes through as control characters.
            bytes.iter().map(|&b| b as char).collect()
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// RFC 2047: `=?utf-8?Q?Caf=C3=A9?=` and `=?utf-8?B?...?=` in header values.
fn decode_words(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let between = &rest[..start];
        // Whitespace between two encoded words is dropped.
        if !(last_was_word && between.trim().is_empty()) {
            out.push_str(between);
        }
        let word = &rest[start + 2..];
        let decoded = (|| {
            let (charset, word) = word.split_once('?')?;
            let (enc, word) = word.split_once('?')?;
            let end = word.find("?=")?;
            let data = &word[..end];
            let bytes = match enc.to_ascii_lowercase().as_str() {
                "b" => base64(data),
                "q" => quoted_printable(&data.replace('_', " ")),
                _ => return None,
            };
            let consumed = charset.len() + enc.len() + end + 6;
            Some((decode_charset(&bytes, Some(charset)), consumed))
        })();
        match decoded {
            Some((text, consumed)) => {
                out.push_str(&text);
                rest = &rest[start + consumed..];
                last_was_word = true;
            }
            None => {
                out.push_str("=?");
                rest = &rest[start + 2..];
                last_was_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_words() {
        assert_eq!(decode_words("=?utf-8?Q?Caf=C3=A9_Sol?="), "Café Sol");
        assert_eq!(decode_words("=?UTF-8?B?Q2Fmw6k=?= <a@b>"), "Café <a@b>");
        assert_eq!(decode_words("plain"), "plain");
    }

    #[test]
    fn display_names() {
        assert_eq!(
            display_name("\"Paperleaf Supply\" <billing@paperleaf.example>").as_deref(),
            Some("Paperleaf Supply")
        );
        assert_eq!(display_name("billing@paperleaf.example"), None);
    }

    #[test]
    fn transfer_encodings() {
        assert_eq!(base64("Q2Fmw6k=\n"), "Café".as_bytes());
        assert_eq!(
            quoted_printable("Caf=C3=A9 Spring=\nfield"),
            "Café Springfield".as_bytes()
        );
    }

    #[test]
    fn nested_forwarded_message() {
        let raw = "From: Supporter <s@example.org>\nSubject: Fwd: receipt\n\
            Content-Type: multipart/mixed; boundary=x\n\n--x\nContent-Type: text/plain\n\nsee below\n\
            --x\nContent-Type: message/rfc822\n\nFrom: Shop Co <shop@example.com>\n\
            Content-Type: text/plain\n\nTotal USD 1.00\n--x--\n";
        let email = parse(raw);
        assert_eq!(email.merchant_sender().as_deref(), Some("Shop Co"));
        assert!(email.text.contains("Total USD 1.00"), "{}", email.text);
    }
}
