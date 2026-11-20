//! Statements as signed envelopes: sealing, redaction by elision, and verification.

use bc_envelope::prelude::*;
use bc_rand::make_fake_random_number_generator;
use rdx_envelope::{Error, from_ur, redact, root_digest, seal, synth, to_ur, verify};
use rdx_model::synth::{Config, Scenario};
use rdx_model::*;

fn scenario() -> Scenario {
    Scenario::generate(&Config::small())
}

fn first(s: &Scenario) -> &Statement {
    &s.statements[0]
}

fn matches_for(s: &Scenario, st: &Statement) -> Vec<Match> {
    s.matches
        .iter()
        .filter(|m| m.line.statement == st.id)
        .cloned()
        .collect()
}

#[test]
fn a_sealed_statement_verifies_and_shows_every_line() {
    let s = scenario();
    let st = first(&s);
    let signer = synth::signer();
    let sealed = seal(st, &signer, &mut make_fake_random_number_generator()).unwrap();

    let view = verify(&sealed, &synth::verifier()).unwrap();
    assert_eq!(view.statement, st.id);
    assert_eq!(view.supporter, st.supporter);
    assert_eq!(view.account, st.account);
    assert_eq!(view.period, st.period);
    assert_eq!(view.lines, st.lines, "every line, in statement order");
    assert_eq!(view.redacted_rows, 0);
    assert_eq!(view.summary.opening, st.opening);
    assert_eq!(view.summary.closing, st.closing);
    assert_eq!(view.summary.redacted, Money::zero(st.currency));
}

#[test]
fn redaction_elides_unmatched_lines_and_keeps_the_root_digest() {
    let s = scenario();
    let st = first(&s);
    let matches = matches_for(&s, st);
    let sealed = seal(
        st,
        &synth::signer(),
        &mut make_fake_random_number_generator(),
    )
    .unwrap();

    let redacted = redact(&sealed, &matches).unwrap();
    assert_eq!(root_digest(&redacted.envelope), root_digest(&sealed));

    let view = verify(&redacted.envelope, &synth::verifier()).unwrap();
    let classes = st.classify(&matches);
    let org: Vec<_> = st
        .lines
        .iter()
        .zip(&classes)
        .filter(|(_, c)| **c == LineClass::Org)
        .map(|(l, _)| l.clone())
        .collect();
    assert!(
        !org.is_empty() && org.len() < st.lines.len(),
        "fixture has both kinds"
    );
    assert_eq!(view.lines, org);
    assert_eq!(view.redacted_rows, st.lines.len() - org.len());
    assert_eq!(redacted.shown, org.len());
    assert_eq!(redacted.redacted, view.redacted_rows);

    // The single redacted figure makes the statement reconcile, and agrees with the model.
    assert_eq!(view.summary, st.redaction_summary(&matches).unwrap());
    let total = view
        .summary
        .opening
        .checked_add(view.summary.shown)
        .and_then(|t| t.checked_add(view.summary.redacted))
        .unwrap();
    assert_eq!(total, view.summary.closing);
}

#[test]
fn redacted_lines_leave_no_trace_of_their_contents() {
    let s = scenario();
    let st = first(&s);
    let sealed = seal(
        st,
        &synth::signer(),
        &mut make_fake_random_number_generator(),
    )
    .unwrap();
    let redacted = redact(&sealed, &[]).unwrap();
    let ur = to_ur(&redacted.envelope);
    let bytes = from_ur(&ur).unwrap().to_cbor_data();
    for line in &st.lines {
        let description = line.description.as_bytes();
        assert!(
            !bytes.windows(description.len()).any(|w| w == description),
            "`{}` leaked",
            line.description
        );
    }
    let view = verify(&redacted.envelope, &synth::verifier()).unwrap();
    assert!(view.lines.is_empty());
    assert_eq!(view.redacted_rows, st.lines.len());
}

