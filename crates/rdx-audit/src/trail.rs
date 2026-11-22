//! `audit.trail`: from each ledger entry to its receipt, its encrypted original and the viewable
//! row that shows it, with who was granted the original.

use std::collections::{BTreeMap, BTreeSet};

use rdx_envelope::StatementView;
use rdx_host::{Capability, Host};
use rdx_keys::ops::load_registry;
use rdx_keys::registry::DeviceStatus;
use rdx_keys::{envelope_from_ur, public_keys_from_ur};
use rdx_model::LedgerEntry;
use rdx_plan::{Op, OpError, RunContext};
use serde::Serialize;
use serde_json::{Value, json};

use crate::custody::{GrantState, State};
use crate::ops::load_custody;

pub(crate) struct Trail;

/// A ledger entry and where its links lead.
#[derive(Serialize)]
struct EntryTrail<'a> {
    line: String,
    date: String,
    payee: &'a str,
    postings: &'a [rdx_model::Posting],
    receipt: &'a str,
    receipt_source: &'a str,
    /// Whether the receipt's document is in the blob store.
    receipt_stored: bool,
    /// The original's digest, as the entry names it.
    statement: &'a str,
    /// From the custody record; `None` when the statement has none.
    root: Option<String>,
    source: Option<String>,
    /// `"shown"` or `"redacted"` in the viewable statement given for this statement; `None`
    /// when none was given.
    viewable: Option<&'static str>,
    /// Who may open the original now.
    granted_to: Vec<String>,
}

#[derive(Serialize)]
struct StatementTrail<'a> {
    statement: &'a str,
    address: &'a str,
    root: &'a str,
    original: &'a str,
    sealed: &'a str,
    source: &'a str,
    signer: &'a str,
    kept_at: u64,
    kept_by: &'a str,
    entries: usize,
    grants: &'a [GrantState],
}

/// Every signing key an address's registry has ever listed, with the device and when it was
/// revoked. Old keys count: a custody event signed before a rotation still stands.
fn known_signers(
    host: &Host,
    address: &str,
) -> Result<BTreeMap<String, (String, Option<u64>)>, String> {
    let registry = load_registry(host, address)
        .map_err(|e| e.message)?
        .ok_or_else(|| format!("`{address}` has no key registry here"))?;
    let state = registry.state().map_err(|e| e.to_string())?;
    let mut known = BTreeMap::new();
    let ur = |k: &bc_components::PublicKeys| rdx_envelope::public_key_to_ur(k.signing_public_key());
    known.insert(ur(&state.recovery), ("recovery".to_owned(), None));
    for event in &registry.events {
        let content = envelope_from_ur(event)
            .ok()
            .and_then(|e| e.try_unwrap().ok())
            .ok_or("the key registry has an unreadable event")?;
        let device = content
            .extract_object_for_predicate::<String>("device")
            .ok();
        let keys = content
            .optional_object_for_predicate("keys")
            .ok()
            .flatten()
            .and_then(|o| o.extract_subject::<String>().ok())
            .and_then(|k| public_keys_from_ur(&k).ok());
        if let (Some(device), Some(keys)) = (device, keys) {
            let revoked = match state.devices.get(&device).map(|d| &d.status) {
                Some(DeviceStatus::Revoked { at, .. }) => Some(*at),
                _ => None,
            };
            known.insert(ur(&keys), (device, revoked));
        }
    }
    Ok(known)
}

fn signer_problems(host: &Host, state: &State) -> Vec<String> {
    let known = match known_signers(host, &state.address) {
        Ok(k) => k,
        Err(e) => {
            return vec![format!(
                "`{}`: custody signatures can't be checked: {e}",
                state.statement
            )];
        }
    };
    let mut problems = Vec::new();
    for e in &state.events {
        match known.get(&e.signer_key) {
            None => problems.push(format!(
                "`{}`: the `{}` event was signed by a key `{}`'s registry never listed",
                state.statement, e.event, state.address
            )),
            Some((device, Some(revoked))) if e.at > *revoked => problems.push(format!(
                "`{}`: the `{}` event was signed by `{device}` after it was revoked",
                state.statement, e.event
            )),
            Some((device, _)) if *device != e.device && device != "recovery" => {
                problems.push(format!(
                    "`{}`: the `{}` event says `{}` signed it, but the key is `{device}`'s",
                    state.statement, e.event, e.device
                ))
            }
            _ => {}
        }
    }
    problems
}

