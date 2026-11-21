//! Reading text layers: PDFs written by the fixture writer, hand-built PDFs that use the harder
//! features (composite fonts with ToUnicode maps, object streams, TJ kerning), and bad input.

use rdx_pdf::write::{Pdf, Style};
use rdx_pdf::{PdfError, text};

fn spans(line: &rdx_pdf::Line) -> Vec<&str> {
    line.spans.iter().map(|s| s.text.as_str()).collect()
}

#[test]
fn columns_become_spans_and_lines_read_top_to_bottom() {
    let mut pdf = Pdf::new();
    let page = pdf.page();
    page.text(Style::Bold, 12.0, 72.0, 720.0, "Example Bank");
    page.text(Style::Regular, 9.0, 72.0, 600.0, "03/03");
    page.text(
        Style::Regular,
        9.0,
        120.0,
        600.0,
        "LANTERN CAFE CATERING #7435",
    );
    page.text_right(Style::Regular, 9.0, 420.0, 600.0, "289.74");
    page.text_right(Style::Regular, 9.0, 540.0, 600.0, "454.81");
    page.text(Style::Regular, 9.0, 72.0, 588.0, "03/11");
    let t = text(&pdf.finish()).unwrap();

    let lines = &t.pages[0].lines;
    assert_eq!(lines.len(), 3);
    assert_eq!(spans(&lines[0]), ["Example Bank"]);
    assert_eq!(
        spans(&lines[1]),
        ["03/03", "LANTERN CAFE CATERING #7435", "289.74", "454.81"]
    );
    // Right edges line up with where the writer right-aligned them.
    assert!((lines[1].spans[2].x_end - 420.0).abs() < 0.2);
    assert!((lines[1].spans[3].x_end - 540.0).abs() < 0.2);
    assert_eq!(lines[1].y, 600.0);
    assert_eq!(
        t.to_string(),
        "Example Bank\n03/03  LANTERN CAFE CATERING #7435  289.74  454.81\n03/11\n"
    );
}

#[test]
fn pages_stay_separate_and_latin_1_survives() {
    let mut pdf = Pdf::new();
    pdf.page()
        .text(Style::Regular, 10.0, 72.0, 700.0, "Cafetería Niño (centro)");
    pdf.page()
        .text(Style::Regular, 10.0, 72.0, 700.0, "Página 2");
    let t = text(&pdf.finish()).unwrap();
    assert_eq!(t.pages.len(), 2);
    assert_eq!(t.to_string(), "Cafetería Niño (centro)\n\u{c}Página 2\n");
}

/// A PDF assembled by hand, so tests can use features the fixture writer doesn't.
fn hand_built(objects: &[&[u8]]) -> Vec<u8> {
    let numbered: Vec<(usize, &[u8])> = objects
        .iter()
        .enumerate()
        .map(|(i, b)| (i + 1, *b))
        .collect();
    hand_built_numbered(&numbered)
}

fn hand_built_numbered(objects: &[(usize, &[u8])]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    for (num, body) in objects {
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(b"trailer\n<< /Root 1 0 R >>\n%%EOF\n");
    out
}

fn stream(data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

#[test]
fn composite_fonts_use_their_to_unicode_map_and_widths() {
    // Glyph ids 1..4 are arbitrary, as in a subset font; only the ToUnicode map says what they are.
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
        1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
        3 beginbfchar <0001> <0054> <0002> <006F> <0003> <0074> endbfchar\n\
        1 beginbfrange <0004> <0005> [<00E1> <006C>] endbfrange\n\
        endcmap CMapName currentdict /CMap defineresource pop end end";
    // "Total" kerned with TJ, then a big TJ gap before the amount: two spans.
    let content = b"BT /F1 10 Tf 72 700 Td [<0001> 20 <0002> <0003> -10 <0004> <0005>] TJ \
        [-4000 <0001>] TJ ET";
    let pdf = hand_built(&[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 4 0 R >> >> /Contents 7 0 R >>",
        b"<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+Sans /Encoding /Identity-H \
           /DescendantFonts [5 0 R] /ToUnicode 6 0 R >>",
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Sans /DW 500 /W [1 [600 550] 3 5 300] >>",
        &stream(cmap),
        &stream(content),
    ]);
    let t = text(&pdf).unwrap();
    assert_eq!(spans(&t.pages[0].lines[0]), ["Totál", "T"]);
}

#[test]
fn objects_inside_object_streams_are_found() {
    // The page and its font live in a compressed object stream, as modern producers write them.
    let page =
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 4 0 R >> >> /Contents 6 0 R >> ";
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
    let header = format!("3 0 4 {} ", page.len());
    let first = header.len();
    let body = format!("{header}{page}{font}").into_bytes();
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&body, 6);
    let mut objstm = format!(
        "<< /Type /ObjStm /N 2 /First {first} /Length {} /Filter /FlateDecode >>\nstream\n",
        compressed.len()
    )
    .into_bytes();
    objstm.extend_from_slice(&compressed);
    objstm.extend_from_slice(b"\nendstream");

    // Objects 3 and 4 exist only inside the object stream (object 5).
    let pdf = hand_built_numbered(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (5, &objstm),
        (6, &stream(b"BT /F1 9 Tf 72 700 Td (Opening balance) Tj ET")),
    ]);
    let t = text(&pdf).unwrap();
    assert_eq!(t.to_string(), "Opening balance\n");
}

#[test]
fn a_scan_has_an_empty_text_layer() {
    let pdf = hand_built(&[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        &stream(b"q 612 0 0 792 0 0 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x00 EI Q"),
    ]);
    let t = text(&pdf).unwrap();
    assert!(t.is_empty());
    assert_eq!(t.pages.len(), 1);
}

#[test]
fn bad_input_says_what_is_wrong() {
    assert_eq!(text(b"date,amount\n"), Err(PdfError::NotPdf));
    // Password-protected PDFs are tested in tests/locked.rs; this one's protection is broken.
    let encrypted = hand_built(&[b"<< /Type /Catalog >>"]);
    let encrypted = String::from_utf8_lossy(&encrypted).replace(
        "<< /Root 1 0 R >>",
        "<< /Root 1 0 R /Encrypt << /Filter /Standard /V 2 /R 3 >> >>",
    );
    assert!(matches!(
        text(encrypted.as_bytes()),
        Err(PdfError::Malformed(_))
    ));
    assert!(matches!(
        text(b"%PDF-1.4\n%%EOF"),
        Err(PdfError::Malformed(_))
    ));
}

#[test]
fn garbage_after_the_header_never_panics() {
    let mut seed = 7u64;
    for len in [10usize, 100, 1000, 5000] {
        let mut bytes = b"%PDF-1.4\n1 0 obj << /Type /Page /Contents 2 0 R >> endobj 2 0 obj << /Length 9 >> stream\n".to_vec();
        for _ in 0..len {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            bytes.push((seed >> 33) as u8);
        }
        let _ = text(&bytes);
    }
}
