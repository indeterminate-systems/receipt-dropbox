//! CFDI 4.0 (Mexican electronic invoice) XML: the receipt fields, plus the fiscal data piece 15
//! needs (folio fiscal, issuer and receiver RFC, payment form). Reads attributes of the few
//! elements it needs; validating the seal and SAT status is piece 15's job.

use rdx_model::{Currency, Date, Money, ReceiptKind};

use crate::{Fields, Fiscal, Found, Hints, ReceiptExtractor, html};

pub struct Cfdi;

impl ReceiptExtractor for Cfdi {
    fn name(&self) -> &'static str {
        "cfdi"
    }

    fn detect(&self, text: &str, media_type: &str) -> bool {
        media_type.ends_with("xml") && element(text, "Comprobante").is_some()
    }

    fn extract(&self, text: &str, _media_type: &str, _hints: &Hints) -> Found {
        let mut found = Found::default();
        let Some(comprobante) = element(text, "Comprobante") else {
            return found;
        };
        let emisor = element(text, "Emisor").unwrap_or_default();
        let receptor = element(text, "Receptor").unwrap_or_default();
        let timbre = element(text, "TimbreFiscalDigital").unwrap_or_default();
        let get = |attrs: &[(String, String)], name: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_empty())
        };

        let version = get(&comprobante, "Version").unwrap_or_default();
        if !version.starts_with('4') {
            found.warnings.push(format!(
                "CFDI version {version}; fields are read as in 4.0, check them"
            ));
        }
        let cfdi_type = get(&comprobante, "TipoDeComprobante").unwrap_or_default();
        let kind = match cfdi_type.as_str() {
            "I" => Some(ReceiptKind::Bill),
            "E" => Some(ReceiptKind::Refund),
            "P" => {
                found.warnings.push(
                    "a payment complement (tipo P) records a payment against earlier CFDIs; match those CFDIs, not this one"
                        .into(),
                );
                None
            }
            other => {
                found.warnings.push(format!(
                    "CFDI of type `{other}` is not a purchase (I) or credit note (E)"
                ));
                None
            }
        };

        let date = get(&comprobante, "Fecha").and_then(|f| f.get(..10)?.parse::<Date>().ok());
        let total = match (get(&comprobante, "Total"), get(&comprobante, "Moneda")) {
            (Some(total), Some(code)) => match Currency::new(&code) {
                Ok(currency) => match money(&total, currency) {
                    Some(m) => Some(m),
                    None => {
                        found
                            .warnings
                            .push(format!("CFDI Total `{total}` doesn't fit {currency}"));
                        None
                    }
                },
                Err(_) => {
                    found.warnings.push(format!(
                        "CFDI Moneda `{code}` is not a currency this system knows"
                    ));
                    found.currency_unknown = true;
                    None
                }
            },
            _ => None,
        };
        let reference = match (get(&comprobante, "Serie"), get(&comprobante, "Folio")) {
            (Some(s), Some(f)) => Some(format!("{s}-{f}")),
            (None, Some(f)) => Some(f),
            _ => None,
        };
        found.fields = Fields {
            merchant: get(&emisor, "Nombre").or_else(|| get(&emisor, "Rfc")),
            date,
            total,
            reference,
            kind,
        };
        match get(&timbre, "UUID") {
            Some(uuid) => {
                found.fiscal = Some(Fiscal {
                    uuid,
                    version,
                    cfdi_type,
                    issuer_rfc: get(&emisor, "Rfc").unwrap_or_default(),
                    receiver_rfc: get(&receptor, "Rfc").unwrap_or_default(),
                    receiver_name: get(&receptor, "Nombre"),
                    payment_form: get(&comprobante, "FormaPago"),
                    payment_method: get(&comprobante, "MetodoPago"),
                })
            }
            None => found
                .warnings
                .push("the CFDI has no TimbreFiscalDigital: it was never stamped by a PAC, so it is not a valid invoice".into()),
        }
        found
    }
}

/// CFDI amounts may carry up to six decimals; zeros beyond the currency's exponent are dropped,
/// anything else doesn't fit.
fn money(text: &str, currency: Currency) -> Option<Money> {
    let exp = currency.exponent() as usize;
    let trimmed = match text.split_once('.') {
        Some((whole, frac)) if frac.len() > exp => {
            if frac[exp..].bytes().any(|b| b != b'0') {
                return None;
            }
            if exp == 0 {
                whole.to_owned()
            } else {
                format!("{whole}.{}", &frac[..exp])
            }
        }
        _ => text.to_owned(),
    };
    Money::parse(&trimmed, currency).ok()
}

/// Attributes of the first element with this local name, whatever its namespace prefix.
fn element(xml: &str, local: &str) -> Option<Vec<(String, String)>> {
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        let local_name = name.rsplit(':').next().unwrap_or(name);
        if local_name != local {
            continue;
        }
        let end = rest.find('>')?;
        return Some(attributes(&rest[name_end..end]));
    }
    None
}

fn attributes(mut s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    while let Some(eq) = s.find('=') {
        let name = s[..eq].trim().to_owned();
        let after = s[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            break;
        };
        let Some(close) = after[1..].find(quote) else {
            break;
        };
        out.push((name, html::decode_entities(&after[1..1 + close])));
        s = &after[close + 2..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_decimal_totals() {
        let mxn = Currency::new("MXN").unwrap();
        assert_eq!(money("1160.000000", mxn), Some(Money::new(116000, mxn)));
        assert_eq!(money("1160.5", mxn), Some(Money::new(116050, mxn)));
        assert_eq!(money("1160.123", mxn), None);
    }

    #[test]
    fn attributes_with_entities() {
        assert_eq!(
            attributes(r#" Nombre="A &amp; B" Rfc='X'"#),
            vec![
                ("Nombre".to_owned(), "A & B".to_owned()),
                ("Rfc".to_owned(), "X".to_owned())
            ]
        );
    }
}
