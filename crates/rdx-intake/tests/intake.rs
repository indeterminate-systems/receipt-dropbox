//! Email intake end to end on the in-memory host: who is accepted, what is stored, what is sent
//! back, and what happens when a quarantine is claimed or expires. Every message is synthetic.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rdx_host::memory::{FixedClock, MemorySecrets, Outbox, SeededRandom};
use rdx_host::{BlobStore, Host, Kv};
use rdx_intake::{
    Allow, Ask, Outcome, Policy, ReplyKind, parse_claim_link, receive, release, status, sweep,
};
use rdx_keys::{Keys, envelope_from_ur};

/// Blob store and kv that tests can list, to check what was kept.
#[derive(Clone, Default)]
struct Store(Rc<RefCell<BTreeMap<String, Vec<u8>>>>);

impl Store {
    fn ids(&self) -> Vec<String> {
        self.0.borrow().keys().cloned().collect()
    }
    fn read(&self, id: &str) -> Option<Vec<u8>> {
        self.0.borrow().get(id).cloned()
    }
}

impl BlobStore for Store {
    fn put(&mut self, bytes: &[u8]) -> rdx_host::Result<String> {
        let id = rdx_host::sha256_digest(bytes);
        self.0.borrow_mut().insert(id.clone(), bytes.to_vec());
        Ok(id)
    }
    fn get(&self, id: &str) -> rdx_host::Result<Option<Vec<u8>>> {
        Ok(self.0.borrow().get(id).cloned())
    }
    fn delete(&mut self, id: &str) -> rdx_host::Result<()> {
        self.0.borrow_mut().remove(id);
        Ok(())
    }
}

impl Kv for Store {
    fn get(&self, key: &str) -> rdx_host::Result<Option<Vec<u8>>> {
        Ok(self.0.borrow().get(key).cloned())
    }
    fn put(&mut self, key: &str, value: &[u8]) -> rdx_host::Result<()> {
        self.0.borrow_mut().insert(key.to_owned(), value.to_vec());
        Ok(())
    }
    fn delete(&mut self, key: &str) -> rdx_host::Result<()> {
        self.0.borrow_mut().remove(key);
        Ok(())
    }
}

const DAY: u64 = 24 * 60 * 60 * 1000;

fn policy() -> Policy {
    Policy {
        intake_address: "receipts@intake.example.org".into(),
        claim_url: "https://app.example.org/claim".into(),
        allow: Allow {
            addresses: vec!["volunteer@mail.example.net".into()],
            domains: vec!["example.org".into()],
        },
        authserv_id: "mx.cloudflare.net".into(),
        quarantine_days: 7,
        reply_interval_days: 7,
        max_bytes: 1024 * 1024,
    }
}

struct Fixture {
    host: Host,
    kv: Store,
    blobs: Store,
    outbox: Outbox,
    secrets: MemorySecrets,
}

fn fixture(now: u64) -> Fixture {
    let kv = Store::default();
    let blobs = Store::default();
    let outbox = Outbox::default();
    let secrets = MemorySecrets::default();
    let host = Host::new()
        .with_kv(kv.clone())
        .with_blob_store(blobs.clone())
        .with_mail_out(outbox.clone())
        .with_clock(FixedClock(now))
        .with_random(SeededRandom::new(7))
        .with_secrets(secrets.clone());
    Fixture {
        host,
        kv,
        blobs,
        outbox,
        secrets,
    }
}

impl Fixture {
    fn at(&mut self, now: u64) {
        self.host.clock = Some(Box::new(FixedClock(now)));
    }
}

const T0: u64 = FixedClock::GOLDEN;

fn email(from: &str, auth: &str) -> Vec<u8> {
    format!(
        "Authentication-Results: {auth}\r\n\
         Received: from mail.example.net by mx.cloudflare.net\r\n\
         Authentication-Results: mx.cloudflare.net; dmarc=pass\r\n\
         From: \"Ana Ruiz\" <{from}>\r\n\
         To: receipts@intake.example.org\r\n\
         Subject: Fwd: Your receipt from Lantern Catering\r\n\
         Message-ID: <synthetic-1@mail.example.net>\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         ---------- Forwarded message ---------\r\n\
         From: Lantern Catering <billing@lantern.example>\r\n\
         Total: MXN 1,250.00\r\n"
    )
    .into_bytes()
}

