use rdx_model::{Digest, Match, Receipt, Statement};
use rdx_plan::rdx_host::{Capability, Host};
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::export::{self, Format};
use crate::{Accounts, Booking, Ledger, book};

/// Registers the ledger ops with a host's registry.
pub fn register(registry: &mut Registry) {
    registry.register(Book);
    registry.register(Post);
    registry.register(Read);
    registry.register(Export);
}

const STORES: &[Capability] = &[Capability::BlobStore, Capability::Kv];

fn invalid(op: &str, e: impl std::fmt::Display) -> OpError {
    OpError::new(format!("{op}: invalid arguments: {e}"))
}

/// Where a ledger is: a name (the kv entry `ledger/<name>` holds its current digest) or a digest
/// (one version, read straight from the blob store, as an audit does).
enum Location {
    Named(String),
    Version(Digest),
}

fn location(op: &str, ledger: &str) -> Result<Location, OpError> {
    if ledger.starts_with("sha256:") {
        return Digest::parse(ledger)
            .map(Location::Version)
            .map_err(|e| invalid(op, e));
    }
    let slug = ledger
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && ledger
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if !slug || ledger.len() > 64 {
        return Err(invalid(
            op,
            format!(
                "ledger `{ledger}` is neither a name (lowercase letters, digits, `-`, `_`) nor a \
                 `sha256:` digest"
            ),
        ));
    }
    Ok(Location::Named(ledger.to_string()))
}

fn kv_key(name: &str) -> String {
    format!("ledger/{name}")
}

/// The ledger and the digest of its text (`None`: a name with nothing posted yet).
fn load(op: &str, host: &Host, at: &Location) -> Result<(Ledger, Option<Digest>), OpError> {
    let digest = match at {
        Location::Version(d) => d.clone(),
        Location::Named(name) => {
            let kv = host
                .kv
                .as_ref()
                .ok_or_else(|| OpError::new(format!("{op} ran without the `kv` capability")))?;
            let Some(bytes) = kv.get(&kv_key(name))? else {
                return Ok((Ledger::default(), None));
            };
            let text: String = serde_json::from_slice(&bytes)
                .map_err(|e| OpError::new(format!("{op}: kv `{}`: {e}", kv_key(name))))?;
            Digest::parse(&text)
                .map_err(|e| OpError::new(format!("{op}: kv `{}`: {e}", kv_key(name))))?
        }
    };
    let store = host
        .blob_store
        .as_ref()
        .ok_or_else(|| OpError::new(format!("{op} ran without the `blob-store` capability")))?;
    let bytes = store.get(digest.as_str())?.ok_or_else(|| {
        OpError::new(format!(
            "{op}: ledger version {digest} is not in the blob store"
        ))
    })?;
    let text = String::from_utf8(bytes)
        .map_err(|_| OpError::new(format!("{op}: ledger {digest} is not UTF-8 text")))?;
    let ledger =
        Ledger::parse(&text).map_err(|e| OpError::new(format!("{op}: ledger {digest}: {e}")))?;
    Ok((ledger, Some(digest)))
}

