//! CSV and XLSX exports of the golden ledger.

use std::path::{Path, PathBuf};

use rdx_ledger::{Ledger, export};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ledger() -> Ledger {
    let text =
        std::fs::read_to_string(repo().join("fixtures/ledger/synthetic-small.beancount")).unwrap();
    Ledger::parse(&text).unwrap()
}

#[test]
fn csv_has_a_row_per_posting_with_exact_amounts() {
    let l = ledger();
    let csv = export::csv(&l);
    let path = repo().join("fixtures/ledger/synthetic-small.csv");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &csv).unwrap();
    }
    assert_eq!(
        csv,
        std::fs::read_to_string(&path).unwrap(),
        "UPDATE_GOLDEN=1 to accept"
    );

    let lines: Vec<&str> = csv.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(lines[0], export::COLUMNS.join(","));
    let postings: usize = l.entries.iter().map(|e| e.postings.len()).sum();
    assert_eq!(lines.len(), postings + 1);
    assert!(
        lines.iter().any(|l| l.contains(",-250.85,USD,")),
        "the refund"
    );
}

#[test]
fn csv_quotes_and_defuses_free_text() {
    let mut l = ledger();
    l.entries[0].payee = "=cmd|' /C calc'!A0".into();
    l.entries[0].narration = "Paper, \"A4\"".into();
    let csv = export::csv(&l);
    let row = csv.split("\r\n").nth(1).unwrap();
    assert!(row.contains(",'=cmd|' /C calc'!A0,"), "{row}");
    assert!(row.contains(",\"Paper, \"\"A4\"\"\","), "{row}");
}

/// The stored ZIP's files, checked against their CRCs.
fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let eocd = bytes.len() - 22;
    assert_eq!(u32_at(eocd), 0x0605_4b50, "end of central directory");
    let count = u16_at(eocd + 10);
    let mut at = u32_at(eocd + 16) as usize;
    let mut files = Vec::new();
    for _ in 0..count {
        assert_eq!(u32_at(at), 0x0201_4b50);
        let crc = u32_at(at + 16);
        let size = u32_at(at + 20) as usize;
        let name_len = u16_at(at + 28);
        let offset = u32_at(at + 42) as usize;
        let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec()).unwrap();
        assert_eq!(u32_at(offset), 0x0403_4b50);
        let data_at = offset + 30 + u16_at(offset + 26) + u16_at(offset + 28);
        let data = bytes[data_at..data_at + size].to_vec();
        assert_eq!(crc32(&data), crc, "{name}");
        files.push((name, data));
        at += 46 + name_len;
    }
    files
}

fn crc32(data: &[u8]) -> u32 {
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

#[test]
fn xlsx_is_a_deterministic_workbook_with_numeric_amounts() {
    let l = ledger();
    let bytes = export::xlsx(&l);
    assert_eq!(bytes, export::xlsx(&l), "same ledger, same bytes");

    let files = unzip(&bytes);
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/worksheets/sheet1.xml"
        ]
    );
    let sheet = String::from_utf8(files[4].1.clone()).unwrap();
    let postings: usize = l.entries.iter().map(|e| e.postings.len()).sum();
    assert_eq!(sheet.matches("<row ").count(), postings + 1);
    assert!(sheet.contains(r#"<c r="E2"><v>"#), "amounts are numbers");
    assert!(sheet.contains("<v>-250.85</v>"), "the refund, exactly");
    assert!(sheet.contains(r#"<sheetData><row r="1"><c r="A1" t="inlineStr">"#));
}

#[test]
fn xlsx_escapes_text() {
    let mut l = ledger();
    l.entries[0].payee = "Tom & Jerry <Ltd>".into();
    let sheet = String::from_utf8(unzip(&export::xlsx(&l))[4].1.clone()).unwrap();
    assert!(sheet.contains("Tom &amp; Jerry &lt;Ltd&gt;"));
}