fn pass(from_domain: &str) -> String {
    format!(
        "mx.cloudflare.net; spf=pass smtp.mailfrom={from_domain}; dkim=pass header.d={from_domain}; dmarc=pass header.from={from_domain}"
    )
}

/// Onboards `address` with one device and returns that device's keys, as the PWA would.
fn onboard(host: &mut Host, address: &str) -> Keys {
    let report = run(
        host,
        "keys.onboard",
        serde_json::json!({
            "address": address, "supporter": "s-0001", "device": "laptop",
            "threshold": 2, "holders": ["a", "b", "c"]
        }),
    );
    assert!(report.ok, "onboarding failed: {report:?}");
    let (_, keys) = rdx_keys::ops::this_device("test", host, address).unwrap();
    keys
}

fn run(host: &mut Host, op: &str, args: serde_json::Value) -> rdx_plan::RunReport {
    let mut ops = rdx_plan::Registry::with_core();
    rdx_keys::register(&mut ops);
    rdx_intake::register(&mut ops);
    let plan: rdx_plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": false,
        "steps": [{ "id": "s", "op": op, "args": args }]
    }))
    .unwrap();
    rdx_plan::execute(&plan, &ops, host).unwrap()
}

fn stored(blobs: &Store, id: &str) -> bc_envelope::Envelope {
    let bytes = blobs.read(id).expect("the document is in the blob store");
    envelope_from_ur(std::str::from_utf8(&bytes).unwrap()).unwrap()
}

#[test]
fn a_known_sender_s_mail_is_encrypted_to_their_keys_and_nothing_else_is_kept() {
    let mut f = fixture(T0);
    let keys = onboard(&mut f.host, "ana@example.org");
    let raw = email("ana@example.org", &pass("example.org"));

    let got = receive(&mut f.host, &policy(), &raw).unwrap();

    let Outcome::Delivered {
        address, document, ..
    } = &got.outcome
    else {
        panic!("expected delivery, got {:?}", got.outcome)
    };
    assert_eq!(address, "ana@example.org");
    assert!(got.reply.is_none());
    // The stored document opens with the supporter's key and holds the message as sent.
    let opened = rdx_keys::decrypt(&stored(&f.blobs, document), &keys).unwrap();
    assert_eq!(rdx_intake::message_bytes(&opened).unwrap(), raw);
    // The plaintext is nowhere in storage.
    let needle = b"Lantern Catering";
    for id in f.blobs.ids() {
        let b = f.blobs.read(&id).unwrap();
        assert!(
            !b.windows(needle.len()).any(|w| w == needle),
            "plaintext in blob {id}"
        );
    }
    // The inbox lists it for the supporter's devices.
    let inbox = rdx_intake::inbox(&f.host, "ana@example.org").unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(&inbox[0].id, document);
}

#[test]
fn senders_outside_the_allowlist_are_rejected() {
    let mut f = fixture(T0);
    let raw = email("stranger@elsewhere.example", &pass("elsewhere.example"));
    let got = receive(&mut f.host, &policy(), &raw).unwrap();
    let Outcome::Rejected { reason, .. } = &got.outcome else {
        panic!("expected a rejection, got {:?}", got.outcome)
    };
    assert!(
        reason.contains("not on this organization's list"),
        "{reason}"
    );
    assert!(f.blobs.ids().is_empty());
}

#[test]
fn single_addresses_can_be_allowlisted_outside_the_org_domain() {
    let mut f = fixture(T0);
    let raw = email("Volunteer@Mail.Example.NET", &pass("mail.example.net"));
    let got = receive(&mut f.host, &policy(), &raw).unwrap();
    assert!(
        matches!(&got.outcome, Outcome::Quarantined { address, .. } if address == "volunteer@mail.example.net"),
        "{:?}",
        got.outcome
    );
}

