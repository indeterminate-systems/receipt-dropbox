//! What intake writes to people: the verification reply with its claim link, the expiry notice,
//! and the plain-text MIME a host hands to its mail service.

use rdx_host::Message;
use serde::Serialize;

use crate::{Error, Policy};

/// The one reply an email can get: the message, and the `Message-ID` it answers, which threads it
/// under what they sent.
///
/// A verification reply's text holds the claim link, the only copy of the quarantine key, so a
/// reply is never part of a report or the audit log.
#[derive(Clone, PartialEq, Eq)]
pub struct Reply {
    pub kind: ReplyKind,
    pub message: Message,
    pub in_reply_to: Option<String>,
}

/// Why intake replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplyKind {
    /// The setup link for a sender without keys (and any [`Ask`] for the same email). If it
    /// can't be sent, the email must not be kept: no one could claim it.
    Verification,
    /// Only an [`Ask`]: the email is kept whether or not the reply goes out.
    Ask,
}

/// Something intake asks the sender to send, because what arrived can't be used as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ask {
    /// A Wise tracking link arrived without the transfer's receipt: send the PDF.
    WiseReceipt,
}

impl Ask {
    fn text(self, intake: &str) -> String {
        match self {
            Ask::WiseReceipt => format!(
                "Your email has a link to a Wise transfer but no receipt. We don't open links: \
                 the page behind one shows payment details we'd have to read on a server, and it \
                 can change or expire. Please send the transfer's receipt instead:\n\
                 \n\
                 1. Open the transfer in the Wise app or on wise.com.\n\
                 2. Choose its receipt (Get receipt, or Download receipt) and save the PDF.\n\
                 3. Email the PDF to {intake} from this address.\n"
            ),
        }
    }
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reply")
            .field("kind", &self.kind)
            .field("to", &self.message.to)
            .field("in_reply_to", &self.in_reply_to)
            .finish_non_exhaustive()
    }
}

impl Reply {
    /// The reply as a MIME message from `from` (the intake address) dated `now_ms`.
    pub fn to_mime(&self, from: &str, now_ms: u64) -> String {
        mime(&self.message, from, now_ms, self.in_reply_to.as_deref())
    }
}

/// `message` as a plain-text MIME message, with `In-Reply-To` and `References` when it answers
/// one. `Auto-Submitted` keeps auto-responders from answering it (RFC 3834).
pub fn mime(message: &Message, from: &str, now_ms: u64, in_reply_to: Option<&str>) -> String {
    let domain = from.rsplit_once('@').map_or("localhost", |(_, d)| d);
    let id =
        rdx_host::sha256_digest(format!("{now_ms}\n{}\n{}", message.to, message.text).as_bytes());
    let id = id.trim_start_matches("sha256:");
    let mut m = String::new();
    let mut h = |name: &str, value: &str| {
        m.push_str(name);
        m.push_str(": ");
        m.push_str(value);
        m.push_str("\r\n");
    };
    h("From", from);
    h("To", &message.to);
    h("Subject", &message.subject);
    h("Date", &rfc5322_date(now_ms));
    h("Message-ID", &format!("<{}@{domain}>", &id[..32]));
    if let Some(r) = in_reply_to {
        h("In-Reply-To", r);
        h("References", r);
    }
    h("Auto-Submitted", "auto-replied");
    h("MIME-Version", "1.0");
    h("Content-Type", "text/plain; charset=utf-8");
    h("Content-Transfer-Encoding", "8bit");
    m.push_str("\r\n");
    for line in message.text.lines() {
        m.push_str(line);
        m.push_str("\r\n");
    }
    m
}

pub(crate) fn verification(
    policy: &Policy,
    address: &str,
    secret: &[u8; 32],
    expires_at: u64,
    in_reply_to: Option<String>,
    ask: Option<Ask>,
) -> Reply {
    let link = claim_link(&policy.claim_url, address, secret);
    let intake = &policy.intake_address;
    let until = date(expires_at);
    let ask = ask.map_or(String::new(), |a| format!("\n{}", a.text(intake)));
    Reply {
        kind: ReplyKind::Verification,
        message: Message {
            to: address.to_owned(),
            subject: "Verify your address to send receipts".into(),
            text: format!(
                "We received your email to {intake}, but {address} isn't set up to send \
                 receipts yet, so we're holding what you sent, encrypted, until {until}.\n\
                 \n\
                 To set up this address, open this link on the phone or computer you'll use:\n\
                 \n\
                 {link}\n\
                 \n\
                 The link is the only key to what you sent from this address, so don't forward \
                 it. If you don't open it by {until}, what you sent is deleted unread and you'll \
                 need to send it again.\n\
                 \n\
                 If you didn't write to {intake}, you can ignore this email.\n\
                 {ask}"
            ),
        },
        in_reply_to,
    }
}

