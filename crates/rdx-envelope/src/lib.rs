//! Statements as signed Gordian Envelopes, and the redacted view accountants verify.
//!
//! A statement is sealed once, on the supporter's device, as one envelope:
//!
//! ```text
//! {
//!     "st-2026-03.s-0001" [
//!         'isA': "rdx.statement.v1"
//!         "supporter": "s-0001"
//!         "institution": …   "accountMask": …   "currency": "USD"
//!         "periodStart": …   "periodEnd": …
//!         "opening": 120000  "closing": 98734          (minor units)
//!         "original": "sha256:…"                        (encrypted original, when stored)
//!         "lineCount": 9
//!         "line": {                                     (one per line, each salted)
//!             "l01" [ "position": 0  "date": …  "description": …  "amount": -28974
//!                     "reference": …  'salt': Salt ]
//!         }
//!         …
//!     ]
//! } [ 'signed': Signature ]
//! ```
//!
//! [`redact`] elides every `"line"` assertion without a confirmed match covering its whole
//! amount. Elision replaces an assertion by its digest, so the root digest and the signature stay
//! valid; the salt means nobody can confirm a guess at an elided line by hashing it. [`verify`]
//! checks the signature and returns what the viewable statement shows: header, shown lines, one
//! "Personal, redacted" row per elided line, and the total redacted, which is the signed closing
//! balance less the signed opening balance and the shown lines. [`seal`] refuses a statement that
//! doesn't balance, so that figure is right whenever the signature is.
//!
//! The crate does no I/O and builds for `wasm32-unknown-unknown` and `wasm32-wasip2`. Salt comes
//! from the caller's random number generator; Ed25519 signing is deterministic, so the same
//! generator gives the same envelope on every platform.

mod ops;
pub mod synth;

use std::collections::HashSet;

use bc_components::{Salt, SigningPrivateKey, SigningPublicKey};
use bc_envelope::prelude::*;
use bc_rand::RandomNumberGenerator;
use rdx_model::{
    AccountRef, Currency, Date, Digest as RdxDigest, LineClass, LineId, Match, Money, MoneyError,
    Period, RedactionSummary, Statement, StatementId, StatementLine, StatementProblem, SupporterId,
};
use serde::{Deserialize, Serialize};

pub use ops::register;

/// The `'isA'` of a statement envelope. A new layout gets a new version.
pub const STATEMENT_TYPE: &str = "rdx.statement.v1";

/// Bytes of salt per line: 128 bits, far beyond guessing.
const SALT_LEN: usize = 16;

mod pred {
    pub const SUPPORTER: &str = "supporter";
    pub const INSTITUTION: &str = "institution";
    pub const ACCOUNT_MASK: &str = "accountMask";
    pub const CURRENCY: &str = "currency";
    pub const PERIOD_START: &str = "periodStart";
    pub const PERIOD_END: &str = "periodEnd";
    pub const OPENING: &str = "opening";
    pub const CLOSING: &str = "closing";
    pub const ORIGINAL: &str = "original";
    pub const LINE_COUNT: &str = "lineCount";
    pub const LINE: &str = "line";
    pub const POSITION: &str = "position";
    pub const DATE: &str = "date";
    pub const DESCRIPTION: &str = "description";
    pub const AMOUNT: &str = "amount";
    pub const REFERENCE: &str = "reference";
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the statement can't be sealed: {}", join(.0))]
    Statement(Vec<StatementProblem>),
    #[error("not an rdx statement envelope: {0}")]
    NotAStatement(String),
    #[error(
        "the signature does not verify with this key: the envelope was changed, or someone else signed it"
    )]
    Signature,
    #[error("not a valid `ur:envelope/…` string: {0}")]
    Ur(String),
    #[error("not a valid `ur:signing-public-key/…` string: {0}")]
    Key(String),
    #[error(transparent)]
    Money(#[from] MoneyError),
}