#[test]
fn mail_that_fails_dmarc_or_carries_only_sender_supplied_results_is_rejected() {
    let p = policy();
    for (auth, why) in [
        (
            "mx.cloudflare.net; spf=pass; dkim=fail; dmarc=fail header.from=example.org",
            "dmarc=fail",
        ),
        ("mx.cloudflare.net; spf=pass; dkim=pass", "no dmarc result"),
        (
            "mx.cloudflare.net; dmarc=pass header.from=other.example",
            "dmarc passed for another domain",
        ),
    ] {
        let mut f = fixture(T0);
        onboard(&mut f.host, "ana@example.org");
        let got = receive(&mut f.host, &p, &email("ana@example.org", auth)).unwrap();
        assert!(
            matches!(&got.outcome, Outcome::Rejected { reason, .. } if reason.contains("DMARC")),
            "{why}: {:?}",
            got.outcome
        );
    }

    // A sender can write its own Authentication-Results lower down; only the first one, which the
    // receiving server prepends, counts.
    let mut f = fixture(T0);
    let forged = format!(
        "Authentication-Results: other.example; dmarc=pass\r\n{}",
        String::from_utf8(email("ana@example.org", &pass("example.org"))).unwrap()
    );
    let got = receive(&mut f.host, &p, forged.as_bytes()).unwrap();
    assert!(
        matches!(&got.outcome, Outcome::Rejected { reason, .. } if reason.contains("mx.cloudflare.net")),
        "{:?}",
        got.outcome
    );
}

#[test]
fn oversized_and_malformed_mail_is_rejected() {
    let mut p = policy();
    p.max_bytes = 100;
    let mut f = fixture(T0);
    let got = receive(
        &mut f.host,
        &p,
        &email("ana@example.org", &pass("example.org")),
    )
    .unwrap();
    assert!(
        matches!(&got.outcome, Outcome::Rejected { reason, .. } if reason.contains("larger than")),
        "{:?}",
        got.outcome
    );

    let two = "Authentication-Results: mx.cloudflare.net; dmarc=pass header.from=example.org\r\n\
               From: ana@example.org, ben@example.org\r\n\r\nhi\r\n";
    let got = receive(&mut f.host, &policy(), two.as_bytes()).unwrap();
    assert!(
        matches!(&got.outcome, Outcome::Rejected { reason, .. } if reason.contains("one sender")),
        "{:?}",
        got.outcome
    );
}

#[test]
fn an_unverified_sender_gets_one_link_and_their_mail_waits_encrypted_to_it() {
    let mut f = fixture(T0);
    let p = policy();
    let raw = email("ben@example.org", &pass("example.org"));

    let first = receive(&mut f.host, &p, &raw).unwrap();
    let Outcome::Quarantined {
        document,
        expires_at,
        ..
    } = &first.outcome
    else {
        panic!("expected quarantine, got {:?}", first.outcome)
    };
    assert_eq!(*expires_at, T0 + 7 * DAY);
    let reply = first.reply.as_ref().expect("a verification reply");
    assert_eq!(reply.message.to, "ben@example.org");
    assert!(
        reply
            .message
            .text
            .contains("https://app.example.org/claim#")
    );
    assert_eq!(
        reply.in_reply_to.as_deref(),
        Some("<synthetic-1@mail.example.net>")
    );
    // The reply's link holds the only copy of the quarantine key.
    let (address, secret) = parse_claim_link(&reply.message.text).unwrap();
    assert_eq!(address, "ben@example.org");
    let key = Keys::from_secret(&secret);
    let opened = rdx_keys::decrypt(&stored(&f.blobs, document), &key).unwrap();
    assert_eq!(rdx_intake::message_bytes(&opened).unwrap(), raw);
    let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
    for k in f.kv.ids() {
        let v = String::from_utf8_lossy(&f.kv.read(&k).unwrap()).into_owned();
        assert!(!v.contains(&hex), "the quarantine key is stored at {k}");
    }

    // More mail from them joins the same quarantine, with no second reply.
    f.at(T0 + DAY);
    let second = receive(&mut f.host, &p, &raw).unwrap();
    assert!(matches!(second.outcome, Outcome::Quarantined { .. }));
    assert!(second.reply.is_none());
    let s = status(&f.host).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].documents, 2);
    assert!(!s[0].expired);
}

#[test]
fn automatic_mail_from_an_unverified_sender_gets_no_reply() {
    let mut f = fixture(T0);
    let raw = String::from_utf8(email("ben@example.org", &pass("example.org")))
        .unwrap()
        .replacen("Subject:", "Auto-Submitted: auto-replied\r\nSubject:", 1);
    let got = receive(&mut f.host, &policy(), raw.as_bytes()).unwrap();
    assert!(
        matches!(&got.outcome, Outcome::Rejected { reason, .. } if reason.contains("automatic")),
        "{:?}",
        got.outcome
    );
    assert!(got.reply.is_none());
}

