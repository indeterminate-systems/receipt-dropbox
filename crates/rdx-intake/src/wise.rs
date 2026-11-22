//! Wise transfer links. Supporters who pay through Wise often share the transfer from the app,
//! which sends a tracking link (`https://wise.com/<lang>/track/<id>`) and no receipt. Intake never
//! opens links: the page behind one shows payment details a server would read in plaintext, and
//! it can change or expire. It asks for the transfer's PDF receipt instead, which the receipt
//! extractor reads like any other PDF.

/// Hosts Wise's tracking links live on, then and now.
const HOSTS: &[&str] = &[
    "wise.com",
    "www.wise.com",
    "transferwise.com",
    "www.transferwise.com",
];

/// Inline images below this size are signature logos, not photographed receipts.
const LOGO_BYTES: usize = 32 * 1024;

/// Whether `raw` carries a Wise tracking link and no document that could be its receipt. Reads
/// the message in the caller's memory only; nothing of it is kept.
pub(crate) fn link_without_receipt(raw: &[u8]) -> bool {
    let email = rdx_receipt::parse_email(&String::from_utf8_lossy(raw));
    let documents = email
        .attachments
        .iter()
        .filter(|a| !(a.media_type.starts_with("image/") && a.size < LOGO_BYTES))
        .count()
        + email.xml.len();
    documents == 0 && has_tracking_link(&email.text)
}

fn has_tracking_link(text: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || "<>\"'()[]".contains(c))
        .any(is_tracking_link)
}

/// `https://wise.com/gb/track/…`, with or without the scheme or a language segment.
fn is_tracking_link(token: &str) -> bool {
    let rest = token
        .strip_prefix("https://")
        .or_else(|| token.strip_prefix("http://"))
        .unwrap_or(token);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.split(['?', '#']).next().unwrap_or("");
    HOSTS.iter().any(|h| h.eq_ignore_ascii_case(host))
        && path
            .split('/')
            .take(2)
            .any(|s| s.eq_ignore_ascii_case("track"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_tracking_links_only() {
        for yes in [
            "https://wise.com/gb/track/abc?utm_source=share",
            "https://wise.com/track/abc",
            "wise.com/es/track/abc",
            "https://transferwise.com/gb/track/abc",
            "https://WWW.Wise.com/gb/track/abc",
        ] {
            assert!(is_tracking_link(yes), "{yes}");
        }
        for no in [
            "https://wise.com/invite/u/ana",
            "https://wise.com.evil.example/gb/track/abc",
            "https://notwise.com/gb/track/abc",
            "https://wise.com/gb/blog/how-to-track",
            "https://wise.com/a/b/track/abc",
        ] {
            assert!(!is_tracking_link(no), "{no}");
        }
        assert!(has_tracking_link("see <https://wise.com/gb/track/abc>."));
    }
}
