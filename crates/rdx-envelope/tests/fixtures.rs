//! Envelope fixtures for skills and later pieces (the Typst renderer, the PWA).
//!
//! `UPDATE_GOLDEN=1 cargo test -p rdx-envelope --test fixtures` rewrites
//! `fixtures/synthetic/small/envelopes/` from the small scenario, sealed with the published
//! synthetic key and a fixed salt generator; review the diff before committing it.

use std::path::{Path, PathBuf};

use bc_rand::make_fake_random_number_generator;
use rdx_envelope::{from_ur, public_key_to_ur, redact, seal, synth, to_ur, verify};
use rdx_model::synth::{Config, Scenario};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/small/envelopes")
}

fn golden(path: &Path, actual: &str) {
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(path).unwrap_or_else(|_| {
        panic!(
            "{} is missing; run with UPDATE_GOLDEN=1 to create it",
            path.display()
        )
    });
    assert!(
        expected == actual,
        "{} differs from what sealing produces; run with UPDATE_GOLDEN=1 and review the diff",
        path.display()
    );
}

#[test]
fn committed_envelopes_match_what_sealing_produces() {
    let scenario = Scenario::generate(&Config::small());
    golden(
        &dir().join("signer.ur"),
        &format!("{}\n", public_key_to_ur(&synth::verifier())),
    );
    for st in &scenario.statements {
        let sealed = seal(
            st,
            &synth::signer(),
            &mut make_fake_random_number_generator(),
        )
        .unwrap();
        let redacted = redact(&sealed, &scenario.matches).unwrap();
        golden(
            &dir().join(format!("{}.full.ur", st.id)),
            &format!("{}\n", to_ur(&sealed)),
        );
        golden(
            &dir().join(format!("{}.redacted.ur", st.id)),
            &format!("{}\n", to_ur(&redacted.envelope)),
        );
    }
}

#[test]
fn committed_redacted_envelopes_verify() {
    let scenario = Scenario::generate(&Config::small());
    for st in &scenario.statements {
        let path = dir().join(format!("{}.redacted.ur", st.id));
        let ur = std::fs::read_to_string(&path).unwrap();
        let view = verify(&from_ur(&ur).unwrap(), &synth::verifier()).unwrap();
        assert_eq!(
            view.summary,
            st.redaction_summary(&scenario.matches).unwrap()
        );
        assert!(view.redacted_rows > 0, "{}", path.display());
    }
}