#[test]
fn claiming_a_quarantine_moves_its_mail_to_the_new_keys_and_forgets_the_link() {
    let mut f = fixture(T0);
    let p = policy();
    let raw = email("ben@example.org", &pass("example.org"));
    let first = receive(&mut f.host, &p, &raw).unwrap();
    let old = match &first.outcome {
        Outcome::Quarantined { document, .. } => document.clone(),
        o => panic!("{o:?}"),
    };
    let link = first.reply.unwrap().message.text;
    let (address, secret) = parse_claim_link(&link).unwrap();

    // Ben opens the link: the PWA onboards the address, then claims what waited for it.
    f.at(T0 + 2 * DAY);
    let keys = onboard(&mut f.host, &address);
    let wrong = [9u8; 32];
    assert!(release(&mut f.host, &address, &wrong).is_err());
    let moved = release(&mut f.host, &address, &secret).unwrap();

    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].from, old);
    assert!(f.blobs.read(&old).is_none(), "quarantined copy deleted");
    let opened = rdx_keys::decrypt(&stored(&f.blobs, &moved[0].to), &keys).unwrap();
    assert_eq!(rdx_intake::message_bytes(&opened).unwrap(), raw);
    // The old key no longer opens anything.
    assert!(
        rdx_keys::decrypt(&stored(&f.blobs, &moved[0].to), &Keys::from_secret(&secret)).is_err()
    );
    assert!(status(&f.host).unwrap().is_empty());
    let inbox = rdx_intake::inbox(&f.host, &address).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].id, moved[0].to);

    // From now on Ben's mail is delivered straight to his keys.
    let next = receive(&mut f.host, &p, &raw).unwrap();
    assert!(matches!(next.outcome, Outcome::Delivered { .. }));
}

#[test]
fn an_expired_quarantine_is_deleted_and_the_sender_told_once() {
    let mut f = fixture(T0);
    let p = policy();
    let raw = email("ben@example.org", &pass("example.org"));
    let first = receive(&mut f.host, &p, &raw).unwrap();
    let doc = match &first.outcome {
        Outcome::Quarantined { document, .. } => document.clone(),
        o => panic!("{o:?}"),
    };
    let sent_before = f.outbox.sent().len();

    // Not yet expired: nothing happens.
    f.at(T0 + 7 * DAY - 1);
    let early = sweep(&mut f.host, &p).unwrap();
    assert!(early.expired.is_empty());
    assert!(early.notices.is_empty());

    f.at(T0 + 7 * DAY);
    let s = status(&f.host).unwrap();
    assert!(s[0].expired);
    let swept = sweep(&mut f.host, &p).unwrap();
    assert_eq!(swept.expired.len(), 1);
    assert_eq!(swept.expired[0].address, "ben@example.org");
    assert_eq!(swept.expired[0].deleted, vec![doc.clone()]);
    assert!(f.blobs.read(&doc).is_none());
    assert!(status(&f.host).unwrap().is_empty());
    assert_eq!(swept.notices.len(), 1);
    assert_eq!(swept.notices[0].to, "ben@example.org");
    assert!(swept.notices[0].text.contains("deleted"));
    // `sweep` hands notices back; it doesn't send them itself.
    assert_eq!(f.outbox.sent().len(), sent_before);

    // A second sweep finds nothing.
    assert!(sweep(&mut f.host, &p).unwrap().expired.is_empty());
}

#[test]
fn a_new_link_waits_for_the_reply_interval() {
    let mut f = fixture(T0);
    let mut p = policy();
    p.quarantine_days = 2;
    p.reply_interval_days = 5;
    let raw = email("ben@example.org", &pass("example.org"));
    assert!(receive(&mut f.host, &p, &raw).unwrap().reply.is_some());
    f.at(T0 + 3 * DAY);
    sweep(&mut f.host, &p).unwrap();

    // Within the interval: refused, and told when to try again.
    let got = receive(&mut f.host, &p, &raw).unwrap();
    assert!(
        matches!(&got.outcome, Outcome::Rejected { reason, .. } if reason.contains("already sent")),
        "{:?}",
        got.outcome
    );
    assert!(got.reply.is_none());

    // After it: a fresh quarantine and link.
    f.at(T0 + 5 * DAY);
    let got = receive(&mut f.host, &p, &raw).unwrap();
    assert!(matches!(got.outcome, Outcome::Quarantined { .. }));
    assert!(got.reply.is_some());
}

