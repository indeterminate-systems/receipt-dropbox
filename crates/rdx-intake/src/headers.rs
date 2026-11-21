//! Just enough RFC 5322 to triage a message: the header block, unfolded, and the `From` address.
//! The body is never read here: intake encrypts the message whole.

/// The header fields in order, names lowercased, folded lines joined.
pub(crate) fn parse(raw: &[u8]) -> Vec<(String, String)> {
    let end = find(raw, b"\r\n\r\n")
        .or_else(|| find(raw, b"\n\n"))
        .unwrap_or(raw.len());
    // Header fields are ASCII; anything else (raw UTF-8 in a display name) is kept lossily, as
    // only addresses and tokens are read.
    let block = String::from_utf8_lossy(&raw[..end]);
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in block.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = fields.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            fields.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    fields
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

pub(crate) fn first<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// Why a `From` field can't be used.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FromError {
    Missing,
    Several,
    Malformed(String),
}

/// The one mailbox in `From`, lowercased. DMARC authenticates this address's domain, so it is
/// the sender intake trusts, not the SMTP envelope sender.
pub(crate) fn from_address(fields: &[(String, String)]) -> Result<String, FromError> {
    let mut froms = fields.iter().filter(|(n, _)| n == "from");
    let (_, value) = froms.next().ok_or(FromError::Missing)?;
    if froms.next().is_some() {
        return Err(FromError::Several);
    }
    // Drop quoted display names and comments, which may contain `,`, `<` or `@`.
    let mut bare = String::new();
    let (mut quoted, mut depth, mut escaped) = (false, 0u32, false);
    for c in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted || depth > 0 => escaped = true,
            '"' if depth == 0 => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted && depth > 0 => depth -= 1,
            _ if quoted || depth > 0 => {}
            _ => bare.push(c),
        }
    }
    if bare.contains(',') || bare.contains(';') {
        return Err(FromError::Several);
    }
    let addr = match (bare.find('<'), bare.rfind('>')) {
        (Some(open), Some(close)) if open < close => &bare[open + 1..close],
        (None, None) => bare.as_str(),
        _ => return Err(FromError::Malformed(value.clone())),
    };
    let addr = addr.trim().to_lowercase();
    match addr.split_once('@') {
        Some((local, domain))
            if !local.is_empty()
                && domain.contains('.')
                && !domain.contains('@')
                && !addr
                    .chars()
                    .any(|c| c.is_whitespace() || c == '/' || c == '<') =>
        {
            Ok(addr)
        }
        _ => Err(FromError::Malformed(value.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from(v: &str) -> Result<String, FromError> {
        from_address(&parse(format!("From: {v}\r\n\r\nbody").as_bytes()))
    }

    #[test]
    fn unfolds_and_lowercases_names() {
        let f = parse(b"Subject: a\r\n  b\r\nX-Y: c\r\n\r\nSubject: body\r\n");
        assert_eq!(
            f,
            vec![("subject".into(), "a b".into()), ("x-y".into(), "c".into())]
        );
    }

    #[test]
    fn reads_the_from_address() {
        assert_eq!(from("Ana@Example.org").unwrap(), "ana@example.org");
        assert_eq!(
            from("\"Ruiz, Ana <x@y.z>\" <ana@example.org>").unwrap(),
            "ana@example.org"
        );
        assert_eq!(
            from("ana@example.org (Ana, at home)").unwrap(),
            "ana@example.org"
        );
        assert_eq!(from("a@x.org, b@x.org"), Err(FromError::Several));
        assert!(matches!(from("<nobody>"), Err(FromError::Malformed(_))));
        assert_eq!(
            from_address(&parse(b"To: a@b.c\r\n\r\n")),
            Err(FromError::Missing)
        );
    }
}