fn join(problems: &[StatementProblem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

fn not_a_statement(what: impl std::fmt::Display) -> Error {
    Error::NotAStatement(what.to_string())
}

/// Seals a statement: one salted assertion per line, signed with the supporter's key.
pub fn seal(
    statement: &Statement,
    signer: &SigningPrivateKey,
    rng: &mut impl RandomNumberGenerator,
) -> Result<Envelope, Error> {
    statement.validate().map_err(Error::Statement)?;
    let mut e = Envelope::new(statement.id.as_str())
        .add_assertion(known_values::IS_A, STATEMENT_TYPE)
        .add_assertion(pred::SUPPORTER, statement.supporter.as_str())
        .add_assertion(pred::INSTITUTION, statement.account.institution.as_str())
        .add_assertion(pred::ACCOUNT_MASK, statement.account.mask.as_str())
        .add_assertion(pred::CURRENCY, statement.currency.code())
        .add_assertion(pred::PERIOD_START, statement.period.start().to_string())
        .add_assertion(pred::PERIOD_END, statement.period.end().to_string())
        .add_assertion(pred::OPENING, statement.opening.minor())
        .add_assertion(pred::CLOSING, statement.closing.minor())
        .add_assertion(pred::LINE_COUNT, statement.lines.len() as u64);
    if let Some(original) = &statement.original {
        e = e.add_assertion(pred::ORIGINAL, original.as_str());
    }
    for (position, line) in statement.lines.iter().enumerate() {
        let mut l = Envelope::new(line.id.as_str())
            .add_assertion(pred::POSITION, position as u64)
            .add_assertion(pred::DATE, line.date.to_string())
            .add_assertion(pred::DESCRIPTION, line.description.as_str())
            .add_assertion(pred::AMOUNT, line.amount.minor());
        if let Some(reference) = &line.reference {
            l = l.add_assertion(pred::REFERENCE, reference.as_str());
        }
        // A fixed length rather than bc-envelope's size-proportional `add_salt_using`: that one
        // draws the length as a `usize`, so the same generator gives different salt on 32-bit
        // WebAssembly and on 64-bit hosts, and envelopes would differ by platform. An elided
        // line hides its salt's length anyway.
        let salt = Salt::new_with_len_using(SALT_LEN, rng).expect("SALT_LEN is at least 8");
        e = e.add_assertion(pred::LINE, l.add_salt_instance(salt));
    }
    Ok(e.sign(signer))
}

/// A redacted envelope and what it shows.
#[derive(Debug, Clone)]
pub struct Redaction {
    pub envelope: Envelope,
    /// Lines left visible: confirmed matches cover their whole amount.
    pub shown: usize,
    /// Lines elided.
    pub redacted: usize,
    /// Elided lines that confirmed matches cover only in part: someone should look at them.
    pub review: Vec<LineId>,
}

/// Elides every line that confirmed matches don't fully cover. The root digest is unchanged.
///
/// Works on the supporter's full envelope, which only their device holds in the clear. The
/// signature is not checked here (the supporter's own device made it); [`verify`] checks it.
pub fn redact(signed: &Envelope, matches: &[Match]) -> Result<Redaction, Error> {
    let content = signed.try_unwrap().map_err(not_a_statement)?;
    let statement = decode(&content)?;
    let full = statement
        .statement()
        .ok_or_else(|| not_a_statement("it is already redacted"))?;
    let classes = full.classify(matches);
    let mut elide = HashSet::new();
    let mut review = Vec::new();
    for (line, class) in statement.lines.iter().zip(&classes) {
        match class {
            LineClass::Org => {}
            LineClass::PartlyMatched => {
                review.push(line.line.id.clone());
                elide.insert(line.assertion);
            }
            LineClass::Personal => {
                elide.insert(line.assertion);
            }
        }
    }
    Ok(Redaction {
        envelope: signed.elide_removing_set(&elide),
        shown: statement.lines.len() - elide.len(),
        redacted: elide.len(),
        review,
    })
}

/// What a viewable statement shows, read from an envelope whose signature verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementView {
    /// The envelope's root digest: the same for the full and every redacted envelope.
    pub root: RdxDigest,
    pub statement: StatementId,
    pub supporter: SupporterId,
    pub account: AccountRef,
    pub currency: Currency,
    pub period: Period,
    /// Digest of the encrypted original, when it was stored before sealing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<RdxDigest>,
    /// Shown lines, in statement order.
    pub lines: Vec<StatementLine>,
    /// "Personal, redacted" rows: one per elided line.
    pub redacted_rows: usize,
    pub summary: RedactionSummary,
}

/// Checks the signature and reads what the (possibly redacted) statement shows.
pub fn verify(signed: &Envelope, signer: &SigningPublicKey) -> Result<StatementView, Error> {
    let content = signed.verify(signer).map_err(|e| match e {
        bc_envelope::Error::UnverifiedSignature => Error::Signature,
        other => not_a_statement(other),
    })?;
    let decoded = decode(&content)?;
    let lines: Vec<StatementLine> = decoded.lines.into_iter().map(|l| l.line).collect();
    let shown = Money::sum(decoded.currency, lines.iter().map(|l| &l.amount))?;
    let redacted = decoded
        .closing
        .checked_sub(decoded.opening)?
        .checked_sub(shown)?;
    Ok(StatementView {
        root: root_digest(signed),
        statement: decoded.id,
        supporter: decoded.supporter,
        account: decoded.account,
        currency: decoded.currency,
        period: decoded.period,
        original: decoded.original,
        lines,
        redacted_rows: decoded.elided,
        summary: RedactionSummary {
            opening: decoded.opening,
            closing: decoded.closing,
            shown,
            redacted,
        },
    })
}

/// The envelope's root digest as `sha256:<hex>`. Gordian Envelope digests are SHA-256, so this is
/// the same value the envelope tools print, in this repo's digest notation.
pub fn root_digest(envelope: &Envelope) -> RdxDigest {
    let hex: String = envelope
        .digest()
        .data()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    RdxDigest::parse(&format!("sha256:{hex}")).expect("SHA-256 digests are 32 bytes")
}

/// The digests of a full envelope's line assertions, in statement order.
pub fn line_digests(signed: &Envelope) -> Result<Vec<Digest>, Error> {
    let content = signed.try_unwrap().map_err(not_a_statement)?;
    Ok(decode(&content)?
        .lines
        .into_iter()
        .map(|l| l.assertion)
        .collect())
}

/// UR type names come from the global CBOR tag registry, which starts empty.
fn tags() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(register_tags);
}