#[test]
fn lines_are_salted_so_an_elided_line_cannot_be_guessed() {
    // Two seals of the same statement with different salt share no line digests: someone who
    // guesses a line's contents can't confirm the guess against an elided digest.
    let s = scenario();
    let st = first(&s);
    let a = seal(
        st,
        &synth::signer(),
        &mut make_fake_random_number_generator(),
    )
    .unwrap();
    let mut other = make_fake_random_number_generator();
    let _ = bc_rand::rng_random_data(&mut other, 7);
    let b = seal(st, &synth::signer(), &mut other).unwrap();
    let da = rdx_envelope::line_digests(&a).unwrap();
    let db = rdx_envelope::line_digests(&b).unwrap();
    assert_eq!(da.len(), st.lines.len());
    assert!(da.iter().all(|d| !db.contains(d)));
}

#[test]
fn partly_matched_lines_are_redacted_and_flagged() {
    let s = scenario();
    let st = first(&s);
    let line = st.lines.iter().find(|l| l.amount.is_negative()).unwrap();
    let half = Money::new(line.amount.minor().abs() / 2, st.currency);
    let partial = Match {
        line: LineRef {
            statement: st.id.clone(),
            line: line.id.clone(),
        },
        receipt: ReceiptId::new("r-partial").unwrap(),
        amount: half,
        status: MatchStatus::Confirmed,
    };
    let sealed = seal(
        st,
        &synth::signer(),
        &mut make_fake_random_number_generator(),
    )
    .unwrap();
    let redacted = redact(&sealed, &[partial]).unwrap();
    assert_eq!(redacted.review, vec![line.id.clone()]);
    let view = verify(&redacted.envelope, &synth::verifier()).unwrap();
    assert!(view.lines.is_empty());
}

#[test]
fn a_tampered_or_foreign_envelope_fails_verification() {
    let s = scenario();
    let st = first(&s);
    let sealed = seal(
        st,
        &synth::signer(),
        &mut make_fake_random_number_generator(),
    )
    .unwrap();

    // Signed by someone else.
    let other = synth::signer_from_seed(b"someone else entirely");
    let forged = seal(st, &other, &mut make_fake_random_number_generator()).unwrap();
    assert!(matches!(
        verify(&forged, &synth::verifier()),
        Err(Error::Signature)
    ));

    // A line changed after signing: reseal the content without the signature, then graft the
    // original signature on.
    let mut changed = st.clone();
    changed.lines[0].amount = Money::new(changed.lines[0].amount.minor() - 1, st.currency);
    changed.closing = Money::new(changed.closing.minor() - 1, st.currency);
    let resealed = seal(&changed, &other, &mut make_fake_random_number_generator()).unwrap();
    let grafted = resealed
        .subject()
        .add_assertion_envelope(sealed.assertions()[0].clone())
        .unwrap();
    assert!(matches!(
        verify(&grafted, &synth::verifier()),
        Err(Error::Signature)
    ));
}

#[test]
fn only_balanced_statements_are_sealed() {
    // The redacted figure is derived from the signed balances, so it is only right if the
    // statement balanced when it was signed.
    let s = scenario();
    let mut st = first(&s).clone();
    st.closing = Money::new(st.closing.minor() + 1, st.currency);
    let err = seal(
        &st,
        &synth::signer(),
        &mut make_fake_random_number_generator(),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Statement(_)), "{err}");
}

#[test]
fn ur_round_trips_and_rejects_garbage() {
    let s = scenario();
    let sealed = seal(
        first(&s),
        &synth::signer(),
        &mut make_fake_random_number_generator(),
    )
    .unwrap();
    let ur = to_ur(&sealed);
    assert!(ur.starts_with("ur:envelope/"));
    assert_eq!(root_digest(&from_ur(&ur).unwrap()), root_digest(&sealed));
    let err = from_ur("ur:envelope/nonsense").unwrap_err();
    assert!(matches!(err, Error::Ur(_)), "{err}");
}

#[test]
fn an_envelope_that_is_not_a_statement_is_rejected() {
    let signed = Envelope::new("hello").sign(&synth::signer());
    let err = verify(&signed, &synth::verifier()).unwrap_err();
    assert!(matches!(err, Error::NotAStatement(_)), "{err}");
}
