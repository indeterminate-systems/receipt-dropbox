//! Password-protected statements: `statement.unlock` and `statement.remember-password`.
//!
//! Mexican banks send statements as PDFs that open only with a password the account holder
//! knows, usually their RFC or date of birth. The original stays exactly as the bank sent it;
//! only its text is read, in memory, on the supporter's device. The password comes from one of
//! two places, and never appears in a plan, an output or the audit log:
//!
//! - the person, asked on their device through the `secrets` capability, under the label
//!   `statement-password-<first 12 hex digits of the document digest>`;
//! - a password remembered for the bank account, kept in `kv` under
//!   `statement-passwords/<address>` as `{ "<institution> <mask>": "ur:envelope/…" }`, each
//!   encrypted to the address's keys (every active device and the recovery key), so only the
//!   supporter's own devices can read it, and recovery brings it back with everything else.
//!
//! Servers offer no `secrets`, so neither op can run there.

use std::collections::BTreeMap;

use bc_envelope::prelude::*;
use rdx_keys::ops::{recipients, this_device};
use rdx_keys::{decrypt, encrypt, envelope_from_ur, envelope_to_ur, host_rng};
use rdx_plan::rdx_host::{Capability, Host};
use rdx_plan::{Op, OpError, RunContext};
use serde_json::{Value, json};
use zeroize::Zeroizing;

/// The `'isA'` of a remembered statement password.
pub const PASSWORD_TYPE: &str = "rdx.statement-password.v1";

fn passwords_key(address: &str) -> String {
    format!("statement-passwords/{address}")
}

/// The label the device asks the person under for `digest`'s password. Per document, so an
/// answer given for one statement is never silently reused for another.
pub fn password_label(digest: &str) -> String {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    format!("statement-password-{}", &hex[..hex.len().min(12)])
}

/// The remembered passwords for `address`: account name to encrypted envelope (UR).
fn remembered(host: &Host, address: &str) -> Result<BTreeMap<String, String>, OpError> {
    let kv = host.kv.as_ref().expect("checked by needs()");
    match kv.get(&passwords_key(address))? {
        None => Ok(BTreeMap::new()),
        Some(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            OpError::new(format!(
                "the remembered statement passwords for `{address}` are unreadable: {e}"
            ))
        }),
    }
}

fn open_remembered(ur: &str, keys: &rdx_keys::Keys) -> Option<Zeroizing<String>> {
    let envelope = envelope_from_ur(ur).ok()?;
    let inner = decrypt(&envelope, keys).ok()?;
    let kind: String = inner
        .extract_object_for_predicate(known_values::IS_A)
        .ok()?;
    if kind != PASSWORD_TYPE {
        return None;
    }
    inner.extract_subject::<String>().ok().map(Zeroizing::new)
}

fn str_arg<'a>(op: &str, args: &'a Value, key: &str) -> Result<Option<&'a str>, OpError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(OpError::new(format!("{op}: `{key}` must be a string"))),
    }
}

/// `statement.unlock { document, address?, remember? }` → the text layer of a PDF statement,
/// opened with its password if it has one: `{ document, source, pages, text, warnings,
/// password }`, the same shape as `ocr.read`, for `statement.parse`'s `text`.
///
/// `password` says how it opened: `none` (not protected, or protected only against editing),
/// `remembered` (with `account`, whose remembered password opened it) or `asked`. With
/// `remember = true` and an asked password, `remember` holds that password encrypted to
/// `address`'s keys, for `statement.remember-password` to keep.
pub struct StatementUnlock;

impl Op for StatementUnlock {
    fn id(&self) -> &'static str {
        "statement.unlock"
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::BlobStore,
            Capability::Kv,
            Capability::Secrets,
            Capability::Random,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let (digest, bytes) = rdx_pdf::read_document(id, args, cx)?;
        let address = str_arg(id, args, "address")?;
        let remember = args
            .get("remember")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if remember && address.is_none() {
            return Err(OpError::new(format!(
                "{id}: `remember` needs `address`, whose keys the password is encrypted to"
            )));
        }
        let fail = |e: &dyn std::fmt::Display| OpError::new(format!("{digest}: {e}"));
        if !rdx_pdf::is_pdf(&bytes) {
            return Err(fail(&format!(
                "{id} reads PDFs; this document isn't one, so read it with `ocr.read` instead"
            )));
        }
        let host = &mut *cx.host;

