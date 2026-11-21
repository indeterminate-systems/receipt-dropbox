//! The receiving server's verdict on a message (RFC 8601 `Authentication-Results`).
//!
//! Intake doesn't check SPF, DKIM or DMARC itself: it has no DNS. The receiving server (Cloudflare
//! Email Routing) checks them and prepends its results as the topmost `Authentication-Results`
//! field. Fields further down may have been written by the sender, so only the first counts, and
//! only if it names the expected server (its `authserv-id`).

/// One method's verdict: `(method, result, properties)`, properties as `(name, value)` like
/// `("header.from", "example.org")`.
pub(crate) type Method = (String, String, Vec<(String, String)>);

/// The checks one `Authentication-Results` field reports, by method (`spf`, `dkim`, `dmarc`).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Results {
    pub authserv_id: String,
    pub methods: Vec<Method>,
}

impl Results {
    /// The DMARC result and the `header.from` domain it was for, if the server said.
    pub fn dmarc(&self) -> Option<(&str, Option<&str>)> {
        self.methods
            .iter()
            .find(|(m, _, _)| m == "dmarc")
            .map(|(_, result, props)| {
                let domain = props
                    .iter()
                    .find(|(n, _)| n == "header.from")
                    .map(|(_, v)| v.as_str());
                (result.as_str(), domain)
            })
    }
}

/// Parses one field's value. Comments (`(…)`) are dropped; `reason=` and other properties are
/// kept as properties.
pub(crate) fn parse(value: &str) -> Results {
    let text = strip_comments(value);
    let mut parts = text.split(';');
    let authserv_id = parts
        .next()
        .and_then(|p| p.split_whitespace().next())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut methods = Vec::new();
    for part in parts {
        let mut words = part.split_whitespace();
        let Some((method, result)) = words.next().and_then(|w| w.split_once('=')) else {
            continue; // `none`, or an empty clause after a trailing `;`
        };
        // `dkim/1=pass` has a version; the method name is what matters.
        let method = method
            .split('/')
            .next()
            .unwrap_or(method)
            .to_ascii_lowercase();
        let props = words
            .filter_map(|w| w.split_once('='))
            .map(|(n, v)| {
                (
                    n.to_ascii_lowercase(),
                    v.trim_matches('"').to_ascii_lowercase(),
                )
            })
            .collect();
        methods.push((method, result.to_ascii_lowercase(), props));
    }
    Results {
        authserv_id,
        methods,
    }
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let (mut depth, mut quoted) = (0u32, false);
    for c in s.chars() {
        match c {
            '"' if depth == 0 => {
                quoted = !quoted;
                out.push(c);
            }
            '(' if !quoted => depth += 1,
            ')' if !quoted && depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cloudflare_s_results() {
        let r = parse(
            "mx.cloudflare.net; dkim=pass header.d=example.org header.s=s1 header.b=AbC; \
             spf=pass (mx.cloudflare.net: domain of a@example.org designates 1.2.3.4 as permitted sender) \
             smtp.mailfrom=a@example.org; dmarc=pass header.from=example.org",
        );
        assert_eq!(r.authserv_id, "mx.cloudflare.net");
        assert_eq!(r.dmarc(), Some(("pass", Some("example.org"))));
        assert_eq!(r.methods.len(), 3);
    }

    #[test]
    fn tolerates_versions_none_and_missing_dmarc() {
        let r = parse("mx.example 1; none");
        assert_eq!(r.authserv_id, "mx.example");
        assert_eq!(r.dmarc(), None);
        let r = parse("mx.example; dkim/1=fail; DMARC=Fail");
        assert_eq!(r.dmarc(), Some(("fail", None)));
    }
}
