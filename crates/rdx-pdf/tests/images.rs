//! Page images: what a scan is made of, for OCR (piece 11).

use rdx_pdf::write::{PAGE_HEIGHT, PAGE_WIDTH, Pdf};
use rdx_pdf::{ImageData, page_images, text};

/// A PDF assembled by hand, so tests can use features the fixture writer doesn't.
fn hand_built(objects: &[&[u8]]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    for (i, body) in objects.iter().enumerate() {
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(b"trailer\n<< /Root 1 0 R >>\n%%EOF\n");
    out
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

#[test]
fn a_written_scan_gives_back_its_images_and_where_they_sit() {
    let gray = vec![0u8, 64, 128, 255, 255, 128, 64, 0];
    // Not a real JPEG: the reader passes JPEG data through without decoding it.
    let jpeg = b"\xff\xd8\xff\xe0 pretend jpeg \xff\xd9".to_vec();
    let mut pdf = Pdf::new();
    pdf.page().image(
        72.0,
        100.0,
        200.0,
        100.0,
        4,
        2,
        &ImageData::Gray8(gray.clone()),
    );
    pdf.page().image(
        0.0,
        0.0,
        PAGE_WIDTH,
        PAGE_HEIGHT,
        3,
        5,
        &ImageData::Jpeg(jpeg.clone()),
    );
    let bytes = pdf.finish();

    assert!(text(&bytes).unwrap().is_empty(), "a scan has no text layer");
    let pages = page_images(&bytes).unwrap();
    assert_eq!(pages.len(), 2);

    let first = &pages[0];
    assert_eq!((first.width, first.height), (PAGE_WIDTH, PAGE_HEIGHT));
    assert_eq!(first.images.len(), 1);
    let img = &first.images[0];
    assert_eq!(
        (img.x, img.y, img.width, img.height),
        (72.0, 100.0, 200.0, 100.0)
    );
    assert_eq!((img.pixels_wide, img.pixels_high), (4, 2));
    assert_eq!(img.data, ImageData::Gray8(gray));

    let second = &pages[1].images[0];
    assert_eq!((second.pixels_wide, second.pixels_high), (3, 5));
    assert_eq!(second.data, ImageData::Jpeg(jpeg));
    assert_eq!(second.width, PAGE_WIDTH);
}

#[test]
fn rgb_images_and_inherited_media_boxes() {
    let rgb = [255u8, 0, 0, 0, 255, 0];
    let pdf = hand_built(&[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 595 842] >>",
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im0 5 0 R >> >> >>",
        &stream("", b"q 595 0 0 842 0 0 cm /Im0 Do Q"),
        &stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &rgb,
        ),
    ]);
    let pages = page_images(&pdf).unwrap();
    assert_eq!((pages[0].width, pages[0].height), (595.0, 842.0));
    assert_eq!(pages[0].images[0].data, ImageData::Rgb8(rgb.to_vec()));
}

#[test]
fn one_bit_scans_become_gray_and_images_inside_forms_are_found() {
    // A 1-bit fax-style image, 10 pixels wide: each row is padded to 2 bytes. 0 is black.
    let rows = [0b1010_1010u8, 0b1100_0000, 0b0000_0000, 0b0100_0000];
    let pdf = hand_built(&[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /XObject << /Fm0 5 0 R >> >> >>",
        &stream("", b"q 1 0 0 1 10 20 cm /Fm0 Do Q"),
        &stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources << /XObject << /Im0 6 0 R >> >>",
            b"q 100 0 0 50 0 0 cm /Im0 Do Q",
        ),
        &stream(
            "/Type /XObject /Subtype /Image /Width 10 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 1",
            &rows,
        ),
    ]);
    let img = &page_images(&pdf).unwrap()[0].images[0];
    assert_eq!(
        (img.x, img.y, img.width, img.height),
        (10.0, 20.0, 100.0, 50.0)
    );
    let ImageData::Gray8(px) = &img.data else {
        panic!("expected gray pixels, got {:?}", img.data)
    };
    assert_eq!(
        px,
        &[
            255, 0, 255, 0, 255, 0, 255, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255
        ]
    );
}

#[test]
fn encodings_ocr_cannot_read_are_named() {
    let pdf = hand_built(&[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im0 5 0 R >> >> >>",
        &stream("", b"q 612 0 0 792 0 0 cm /Im0 Do Q"),
        &stream(
            "/Type /XObject /Subtype /Image /Width 8 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode",
            b"\x00\x01",
        ),
    ]);
    let img = &page_images(&pdf).unwrap()[0].images[0];
    match &img.data {
        ImageData::Unsupported(why) => assert!(why.contains("CCITTFaxDecode"), "{why}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn a_text_pdf_has_no_images() {
    let mut pdf = Pdf::new();
    pdf.page().text(
        rdx_pdf::write::Style::Regular,
        10.0,
        72.0,
        700.0,
        "Total 4.50",
    );
    let pages = page_images(&pdf.finish()).unwrap();
    assert_eq!(pages.len(), 1);
    assert!(pages[0].images.is_empty());
}
