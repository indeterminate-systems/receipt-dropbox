//! HTML to plain text, keeping the layout extraction relies on: blocks and table rows become
//! lines, cells in a row stay on one line.

/// Converts HTML to text: one line per block or table row, cells separated by two spaces;
/// `head`, `script` and `style` dropped; entities decoded.
pub fn to_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    let mut skip_until: Option<&'static str> = None;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            if skip_until.is_none() {
                push_text(&mut out, rest);
            }
            break;
        };
        if skip_until.is_none() {
            push_text(&mut out, &rest[..lt]);
        }
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |e| &after[e + 3..]);
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if let Some(end) = skip_until {
            if closing && name == end {
                skip_until = None;
            }
            continue;
        }
        match name.as_str() {
            "head" | "script" | "style" | "title" if !closing => {
                skip_until = Some(match name.as_str() {
                    "head" => "head",
                    "script" => "script",
                    "style" => "style",
                    _ => "title",
                });
            }
            "br" | "p" | "div" | "tr" | "li" | "table" | "h1" | "h2" | "h3" | "h4" | "h5"
            | "h6" | "section" | "header" | "footer" | "ul" | "ol" | "hr" | "address" => {
                newline(&mut out)
            }
            "td" | "th" if !closing && !out.ends_with('\n') && !out.is_empty() => {
                out.push_str("  ");
            }
            _ => {}
        }
    }
    let lines: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn newline(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Appends text with whitespace collapsed (HTML ignores line breaks in text) and entities decoded.
fn push_text(out: &mut String, text: &str) {
    let decoded = decode_entities(text);
    for c in decoded.chars() {
        let c = if c == '\u{a0}' { ' ' } else { c };
        if c.is_whitespace() {
            if !out.is_empty() && !out.ends_with([' ', '\n']) {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
}

const ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", "\u{a0}"),
    ("euro", "€"),
    ("pound", "£"),
    ("yen", "¥"),
    ("cent", "¢"),
    ("bull", "•"),
    ("middot", "·"),
    ("ndash", "–"),
    ("mdash", "—"),
    ("copy", "©"),
    ("reg", "®"),
    ("aacute", "á"),
    ("eacute", "é"),
    ("iacute", "í"),
    ("oacute", "ó"),
    ("uacute", "ú"),
    ("ntilde", "ñ"),
    ("Aacute", "Á"),
    ("Eacute", "É"),
    ("Iacute", "Í"),
    ("Oacute", "Ó"),
    ("Uacute", "Ú"),
    ("Ntilde", "Ñ"),
    ("uuml", "ü"),
];

pub(crate) fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let semi = rest.find(';').filter(|&i| i <= 12);
        let decoded = semi.and_then(|semi| {
            let name = &rest[1..semi];
            let c = if let Some(num) = name.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse().ok(),
                };
                code.and_then(char::from_u32).map(String::from)
            } else {
                ENTITIES
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| (*v).to_owned())
            };
            c.map(|c| (c, semi + 1))
        });
        match decoded {
            Some((text, len)) => {
                out.push_str(&text);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_become_lines_and_cells_stay_together() {
        let html = "<html><head><title>x</title><style>td{}</style></head><body>\
            <h1>Shop</h1><table><tr><td>Total</td><td>$1.00</td></tr></table>\
            <p>Caf&eacute; &amp; more&nbsp;&#8364;2</p><!-- note --></body></html>";
        assert_eq!(to_text(html), "Shop\nTotal  $1.00\nCafé & more €2\n");
    }
}
