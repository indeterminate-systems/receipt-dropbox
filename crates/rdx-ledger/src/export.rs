//! Spreadsheet exports of a ledger: one row per posting, so a pivot by account, supporter or
//! statement needs no further work. Amounts are exact decimal text (never floating point), and
//! every row keeps its audit links.
//!
//! The XLSX writer is hand-written: a single sheet of inline strings and numbers in an
//! uncompressed ZIP with fixed timestamps, so the same ledger always exports to the same bytes
//! (and digest), with no compression crate (and no C) in the WebAssembly builds.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::Ledger;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Csv,
    Xlsx,
}

impl Format {
    pub fn media_type(self) -> &'static str {
        match self {
            Format::Csv => "text/csv",
            Format::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Xlsx => "xlsx",
        }
    }
}

pub const COLUMNS: [&str; 12] = [
    "date",
    "payee",
    "narration",
    "account",
    "amount",
    "currency",
    "supporter",
    "statement_id",
    "line",
    "receipt",
    "receipt_source",
    "statement",
];

/// Index of the `amount` column, written as a number in XLSX.
const AMOUNT: usize = 4;
/// Free text from documents; guarded against spreadsheet formula injection.
const FREE_TEXT: [usize; 2] = [1, 2];

/// The ledger's postings as rows of [`COLUMNS`], in ledger order.
pub fn rows(ledger: &Ledger) -> Vec<[String; 12]> {
    let mut out = Vec::new();
    for e in &ledger.entries {
        for p in &e.postings {
            let (amount, currency) = match p.amount {
                Some(a) => (a.decimal(), a.currency().to_string()),
                None => (String::new(), String::new()),
            };
            out.push([
                e.date.to_string(),
                e.payee.clone(),
                e.narration.clone(),
                p.account.to_string(),
                amount,
                currency,
                e.supporter.to_string(),
                e.line.statement.to_string(),
                e.line.line.to_string(),
                e.receipt.to_string(),
                e.receipt_source.to_string(),
                e.statement.to_string(),
            ]);
        }
    }
    out
}

/// A cell a spreadsheet would run as a formula (`=`, `+`, `-`, `@`, tab, carriage return) gets
/// a leading `'`, the usual guard against CSV injection; payees come from receipts anyone can
/// send.
fn defuse(cell: &str) -> String {
    match cell.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{cell}"),
        _ => cell.to_string(),
    }
}

fn cells(row: &[String; 12]) -> impl Iterator<Item = (usize, String)> + '_ {
    row.iter().enumerate().map(|(i, c)| {
        if FREE_TEXT.contains(&i) {
            (i, defuse(c))
        } else {
            (i, c.clone())
        }
    })
}

/// RFC 4180 CSV with a header row and CRLF line endings.
pub fn csv(ledger: &Ledger) -> String {
    let mut out = String::new();
    let line = |out: &mut String, cells: &mut dyn Iterator<Item = String>| {
        let quoted: Vec<String> = cells
            .map(|c| {
                if c.contains([',', '"', '\n', '\r']) {
                    format!("\"{}\"", c.replace('"', "\"\""))
                } else {
                    c
                }
            })
            .collect();
        out.push_str(&quoted.join(","));
        out.push_str("\r\n");
    };
    line(&mut out, &mut COLUMNS.iter().map(|c| c.to_string()));
    for row in rows(ledger) {
        line(&mut out, &mut cells(&row).map(|(_, c)| c));
    }
    out
}

/// An XLSX workbook with one sheet, `Ledger`.
pub fn xlsx(ledger: &Ledger) -> Vec<u8> {
    let mut sheet = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    let mut row_xml = |r: usize, cells: &mut dyn Iterator<Item = (usize, String)>| {
        write!(sheet, r#"<row r="{r}">"#).unwrap();
        for (i, c) in cells {
            let reference = format!("{}{r}", column_name(i));
            if c.is_empty() {
                continue;
            }
            if i == AMOUNT && r > 1 {
                write!(sheet, r#"<c r="{reference}"><v>{c}</v></c>"#).unwrap();
            } else {
                write!(
                    sheet,
                    r#"<c r="{reference}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                    xml_escape(&c)
                )
                .unwrap();
            }
        }
        sheet.push_str("</row>");
    };
    row_xml(1, &mut COLUMNS.iter().map(|c| c.to_string()).enumerate());
    for (n, row) in rows(ledger).iter().enumerate() {
        row_xml(n + 2, &mut cells(row));
    }
    sheet.push_str("</sheetData></worksheet>");

    const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Ledger" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;

    zip_stored(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", RELS.as_bytes()),
        ("xl/workbook.xml", WORKBOOK.as_bytes()),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ])
}

/// `A`, `B`, … (12 columns, so one letter is enough).
fn column_name(i: usize) -> char {
    (b'A' + i as u8) as char
}

/// Escapes XML text and drops characters XML 1.0 can't hold.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// A ZIP archive of uncompressed ("stored") files with a fixed 1980-01-01 timestamp.
pub(crate) fn zip_stored(files: &[(&str, &[u8])]) -> Vec<u8> {
    const DOS_DATE: u16 = (1 << 5) | 1; // 1980-01-01
    const UTF8_NAMES: u16 = 1 << 11;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let common = |v: &mut Vec<u8>| {
            v.extend(20u16.to_le_bytes()); // version needed: 2.0
            v.extend(UTF8_NAMES.to_le_bytes());
            v.extend(0u16.to_le_bytes()); // stored
            v.extend(0u16.to_le_bytes()); // time 00:00
            v.extend(DOS_DATE.to_le_bytes());
            v.extend(crc.to_le_bytes());
            v.extend(size.to_le_bytes());
            v.extend(size.to_le_bytes());
            v.extend((name.len() as u16).to_le_bytes());
            v.extend(0u16.to_le_bytes()); // extra field length
        };
        out.extend(0x0403_4b50u32.to_le_bytes());
        common(&mut out);
        out.extend(name.as_bytes());
        out.extend(*data);

        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend(20u16.to_le_bytes()); // version made by
        common(&mut central);
        central.extend(0u16.to_le_bytes()); // comment length
        central.extend(0u16.to_le_bytes()); // disk number
        central.extend(0u16.to_le_bytes()); // internal attributes
        central.extend(0u32.to_le_bytes()); // external attributes
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    out.extend(central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // this disk
    out.extend(0u16.to_le_bytes()); // disk with the central directory
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend(central_size.to_le_bytes());
    out.extend(central_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // comment length
    out
}

/// CRC-32 (IEEE), as ZIP uses.
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn formulas_are_defused() {
        assert_eq!(defuse("=HYPERLINK(\"x\")"), "'=HYPERLINK(\"x\")");
        assert_eq!(defuse("-1+2"), "'-1+2");
        assert_eq!(defuse("Paperleaf"), "Paperleaf");
    }
}