impl Op for Trail {
    fn id(&self) -> &'static str {
        "audit.trail"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv, Capability::BlobStore]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let ledger = args.get("ledger").cloned().unwrap_or(Value::Null);
        let ledger_digest = ledger.get("digest").cloned().unwrap_or(Value::Null);
        let entries_value = ledger.get("entries").cloned().unwrap_or(ledger);
        let entries: Vec<LedgerEntry> = serde_json::from_value(entries_value).map_err(|e| {
            OpError::new(format!(
                "{op}: `ledger` must be a `ledger.read` output or a list of entries: {e}"
            ))
        })?;
        let views_value = args.get("views").cloned().unwrap_or(json!([]));
        let views: Vec<StatementView> = match views_value {
            Value::Null => Vec::new(),
            v => serde_json::from_value(v).map_err(|e| {
                OpError::new(format!(
                    "{op}: `views` must be a list of `envelope.verify` outputs: {e}"
                ))
            })?,
        };
        let host = &*cx.host;
        let blobs = host.blob_store.as_ref().expect("checked by needs()");
        let mut problems: Vec<String> = Vec::new();

        // Every statement the ledger or a view names, with its custody state.
        let ids: BTreeSet<String> = entries
            .iter()
            .map(|e| e.line.statement.as_str().to_owned())
            .chain(views.iter().map(|v| v.statement.as_str().to_owned()))
            .collect();
        let mut custody: BTreeMap<String, State> = BTreeMap::new();
        for id in &ids {
            match load_custody(host, id)? {
                None => problems.push(format!(
                    "`{id}` has no custody record: its original isn't kept encrypted (run \
                     `keep-statement` on the supporter's device)"
                )),
                Some(c) => match c.state() {
                    Err(e) => problems.push(format!("`{id}`: {e}")),
                    Ok(s) => {
                        problems.extend(signer_problems(host, &s));
                        for blob in [&s.kept.sealed, &s.kept.source] {
                            if blobs.get(blob)?.is_none() {
                                problems.push(format!(
                                    "`{id}`: kept blob `{blob}` is missing from the blob store"
                                ));
                            }
                        }
                        custody.insert(id.clone(), s);
                    }
                },
            }
        }

        // Which lines each view shows.
        let mut shown: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for v in &views {
            let id = v.statement.as_str();
            if let Some(s) = custody.get(id)
                && v.root.as_str() != s.kept.root
            {
                problems.push(format!(
                    "the viewable statement `{id}` has root `{}`, not the kept `{}`: it is not \
                     the statement the supporter kept",
                    v.root.as_str(),
                    s.kept.root
                ));
            }
            shown
                .entry(id)
                .or_default()
                .extend(v.lines.iter().map(|l| l.id.as_str()));
        }

        let mut booked: BTreeSet<(String, String)> = BTreeSet::new();
        let mut trail = Vec::with_capacity(entries.len());
        for e in &entries {
            let id = e.line.statement.as_str();
            let line = e.line.line.as_str();
            booked.insert((id.to_owned(), line.to_owned()));
            let s = custody.get(id);
            if let Some(s) = s
                && s.kept.original != e.statement.as_str()
            {
                problems.push(format!(
                    "the entry for `{id}/{line}` names original `{}`, but `{id}` was kept with \
                     `{}`",
                    e.statement.as_str(),
                    s.kept.original
                ));
            }
            let viewable = shown.get(id).map(|lines| {
                if lines.contains(line) {
                    "shown"
                } else {
                    "redacted"
                }
            });
            if viewable == Some("redacted") {
                problems.push(format!(
                    "`{id}/{line}` is booked but the viewable statement redacts it"
                ));
            }
            trail.push(EntryTrail {
                line: format!("{id}/{line}"),
                date: e.date.to_string(),
                payee: &e.payee,
                postings: &e.postings,
                receipt: e.receipt.as_str(),
                receipt_source: e.receipt_source.as_str(),
                receipt_stored: blobs.get(e.receipt_source.as_str())?.is_some(),
                statement: e.statement.as_str(),
                root: s.map(|s| s.kept.root.clone()),
                source: s.map(|s| s.kept.source.clone()),
                viewable,
                granted_to: s
                    .map(|s| {
                        s.grants
                            .iter()
                            .filter(|g| g.is_active())
                            .map(|g| g.grant.grantee.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
        for (id, lines) in &shown {
            for line in lines {
                if !booked.contains(&((*id).to_owned(), (*line).to_owned())) {
                    problems.push(format!(
                        "the viewable statement shows `{id}/{line}`, but no ledger entry books it"
                    ));
                }
            }
        }

        let statements: Vec<StatementTrail> = custody
            .values()
            .map(|s| StatementTrail {
                statement: &s.statement,
                address: &s.address,
                root: &s.kept.root,
                original: &s.kept.original,
                sealed: &s.kept.sealed,
                source: &s.kept.source,
                signer: &s.kept.signer,
                kept_at: s.kept_at,
                kept_by: &s.events[0].device,
                entries: entries
                    .iter()
                    .filter(|e| e.line.statement.as_str() == s.statement)
                    .count(),
                grants: &s.grants,
            })
            .collect();
        if entries.is_empty() {
            cx.notes.push(format!("{op}: the ledger has no entries"));
        }
        Ok(json!({
            "ok": problems.is_empty(),
            "ledger": ledger_digest,
            "entries": trail,
            "statements": statements,
            "problems": problems,
        }))
    }
}
