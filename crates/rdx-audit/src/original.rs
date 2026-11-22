//! An original document (the bank's file) as an envelope, for encryption:
//!
//! ```text
//! Bytes(…) [ 'isA': "rdx.original.v1"  "digest": "sha256:…"  "statement": "st-…" ]
//! ```
//!
//! `digest` is the SHA-256 of the bytes, the value the signed statement and the ledger name, so
//! whoever opens the envelope can check the document against what the supporter signed.

use bc_envelope::prelude::*;
use sha2::{Digest as _, Sha256};

/// The `'isA'` of an original document envelope.
pub const ORIGINAL_TYPE: &str = "rdx.original.v1";

const DIGEST: &str = "digest";
const STATEMENT: &str = "statement";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("not an original document envelope: {0}")]
    NotAnOriginal(String),
    #[error("the document's bytes hash to `{actual}`, not the `{recorded}` it was kept as")]
    Mismatch { recorded: String, actual: String },
}

/// `sha256:<hex>` of raw bytes, as the blob store addresses them.
pub fn digest_bytes(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// The envelope to encrypt.
pub fn wrap(bytes: &[u8], statement: &str) -> Envelope {
    Envelope::new(ByteString::from(bytes))
        .add_assertion(known_values::IS_A, ORIGINAL_TYPE)
        .add_assertion(DIGEST, digest_bytes(bytes))
        .add_assertion(STATEMENT, statement)
}

/// An opened original: its bytes, checked against the digest it was kept under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Original {
    pub bytes: Vec<u8>,
    pub digest: String,
    pub statement: String,
}

pub fn unwrap(envelope: &Envelope) -> Result<Original, Error> {
    let bad = |e: bc_envelope::Error| Error::NotAnOriginal(e.to_string());
    let kind: String = envelope
        .extract_object_for_predicate(known_values::IS_A)
        .map_err(bad)?;
    if kind != ORIGINAL_TYPE {
        return Err(Error::NotAnOriginal(format!("it is a `{kind}`")));
    }
    let bytes: ByteString = envelope.extract_subject().map_err(bad)?;
    let recorded: String = envelope.extract_object_for_predicate(DIGEST).map_err(bad)?;
    let actual = digest_bytes(bytes.data());
    if actual != recorded {
        return Err(Error::Mismatch { recorded, actual });
    }
    Ok(Original {
        bytes: bytes.data().to_vec(),
        digest: recorded,
        statement: envelope
            .extract_object_for_predicate(STATEMENT)
            .map_err(bad)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_checks_the_digest() {
        let e = wrap(b"%PDF-1.7 statement", "st-1");
        let o = unwrap(&e).unwrap();
        assert_eq!(o.bytes, b"%PDF-1.7 statement");
        assert_eq!(o.digest, digest_bytes(b"%PDF-1.7 statement"));
        assert_eq!(o.statement, "st-1");

        let forged = Envelope::new(ByteString::from(b"other".as_slice()))
            .add_assertion(known_values::IS_A, ORIGINAL_TYPE)
            .add_assertion(DIGEST, digest_bytes(b"%PDF-1.7 statement"))
            .add_assertion(STATEMENT, "st-1");
        assert!(matches!(unwrap(&forged), Err(Error::Mismatch { .. })));
    }
}
