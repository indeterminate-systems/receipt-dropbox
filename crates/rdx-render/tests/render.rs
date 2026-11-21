//! The viewable statement: what an accountant sees, rendered by Typst from a verified, redacted
//! envelope. Text is read back from the PDF with this repo's own reader (`rdx-pdf`), so the tests
//! check what a reader of the PDF gets, not what the template meant to print.

use std::path::{Path, PathBuf};

use rdx_envelope::{from_ur, public_key_from_ur, verify};
use rdx_render::{Error, render_envelope};

fn fixture(name: &str) -> String {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/small/envelopes");
    std::fs::read_to_string(path.join(name)).unwrap()
}

fn signer() -> bc_components::SigningPublicKey {
    public_key_from_ur(fixture("signer.ur").trim()).unwrap()
}

fn redacted() -> bc_envelope::Envelope {
    from_ur(fixture("st-2026-03.s-0001.redacted.ur").trim()).unwrap()
}

fn full() -> bc_envelope::Envelope {
    from_ur(fixture("st-2026-03.s-0001.full.ur").trim()).unwrap()
}

/// The PDF's text, one line per text line, pages separated by form feeds.
fn text_of(pdf: &[u8]) -> String {
    rdx_pdf::text(pdf)
        .expect("rdx-pdf reads Typst's PDFs")
        .to_string()
}

#[test]
fn shows_matched_lines_redacted_rows_balances_and_root() {
    let envelope = redacted();
    let view = verify(&envelope, &signer()).unwrap();
    let out = render_envelope(&envelope, &signer()).unwrap();
    assert!(out.pdf.starts_with(b"%PDF-"), "a PDF");
    assert_eq!(out.view, view, "renders exactly what verification read");
    let text = text_of(&out.pdf);

    for line in &view.lines {
        assert!(
            text.contains(&line.description),
            "shown line {}:\n{text}",
            line.id
        );
        assert!(
            text.contains(&line.amount.decimal()),
            "amount of {}:\n{text}",
            line.id
        );
    }
    assert_eq!(
        text.matches("Personal, redacted").count(),
        view.redacted_rows,
        "one row per elided line:\n{text}"
    );
    let s = &view.summary;
    for (what, money) in [
        ("opening", s.opening),
        ("closing", s.closing),
        ("shown", s.shown),
        ("redacted", s.redacted),
    ] {
        assert!(text.contains(&money.decimal()), "{what} {money}:\n{text}");
    }
    assert!(text.contains("Total redacted"), "{text}");
    assert!(text.contains(view.statement.as_str()), "{text}");
    // Long digests wrap, so look for the hex without line breaks.
    let flat: String = text.split_whitespace().collect();
    assert!(flat.contains(view.root.as_str()), "root digest:\n{text}");
}

#[test]
fn elided_lines_leave_no_trace() {
    let all = verify(&full(), &signer()).unwrap();
    let out = render_envelope(&redacted(), &signer()).unwrap();
    let shown: Vec<_> = out.view.lines.iter().map(|l| &l.id).collect();
    let text = text_of(&out.pdf);
    let hidden: Vec<_> = all
        .lines
        .iter()
        .filter(|l| !shown.contains(&&l.id))
        .collect();
    assert!(!hidden.is_empty(), "the fixture redacts something");
    for line in hidden {
        assert!(
            !text.contains(&line.description),
            "{} leaked:\n{text}",
            line.id
        );
    }
}

#[test]
fn carries_the_envelope_for_verification() {
    let ur = fixture("st-2026-03.s-0001.redacted.ur");
    let out = render_envelope(&redacted(), &signer()).unwrap();
    assert!(out.qr, "a statement this size fits one QR code");
    let flat: String = text_of(&out.pdf).split_whitespace().collect();
    assert!(
        flat.contains(ur.trim()),
        "the UR is printed for transcription"
    );
}

#[test]
fn refuses_an_envelope_the_key_did_not_sign() {
    let other = rdx_envelope::synth::signer_from_seed(b"someone else")
        .public_key()
        .unwrap();
    let err = render_envelope(&redacted(), &other).unwrap_err();
    assert!(matches!(err, Error::Envelope(_)), "{err}");
}

#[test]
fn same_envelope_same_bytes() {
    let a = render_envelope(&redacted(), &signer()).unwrap();
    let b = render_envelope(&redacted(), &signer()).unwrap();
    assert_eq!(a.pdf, b.pdf, "no timestamps or random ids");
}