#[test]
fn the_reply_is_a_mime_message_cloudflare_can_send() {
    let mut f = fixture(T0);
    let got = receive(
        &mut f.host,
        &policy(),
        &email("ben@example.org", &pass("example.org")),
    )
    .unwrap();
    let mime = got
        .reply
        .unwrap()
        .to_mime("receipts@intake.example.org", T0);
    for header in [
        "From: receipts@intake.example.org\r\n",
        "To: ben@example.org\r\n",
        "In-Reply-To: <synthetic-1@mail.example.net>\r\n",
        "References: <synthetic-1@mail.example.net>\r\n",
        "Auto-Submitted: auto-replied\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "Date: Thu, 1 Jan 2026 00:00:00 +0000\r\n",
    ] {
        assert!(mime.contains(header), "missing {header:?} in\n{mime}");
    }
    assert!(mime.contains("Message-ID: <"));
}

#[test]
fn the_secrets_capability_carries_the_claim_link_for_the_release_op() {
    // The op reads the link from `secrets`, so it never appears in plan arguments or reports.
    let mut f = fixture(T0);
    let p = policy();
    let first = receive(
        &mut f.host,
        &p,
        &email("ben@example.org", &pass("example.org")),
    )
    .unwrap();
    let link = first.reply.unwrap().message.text;
    onboard(&mut f.host, "ben@example.org");
    f.secrets.answer("claim-link", &link);

    let report = run(
        &mut f.host,
        "intake.release",
        serde_json::json!({ "address": "ben@example.org" }),
    );
    assert!(report.ok, "{report:?}");
    let out = serde_json::to_string(&report).unwrap();
    assert!(!out.contains("claim#"), "the link leaked into the report");
    assert!(status(&f.host).unwrap().is_empty());
}

/// What a supporter sends when they share a Wise transfer from the app: a tracking link and no
/// receipt. Quoted-printable, as mail apps send it, so the link is split by a soft line break.
fn wise_link_email(from: &str, attachment: Option<(&str, &str)>) -> Vec<u8> {
    let attachment = attachment.map_or(String::new(), |(media_type, name)| {
        format!(
            "--outer\r\n\
             Content-Type: {media_type}; name=\"{name}\"\r\n\
             Content-Disposition: attachment; filename=\"{name}\"\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             JVBERi0xLjQKJSBzeW50aGV0aWMK\r\n"
        )
    });
    format!(
        "Authentication-Results: {}\r\n\
         From: Ana Ruiz <{from}>\r\n\
         To: receipts@intake.example.org\r\n\
         Subject: Rent for the studio\r\n\
         Message-ID: <synthetic-wise@mail.example.net>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"outer\"\r\n\
         \r\n\
         --outer\r\n\
         Content-Type: multipart/alternative; boundary=\"alt\"\r\n\
         \r\n\
         --alt\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\
         \r\n\
         Here's the transfer for November:\r\n\
         https://wise.com/gb/track/synthetic0a1b2c3d?utm_source=3Dshare&utm_medium=\r\n\
         =3Dapp\r\n\
         --alt\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         \r\n\
         <p>Here's the transfer for November: <a href=\"https://wise.com/gb/track/synthetic0a1b2c3d\">track it</a></p>\r\n\
         --alt--\r\n\
         {attachment}\
         --outer--\r\n",
        pass(from.rsplit_once('@').unwrap().1),
    )
    .into_bytes()
}

#[test]
fn a_wise_link_without_a_receipt_is_kept_and_answered_with_how_to_send_the_pdf() {
    let mut f = fixture(T0);
    let keys = onboard(&mut f.host, "ana@example.org");
    let p = policy();
    let raw = wise_link_email("ana@example.org", None);

    let got = receive(&mut f.host, &p, &raw).unwrap();

    // Kept like any other email: encrypted to Ana's keys, the link never fetched or stored plain.
    let Outcome::Delivered { document, .. } = &got.outcome else {
        panic!("expected delivery, got {:?}", got.outcome)
    };
    let opened = rdx_keys::decrypt(&stored(&f.blobs, document), &keys).unwrap();
    assert_eq!(rdx_intake::message_bytes(&opened).unwrap(), raw);
    for id in f.blobs.ids().into_iter().chain(f.kv.ids()) {
        let b = f.blobs.read(&id).or_else(|| f.kv.read(&id)).unwrap();
        assert!(
            !b.windows(9).any(|w| w == b"wise.com/"),
            "the link is stored in plaintext at {id}"
        );
    }
    // One reply, asking for the PDF; it doesn't repeat the link.
    assert_eq!(got.asked, Some(Ask::WiseReceipt));
    let reply = got.reply.as_ref().expect("a reply asking for the receipt");
    assert_eq!(reply.kind, ReplyKind::Ask);
    assert_eq!(reply.message.to, "ana@example.org");
    assert!(reply.message.text.contains("PDF"), "{}", reply.message.text);
    assert!(reply.message.text.contains("receipts@intake.example.org"));
    assert!(!reply.message.text.contains("synthetic0a1b2c3d"));
    assert!(!reply.message.text.contains("claim#"));

    // Another link the next day: kept, but no second reply until the interval has passed.
    f.at(T0 + DAY);
    let again = receive(&mut f.host, &p, &raw).unwrap();
    assert!(matches!(again.outcome, Outcome::Delivered { .. }));
    assert!(again.reply.is_none());
    assert_eq!(again.asked, None);
    f.at(T0 + 7 * DAY);
    let later = receive(&mut f.host, &p, &raw).unwrap();
    assert_eq!(later.asked, Some(Ask::WiseReceipt));
}

