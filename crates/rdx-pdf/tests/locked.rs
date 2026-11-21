//! Password-protected PDFs, as Mexican banks send statements: each fixture in `tests/fixtures`
//! was encrypted by qpdf (see the README there) with one of the PDF standard security handler's
//! revisions, so these tests check the decryption against another implementation.

use std::path::Path;

use rdx_pdf::write::{Pdf, Style};
use rdx_pdf::{PdfError, needs_password, text, text_with_password};

const USER: &str = "XAXX010101000";
const OWNER: &str = "owner-1234";

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn lines(t: &rdx_pdf::Text) -> Vec<String> {
    t.lines().map(|l| l.text()).collect()
}

const EXPECTED: [&str; 3] = [
    "Banco Ejemplo",
    "Estado de cuenta",
    "2026-03-03  LANTERN CAFE CATERING",
];

const LOCKED: [&str; 4] = ["rc4-40.pdf", "rc4-128.pdf", "aes-128.pdf", "aes-256.pdf"];

#[test]
fn each_revision_opens_with_the_user_password() {
    for name in LOCKED {
        let bytes = fixture(name);
        let t = text_with_password(&bytes, USER).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(lines(&t), EXPECTED, "{name}");
    }
}

#[test]
fn each_revision_opens_with_the_owner_password_too() {
    for name in LOCKED {
        let t = text_with_password(&fixture(name), OWNER).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(lines(&t), EXPECTED, "{name}");
    }
}

#[test]
fn without_a_password_a_locked_pdf_says_it_needs_one() {
    for name in LOCKED {
        let bytes = fixture(name);
        assert_eq!(text(&bytes), Err(PdfError::PasswordRequired), "{name}");
        assert_eq!(
            text_with_password(&bytes, ""),
            Err(PdfError::PasswordRequired),
            "{name}"
        );
        assert!(needs_password(&bytes), "{name}");
    }
}

#[test]
fn a_wrong_password_is_refused() {
    for name in LOCKED {
        assert_eq!(
            text_with_password(&fixture(name), "XAXX010101001"),
            Err(PdfError::WrongPassword),
            "{name}"
        );
    }
}

#[test]
fn a_pdf_locked_only_against_editing_opens_without_asking() {
    // An empty user password with an owner password: viewers open it without asking, and so
    // does this reader. Many bank PDFs are protected only this way.
    let bytes = fixture("owner-only.pdf");
    assert!(!needs_password(&bytes));
    assert_eq!(lines(&text(&bytes).unwrap()), EXPECTED);
    assert_eq!(
        lines(&text_with_password(&bytes, "anything").unwrap()),
        EXPECTED
    );
}

#[test]
fn an_unlocked_pdf_ignores_the_password() {
    let mut pdf = Pdf::new();
    pdf.page()
        .text(Style::Regular, 10.0, 72.0, 700.0, "Banco Ejemplo");
    let bytes = pdf.finish();
    assert!(!needs_password(&bytes));
    assert_eq!(
        lines(&text_with_password(&bytes, "whatever").unwrap()),
        ["Banco Ejemplo"]
    );
}

#[test]
fn the_fixture_writer_locks_with_aes_256() {
    let mut pdf = Pdf::new();
    let page = pdf.page();
    page.text(Style::Bold, 12.0, 72.0, 720.0, "Banco Ejemplo");
    page.text(Style::Regular, 10.0, 72.0, 700.0, "Saldo inicial (MXN)");
    let bytes = pdf.finish_locked(USER);

    assert!(needs_password(&bytes));
    assert_eq!(text(&bytes), Err(PdfError::PasswordRequired));
    assert_eq!(
        text_with_password(&bytes, "nope"),
        Err(PdfError::WrongPassword)
    );
    assert_eq!(
        lines(&text_with_password(&bytes, USER).unwrap()),
        ["Banco Ejemplo", "Saldo inicial (MXN)"]
    );
    // Same input, same bytes: fixtures made this way keep their digests.
    let mut again = Pdf::new();
    let page = again.page();
    page.text(Style::Bold, 12.0, 72.0, 720.0, "Banco Ejemplo");
    page.text(Style::Regular, 10.0, 72.0, 700.0, "Saldo inicial (MXN)");
    assert_eq!(again.finish_locked(USER), bytes);
}

#[test]
fn a_locked_scan_gives_its_images_with_the_password() {
    let mut pdf = Pdf::new();
    pdf.page().image(
        72.0,
        600.0,
        100.0,
        50.0,
        2,
        1,
        &rdx_pdf::ImageData::Gray8(vec![0, 255]),
    );
    let bytes = pdf.finish_locked(USER);
    assert_eq!(
        rdx_pdf::page_images(&bytes),
        Err(PdfError::PasswordRequired)
    );
    let pages = rdx_pdf::page_images_with_password(&bytes, USER).unwrap();
    assert_eq!(pages[0].images.len(), 1);
}

#[test]
fn other_security_handlers_are_named() {
    let mut bytes = fixture("aes-256.pdf");
    let at = bytes
        .windows(9)
        .position(|w| w == b"/Standard")
        .expect("the fixture uses the standard handler");
    bytes[at..at + 9].copy_from_slice(b"/AdobePub");
    match text_with_password(&bytes, USER) {
        Err(PdfError::Unsupported(why)) => assert!(why.contains("AdobePub"), "{why}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}