pub fn to_ur(envelope: &Envelope) -> String {
    tags();
    envelope.ur_string()
}

pub fn from_ur(ur: &str) -> Result<Envelope, Error> {
    tags();
    Envelope::from_ur_string(ur.trim()).map_err(|e| Error::Ur(e.to_string()))
}

pub fn public_key_to_ur(key: &SigningPublicKey) -> String {
    tags();
    key.ur_string()
}

pub fn public_key_from_ur(ur: &str) -> Result<SigningPublicKey, Error> {
    tags();
    SigningPublicKey::from_ur_string(ur.trim()).map_err(|e| Error::Key(e.to_string()))
}

struct DecodedLine {
    line: StatementLine,
    /// The digest of the `"line": {…}` assertion: what [`redact`] elides.
    assertion: Digest,
}

struct Decoded {
    id: StatementId,
    supporter: SupporterId,
    account: AccountRef,
    currency: Currency,
    period: Period,
    opening: Money,
    closing: Money,
    original: Option<RdxDigest>,
    /// Visible lines, in statement order.
    lines: Vec<DecodedLine>,
    elided: usize,
}

impl Decoded {
    /// The whole statement, if nothing is elided.
    fn statement(&self) -> Option<Statement> {
        (self.elided == 0).then(|| Statement {
            id: self.id.clone(),
            supporter: self.supporter.clone(),
            account: self.account.clone(),
            currency: self.currency,
            period: self.period,
            opening: self.opening,
            closing: self.closing,
            lines: self.lines.iter().map(|l| l.line.clone()).collect(),
            original: self.original.clone(),
        })
    }
}

fn text(e: &Envelope, predicate: &str) -> Result<String, Error> {
    e.extract_object_for_predicate::<String>(predicate)
        .map_err(|err| not_a_statement(format!("`{predicate}`: {err}")))
}

fn int(e: &Envelope, predicate: &str) -> Result<i64, Error> {
    e.extract_object_for_predicate::<i64>(predicate)
        .map_err(|err| not_a_statement(format!("`{predicate}`: {err}")))
}