fn put(op: &str, host: &mut Host, bytes: &[u8]) -> Result<String, OpError> {
    let store = host
        .blob_store
        .as_mut()
        .ok_or_else(|| OpError::new(format!("{op} ran without the `blob-store` capability")))?;
    Ok(store.put(bytes)?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BookArgs {
    statement: Statement,
    receipts: Vec<Receipt>,
    matches: Vec<Match>,
    accounts: Accounts,
    #[serde(default)]
    original: Option<Digest>,
}

/// `ledger.book { statement, receipts, matches, accounts, original? }`: the entries one
/// statement's confirmed matches book to, and their Beancount text. No effects, so a dry run
/// shows exactly what `ledger.post` would write.
struct Book;

impl Op for Book {
    fn id(&self) -> &'static str {
        "ledger.book"
    }

    fn run(&self, args: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
        let a = BookArgs::deserialize(args).map_err(|e| invalid(self.id(), e))?;
        let booking = book(
            &a.statement,
            &a.receipts,
            &a.matches,
            &a.accounts,
            a.original.as_ref(),
        )
        .map_err(|e| OpError::new(format!("ledger.book: {e}")))?;
        // The booking alone, rendered: the text it adds to a ledger.
        let mut preview = Ledger::default();
        preview
            .post(&booking)
            .map_err(|e| OpError::new(format!("ledger.book: {e}")))?;
        let text = preview
            .render()
            .map_err(|e| OpError::new(format!("ledger.book: {e}")))?;
        let mut out = serde_json::to_value(&booking).expect("bookings serialize");
        out["text"] = Value::String(text);
        Ok(out)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PostArgs {
    booking: Value,
    ledger: String,
}

/// `ledger.post { booking, ledger }`: adds a `ledger.book` output to the named ledger, stores the
/// new version in the blob store and points `ledger/<name>` at it. Posting the same booking again
/// changes nothing. Has effects.
struct Post;

impl Op for Post {
    fn id(&self) -> &'static str {
        "ledger.post"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        STORES
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let a = PostArgs::deserialize(args).map_err(|e| invalid(op, e))?;
        // `booking` is usually `rdx.ref` to a ledger.book step, whose output also has `text`.
        let mut booking = a.booking;
        if let Some(o) = booking.as_object_mut() {
            o.remove("text");
        }
        let booking = Booking::deserialize(&booking).map_err(|e| invalid(op, e))?;
        let Location::Named(name) = location(op, &a.ledger)? else {
            return Err(invalid(
                op,
                "post to a ledger name; a digest names a past version, which never changes",
            ));
        };
        let (mut ledger, previous) = load(op, cx.host, &Location::Named(name.clone()))?;
        let report = ledger
            .post(&booking)
            .map_err(|e| OpError::new(format!("{op}: {e}")))?;
        let text = ledger
            .render()
            .map_err(|e| OpError::new(format!("{op}: {e}")))?;
        let digest = put(op, cx.host, text.as_bytes())?;
        let kv = cx
            .host
            .kv
            .as_mut()
            .ok_or_else(|| OpError::new(format!("{op} ran without the `kv` capability")))?;
        kv.put(
            &kv_key(&name),
            serde_json::to_string(&digest).unwrap().as_bytes(),
        )?;
        Ok(json!({
            "ledger": name,
            "previous": previous,
            "digest": digest,
            "statement": report.statement,
            "new_statement": report.new_statement,
            "added": report.added,
            "already_booked": report.already_booked,
            "entries": ledger.entries.len(),
            "statements": ledger.statements.len(),
        }))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    ledger: String,
}

/// `ledger.read { ledger }`: a ledger's entries, balance assertions and text, by name (current
/// version) or digest (any version). No effects.
struct Read;

impl Op for Read {
    fn id(&self) -> &'static str {
        "ledger.read"
    }

    fn needs(&self) -> &'static [Capability] {
        STORES
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let a = ReadArgs::deserialize(args).map_err(|e| invalid(op, e))?;
        let (ledger, digest) = load(op, cx.host, &location(op, &a.ledger)?)?;
        if digest.is_none() {
            cx.notes
                .push(format!("ledger `{}` has nothing posted yet", a.ledger));
        }
        let balances = ledger
            .balances()
            .map_err(|e| OpError::new(format!("{op}: {e}")))?;
        let text = ledger
            .render()
            .map_err(|e| OpError::new(format!("{op}: {e}")))?;
        Ok(json!({
            "ledger": a.ledger,
            "digest": digest,
            "entries": ledger.entries,
            "balances": balances,
            "text": text,
        }))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportArgs {
    ledger: String,
    format: Format,
}

/// `ledger.export { ledger, format }`: writes the ledger as a CSV or XLSX spreadsheet to the blob
/// store and returns its digest. Has effects (it stores a file).
struct Export;

impl Op for Export {
    fn id(&self) -> &'static str {
        "ledger.export"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        STORES
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let a = ExportArgs::deserialize(args).map_err(|e| invalid(op, e))?;
        let (ledger, digest) = load(op, cx.host, &location(op, &a.ledger)?)?;
        let bytes = match a.format {
            Format::Csv => export::csv(&ledger).into_bytes(),
            Format::Xlsx => export::xlsx(&ledger),
        };
        let document = put(op, cx.host, &bytes)?;
        let base = match &digest {
            Some(d) => format!("ledger-{}", &d.as_str()["sha256:".len()..][..12]),
            None => "ledger-empty".into(),
        };
        Ok(json!({
            "ledger": a.ledger,
            "ledger_digest": digest,
            "format": a.format,
            "document": document,
            "media_type": a.format.media_type(),
            "file_name": format!("{base}.{}", a.format.extension()),
            "rows": export::rows(&ledger).len(),
        }))
    }
}