        let output = |text: rdx_pdf::Text, how: &str| -> Result<Value, OpError> {
            if text.is_empty() {
                return Err(fail(
                    &"the PDF opened but has no text layer (a scan); protected scans can't be \
                      read yet, so ask for the bank's downloadable statement",
                ));
            }
            Ok(json!({
                "document": digest,
                "source": "text-layer",
                "pages": text.pages,
                "text": text.to_string(),
                "warnings": [],
                "password": how,
            }))
        };

        if !rdx_pdf::needs_password(&bytes) {
            return output(rdx_pdf::text(&bytes).map_err(|e| fail(&e))?, "none");
        }

        // A password remembered for one of the supporter's accounts.
        if let Some(address) = address {
            let kept = remembered(host, address)?;
            if !kept.is_empty() {
                let (_, keys) = this_device(id, host, address)?;
                for (account, ur) in &kept {
                    let Some(password) = open_remembered(ur, &keys) else {
                        // Encrypted before this device's key existed; asking renews it.
                        continue;
                    };
                    if let Ok(text) = rdx_pdf::text_with_password(&bytes, &password) {
                        let mut out = output(text, "remembered")?;
                        out["account"] = json!(account);
                        return Ok(out);
                    }
                }
            }
        }

        // Ask the person.
        let label = password_label(&digest);
        let answer = host
            .secrets
            .as_mut()
            .expect("checked by needs()")
            .ask(&label)?
            .map(Zeroizing::new);
        let Some(password) = answer else {
            return Err(fail(&format!(
                "the PDF is password-protected and no password was given: ask the supporter \
                 for it on their device (`{label}`; Mexican banks usually use the holder's RFC \
                 or date of birth)"
            )));
        };
        let text = rdx_pdf::text_with_password(&bytes, &password).map_err(|e| match e {
            rdx_pdf::PdfError::WrongPassword | rdx_pdf::PdfError::PasswordRequired => {
                fail(&format!(
                    "the password given for `{label}` doesn't open this PDF; ask the supporter \
                     to check it"
                ))
            }
            other => fail(&other),
        })?;
        let mut out = output(text, "asked")?;
        if let (true, Some(address)) = (remember, address) {
            let to = recipients(id, host, address)?;
            let mut rng = host_rng(host.random.as_deref_mut().expect("checked by needs()"))?;
            let envelope =
                Envelope::new(password.as_str()).add_assertion(known_values::IS_A, PASSWORD_TYPE);
            let sealed = encrypt(&envelope, &to, &mut rng)
                .map_err(|e| OpError::new(format!("{id}: {e}")))?;
            out["remember"] = json!(envelope_to_ur(&sealed));
        }
        Ok(out)
    }
}

/// `statement.remember-password { address, unlock, statement }` → `{ remembered }`: keeps the
/// password a `statement.unlock` step was given (its `remember`, already encrypted to
/// `address`'s keys) for the account `statement` (a `statement.parse` output) belongs to, so
/// that account's next statements open without asking. `remembered` is the account's name, or
/// null when there was nothing new to keep.
pub struct RememberPassword;

impl Op for RememberPassword {
    fn id(&self) -> &'static str {
        "statement.remember-password"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let address = str_arg(id, args, "address")?
            .ok_or_else(|| OpError::new(format!("{id} needs `address`")))?;
        let unlock = args.get("unlock").unwrap_or(&Value::Null);
        let Some(sealed) = unlock.get("remember").and_then(Value::as_str) else {
            return Ok(json!({ "remembered": null }));
        };
        let statement = args.get("statement").unwrap_or(&Value::Null);
        let statement = statement.get("statement").unwrap_or(statement);
        if statement.get("original") != unlock.get("document") {
            return Err(OpError::new(format!(
                "{id}: `unlock` and `statement` are about different documents"
            )));
        }
        let account = statement.get("account");
        let field = |k: &str| account.and_then(|a| a.get(k)).and_then(Value::as_str);
        let (Some(institution), Some(mask)) = (field("institution"), field("mask")) else {
            return Err(OpError::new(format!(
                "{id}: `statement` must be the output of `statement.parse`, with its account"
            )));
        };
        let name = format!("{institution} {mask}");
        let host = &mut *cx.host;
        let mut kept = remembered(host, address)?;
        kept.insert(name.clone(), sealed.to_owned());
        let bytes = serde_json::to_vec(&kept).expect("maps serialize");
        host.kv
            .as_mut()
            .expect("checked by needs()")
            .put(&passwords_key(address), &bytes)?;
        Ok(json!({ "remembered": name }))
    }
}