fn parsed<T, E: std::fmt::Display>(
    predicate: &str,
    value: String,
    parse: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, Error> {
    parse(&value).map_err(|err| not_a_statement(format!("`{predicate}`: {err}")))
}

fn decode(content: &Envelope) -> Result<Decoded, Error> {
    let kind = content
        .extract_object_for_predicate::<String>(known_values::IS_A)
        .map_err(|_| not_a_statement(format!("no `'isA': \"{STATEMENT_TYPE}\"`")))?;
    if kind != STATEMENT_TYPE {
        return Err(not_a_statement(format!(
            "it is a `{kind}`, not a `{STATEMENT_TYPE}`"
        )));
    }
    let id = content
        .extract_subject::<String>()
        .map_err(|err| not_a_statement(format!("subject: {err}")))?;
    let id = parsed("subject", id, |s: &str| StatementId::new(s))?;
    let currency = parsed(
        pred::CURRENCY,
        text(content, pred::CURRENCY)?,
        Currency::new,
    )?;
    let date =
        |p: &str| -> Result<Date, Error> { parsed(p, text(content, p)?, |s| s.parse::<Date>()) };
    let period =
        Period::new(date(pred::PERIOD_START)?, date(pred::PERIOD_END)?).map_err(not_a_statement)?;
    let original = match content.optional_object_for_predicate(pred::ORIGINAL) {
        Ok(Some(o)) => Some(parsed(
            pred::ORIGINAL,
            o.extract_subject::<String>()
                .map_err(|err| not_a_statement(format!("`{}`: {err}", pred::ORIGINAL)))?,
            RdxDigest::parse,
        )?),
        Ok(None) => None,
        Err(err) => return Err(not_a_statement(format!("`{}`: {err}", pred::ORIGINAL))),
    };
    let line_count = usize::try_from(int(content, pred::LINE_COUNT)?)
        .map_err(|err| not_a_statement(format!("`{}`: {err}", pred::LINE_COUNT)))?;

    let mut lines = Vec::new();
    let mut elided = 0;
    for assertion in content.assertions() {
        if assertion.is_elided() {
            elided += 1;
            continue;
        }
        let predicate = assertion
            .try_predicate()
            .and_then(|p| p.extract_subject::<String>());
        if predicate.ok().as_deref() != Some(pred::LINE) {
            continue;
        }
        let object = assertion.try_object().map_err(not_a_statement)?;
        let (position, line) = decode_line(&object, currency)?;
        lines.push((
            position,
            DecodedLine {
                line,
                assertion: assertion.digest(),
            },
        ));
    }
    lines.sort_by_key(|(position, _)| *position);
    // Positions are 0..line_count with the elided ones missing; anything else was not made by
    // `seal`.
    let positions: Vec<usize> = lines.iter().map(|(p, _)| *p).collect();
    if lines.len() + elided != line_count
        || positions.windows(2).any(|w| w[0] == w[1])
        || positions.last().is_some_and(|p| *p >= line_count)
    {
        return Err(not_a_statement(format!(
            "it declares {line_count} lines but holds {} shown and {elided} elided",
            lines.len()
        )));
    }

    Ok(Decoded {
        id,
        supporter: parsed(
            pred::SUPPORTER,
            text(content, pred::SUPPORTER)?,
            |s: &str| SupporterId::new(s),
        )?,
        account: AccountRef {
            institution: text(content, pred::INSTITUTION)?,
            mask: text(content, pred::ACCOUNT_MASK)?,
        },
        currency,
        period,
        opening: Money::new(int(content, pred::OPENING)?, currency),
        closing: Money::new(int(content, pred::CLOSING)?, currency),
        original,
        lines: lines.into_iter().map(|(_, l)| l).collect(),
        elided,
    })
}

fn decode_line(line: &Envelope, currency: Currency) -> Result<(usize, StatementLine), Error> {
    let id = line
        .extract_subject::<String>()
        .map_err(|err| not_a_statement(format!("line subject: {err}")))?;
    let id = parsed("line subject", id, |s: &str| LineId::new(s))?;
    let position = usize::try_from(int(line, pred::POSITION)?)
        .map_err(|err| not_a_statement(format!("line `{id}` `position`: {err}")))?;
    let reference = match line.optional_object_for_predicate(pred::REFERENCE) {
        Ok(Some(r)) => Some(
            r.extract_subject::<String>()
                .map_err(|err| not_a_statement(format!("line `{id}` `reference`: {err}")))?,
        ),
        Ok(None) => None,
        Err(err) => return Err(not_a_statement(format!("line `{id}` `reference`: {err}"))),
    };
    Ok((
        position,
        StatementLine {
            date: parsed(pred::DATE, text(line, pred::DATE)?, |s| s.parse::<Date>())?,
            description: text(line, pred::DESCRIPTION)?,
            amount: Money::new(int(line, pred::AMOUNT)?, currency),
            reference,
            id,
        },
    ))
}