/// A reply that only asks for something, to a sender whose email is kept either way.
pub(crate) fn ask(policy: &Policy, address: &str, ask: Ask, in_reply_to: Option<String>) -> Reply {
    let intake = &policy.intake_address;
    let subject = match ask {
        Ask::WiseReceipt => "Please send the Wise receipt as a PDF",
    };
    Reply {
        kind: ReplyKind::Ask,
        message: Message {
            to: address.to_owned(),
            subject: subject.into(),
            text: format!(
                "We received your email to {intake} and kept it, encrypted.\n\n{}",
                ask.text(intake)
            ),
        },
        in_reply_to,
    }
}

pub(crate) fn expiry_notice(
    policy: &Policy,
    address: &str,
    documents: usize,
    expired_at: u64,
) -> Message {
    let intake = &policy.intake_address;
    let what = if documents == 1 {
        "the email".to_owned()
    } else {
        format!("the {documents} emails")
    };
    Message {
        to: address.to_owned(),
        subject: format!("What you sent to {intake} was deleted"),
        text: format!(
            "{address} wasn't set up by {}, so {what} you sent to {intake} were deleted unread.\n\
             \n\
             To send receipts from this address, send one more email to {intake}: you'll get a \
             new link to set it up. Then send your receipts again.\n",
            date(expired_at)
        ),
    }
}

/// `<claim_url>#address=<address>&key=<hex>`. The key is in the fragment, which browsers never
/// send to a server, so the PWA reads it and the server never sees it again.
pub fn claim_link(claim_url: &str, address: &str, secret: &[u8; 32]) -> String {
    let key: String = secret.iter().map(|b| format!("{b:02x}")).collect();
    format!("{claim_url}#address={}&key={key}", encode(address))
}

/// The address and quarantine key in a claim link, found anywhere in `text` (the whole reply
/// pasted in works too).
pub fn parse_claim_link(text: &str) -> Result<(String, [u8; 32]), Error> {
    let bad = |why: &str| Error::ClaimLink(why.to_owned());
    let start = text
        .find("#address=")
        .ok_or_else(|| bad("there is no `#address=…&key=…` in it"))?;
    let fragment = text[start + 1..]
        .split(|c: char| c.is_whitespace() || c == '>' || c == '"')
        .next()
        .unwrap_or("");
    let (mut address, mut key) = (None, None);
    for pair in fragment.split('&') {
        match pair.split_once('=') {
            Some(("address", v)) => address = Some(decode(v).ok_or_else(|| bad("bad address"))?),
            Some(("key", v)) => key = Some(v),
            _ => {}
        }
    }
    let address = address.ok_or_else(|| bad("it has no address"))?;
    let hex = key.ok_or_else(|| bad("it has no key"))?;
    if hex.len() != 64 {
        return Err(bad("its key is cut short: copy the whole link"));
    }
    let mut secret = [0u8; 32];
    for (i, b) in secret.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)
            .map_err(|_| bad("its key isn't hexadecimal"))?;
    }
    Ok((address.to_lowercase(), secret))
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"@.-_~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Civil date (UTC) from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// `2026-01-08 00:00 UTC`: how people read a deadline in the text.
pub(crate) fn date(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!("{y}-{m:02}-{d:02} {:02}:{:02} UTC", s / 3600, s % 3600 / 60)
}

/// `Thu, 1 Jan 2026 00:00:00 +0000` (RFC 5322 §3.3).
fn rfc5322_date(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil(days);
    let s = secs.rem_euclid(86_400);
    format!(
        "{}, {d} {} {y} {:02}:{:02}:{:02} +0000",
        WEEKDAYS[days.rem_euclid(7) as usize],
        MONTHS[m as usize - 1],
        s / 3600,
        s % 3600 / 60,
        s % 60
    )
}

/// What a report says about a reply: never its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplyStatus {
    None,
    Sent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_dates() {
        assert_eq!(
            rfc5322_date(1_767_225_600_000),
            "Thu, 1 Jan 2026 00:00:00 +0000"
        );
        assert_eq!(
            date(1_767_225_600_000 + 7 * 86_400_000 + 90_000),
            "2026-01-08 00:01 UTC"
        );
        assert_eq!(
            rfc5322_date(951_782_400_000),
            "Tue, 29 Feb 2000 00:00:00 +0000"
        );
    }

    #[test]
    fn claim_links_round_trip() {
        let secret = [0xab; 32];
        let link = claim_link(
            "https://app.example.org/claim",
            "ana+r&d@example.org",
            &secret,
        );
        assert!(link.contains("address=ana%2Br%26d@example.org&key=abab"));
        let text = format!("Open this:\n\n{link}\n\nThanks");
        assert_eq!(
            parse_claim_link(&text).unwrap(),
            ("ana+r&d@example.org".to_owned(), secret)
        );
        let short = &link[..link.len() - 2];
        assert!(parse_claim_link(short).is_err());
        assert!(parse_claim_link("no link here").is_err());
    }
}