#[test]
fn a_wise_link_with_its_receipt_attached_needs_no_reply() {
    let mut f = fixture(T0);
    onboard(&mut f.host, "ana@example.org");
    let raw = wise_link_email("ana@example.org", Some(("application/pdf", "receipt.pdf")));
    let got = receive(&mut f.host, &policy(), &raw).unwrap();
    assert!(matches!(got.outcome, Outcome::Delivered { .. }));
    assert!(got.reply.is_none());
    assert_eq!(got.asked, None);
}

#[test]
fn only_wise_tracking_links_count() {
    let mut f = fixture(T0);
    onboard(&mut f.host, "ana@example.org");
    let base = String::from_utf8(wise_link_email("ana@example.org", None)).unwrap();
    for other in [
        "https://wise.com/invite/u/ana",
        "https://wise.com.track.example/gb/track/x",
        "https://notwise.com/gb/track/x",
    ] {
        let raw = base
            .replace(
                "https://wise.com/gb/track/synthetic0a1b2c3d?utm_source=3Dshare&utm_medium=\r\n=3Dapp",
                other,
            )
            .replace("https://wise.com/gb/track/synthetic0a1b2c3d", other);
        let got = receive(&mut f.host, &policy(), raw.as_bytes()).unwrap();
        assert!(got.reply.is_none(), "{other} got a reply");
    }
}

#[test]
fn automatic_mail_with_a_wise_link_gets_no_reply() {
    let mut f = fixture(T0);
    onboard(&mut f.host, "ana@example.org");
    let raw = String::from_utf8(wise_link_email("ana@example.org", None))
        .unwrap()
        .replacen("Subject:", "Auto-Submitted: auto-generated\r\nSubject:", 1);
    let got = receive(&mut f.host, &policy(), raw.as_bytes()).unwrap();
    assert!(matches!(got.outcome, Outcome::Delivered { .. }));
    assert!(got.reply.is_none());
}

#[test]
fn an_unverified_sender_s_wise_link_is_asked_about_in_the_setup_reply() {
    let mut f = fixture(T0);
    let p = policy();
    let raw = wise_link_email("ben@example.org", None);

    // Email Routing allows one reply per email, so the setup reply carries the request too.
    let first = receive(&mut f.host, &p, &raw).unwrap();
    assert!(matches!(first.outcome, Outcome::Quarantined { .. }));
    assert_eq!(first.asked, Some(Ask::WiseReceipt));
    let reply = first.reply.as_ref().unwrap();
    assert_eq!(reply.kind, ReplyKind::Verification);
    assert!(reply.message.text.contains("claim#"));
    assert!(reply.message.text.contains("PDF"));

    // A link while the quarantine waits: no second setup link, and the PDF was asked for already.
    f.at(T0 + DAY);
    let second = receive(&mut f.host, &p, &raw).unwrap();
    assert!(matches!(second.outcome, Outcome::Quarantined { .. }));
    assert!(second.reply.is_none());

    // A different sender already in quarantine from an ordinary email gets the request alone.
    let carl = email("carl@example.org", &pass("example.org"));
    receive(&mut f.host, &p, &carl).unwrap();
    let link = receive(&mut f.host, &p, &wise_link_email("carl@example.org", None)).unwrap();
    assert!(matches!(link.outcome, Outcome::Quarantined { .. }));
    assert_eq!(link.reply.as_ref().unwrap().kind, ReplyKind::Ask);
}
