//! CFDI 4.0 XML: the fields fiscal checks need, read with a real XML parser.
//!
//! `rdx-receipt` already reads a CFDI into a receipt (merchant, date, total). This reads the rest:
//! the receptor's postcode, regime and UsoCFDI, the concepts' product codes (fuel), the Pagos 2.0
//! complement and *a cuenta de terceros* concepts. It doesn't verify the seal: that needs the
//! issuer's certificate, and the SAT's status service (R3) answers the question that matters.

use rdx_model::{Currency, Date, Money};
use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};

/// A stamped CFDI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cfdi {
    /// The folio fiscal (`TimbreFiscalDigital/@UUID`), in upper case.
    pub uuid: String,
    pub version: String,
    /// `TipoDeComprobante`: I (ingreso), E (egreso), P (pago), T (traslado), N (nómina).
    pub cfdi_type: String,
    /// The date part of `Fecha`.
    pub issued: Date,
    pub issuer: Issuer,
    pub receiver: Receiver,
    /// `None` for Moneda `XXX`, which payment complements use: they carry no total of their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<Money>,
    /// `TipoCambio`, as written: pesos per unit of `Moneda`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange_rate: Option<String>,
    /// SAT catalog `c_FormaPago`: `01` cash, `03` transfer, `04` credit card, `28` debit card,
    /// `99` to be defined (PPD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment_form: Option<String>,
    /// `PUE` (paid when issued) or `PPD` (paid later, proven by a Pagos complement).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment_method: Option<String>,
    #[serde(default)]
    pub concepts: Vec<Concept>,
    /// For a Pagos complement (type P): the documents it pays.
    #[serde(default)]
    pub payments: Vec<Payment>,
    /// Things worth knowing that don't stop it being read, such as a version other than 4.0.
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issuer {
    pub rfc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receiver {
    pub rfc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `DomicilioFiscalReceptor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postcode: Option<String>,
    /// `RegimenFiscalReceptor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regime: Option<String>,
    /// `UsoCFDI`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uso: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Concept {
    /// `ClaveProdServ`.
    pub product_code: String,
    pub description: String,
    /// `Importe`, in the CFDI's currency.
    pub amount: Money,
    /// `ACuentaTerceros/@RfcACuentaTerceros`: the concept was billed on someone else's behalf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third_party_rfc: Option<String>,
}

/// One `DoctoRelacionado` of a Pagos 2.0 complement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    /// The paid CFDI's UUID, in upper case.
    pub uuid: String,
    /// `ImpPagado`, in `MonedaDR`.
    pub amount: Money,
    /// The date part of `Pago/@FechaPago`.
    pub date: Date,
    /// `Pago/@FormaDePagoP`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CfdiError {
    #[error("not well-formed XML: {0}")]
    Xml(String),
    #[error("not a CFDI: the root element isn't `Comprobante`")]
    NotACfdi,
    #[error(
        "the CFDI has no TimbreFiscalDigital: it was never stamped by a PAC, so it is not a valid invoice"
    )]
    Unstamped,
    #[error("the CFDI lacks `{0}`")]
    Missing(&'static str),
    #[error("`{field}` is `{value}`, which doesn't fit: {reason}")]
    Invalid {
        field: &'static str,
        value: String,
        reason: String,
    },
}

impl Cfdi {
    pub fn parse(xml: &str) -> Result<Cfdi, CfdiError> {
        let doc = Document::parse(xml).map_err(|e| CfdiError::Xml(e.to_string()))?;
        let root = doc.root_element();
        if root.tag_name().name() != "Comprobante" {
            return Err(CfdiError::NotACfdi);
        }
        let timbre = descendant(root, "TimbreFiscalDigital").ok_or(CfdiError::Unstamped)?;
        let uuid = attr(timbre, "UUID")
            .ok_or(CfdiError::Unstamped)?
            .to_ascii_uppercase();

        let version = attr(root, "Version").ok_or(CfdiError::Missing("Version"))?;
        let mut warnings = Vec::new();
        if !version.starts_with('4') {
            warnings.push(format!(
                "CFDI version {version}; its fields are read as in 4.0, so check them"
            ));
        }
        let cfdi_type =
            attr(root, "TipoDeComprobante").ok_or(CfdiError::Missing("TipoDeComprobante"))?;
        let issued = date(root, "Fecha")?;
        let currency = attr(root, "Moneda").ok_or(CfdiError::Missing("Moneda"))?;
        let currency = if currency == "XXX" {
            None
        } else {
            Some(Currency::new(&currency).map_err(|e| CfdiError::Invalid {
                field: "Moneda",
                value: currency.clone(),
                reason: e.to_string(),
            })?)
        };
        let total = match currency {
            Some(c) => Some(money(root, "Total", c)?),
            None => None,
        };

        let emisor = child(root, "Emisor").ok_or(CfdiError::Missing("Emisor"))?;
        let receptor = child(root, "Receptor").ok_or(CfdiError::Missing("Receptor"))?;
        let issuer = Issuer {
            rfc: attr(emisor, "Rfc").ok_or(CfdiError::Missing("Emisor/@Rfc"))?,
            name: attr(emisor, "Nombre"),
            regime: attr(emisor, "RegimenFiscal"),
        };
        let receiver = Receiver {
            rfc: attr(receptor, "Rfc").ok_or(CfdiError::Missing("Receptor/@Rfc"))?,
            name: attr(receptor, "Nombre"),
            postcode: attr(receptor, "DomicilioFiscalReceptor"),
            regime: attr(receptor, "RegimenFiscalReceptor"),
            uso: attr(receptor, "UsoCFDI"),
        };

        let mut concepts = Vec::new();
        if let (Some(conceptos), Some(c)) = (child(root, "Conceptos"), currency) {
            for concepto in conceptos.children().filter(|n| is(n, "Concepto")) {
                concepts.push(Concept {
                    product_code: attr(concepto, "ClaveProdServ").unwrap_or_default(),
                    description: attr(concepto, "Descripcion").unwrap_or_default(),
                    amount: money(concepto, "Importe", c)?,
                    third_party_rfc: child(concepto, "ACuentaTerceros")
                        .and_then(|t| attr(t, "RfcACuentaTerceros")),
                });
            }
        }

        let mut payments = Vec::new();
        for pago in root.descendants().filter(|n| is(n, "Pago")) {
            let paid_on = date(pago, "FechaPago")?;
            let form = attr(pago, "FormaDePagoP");
            for docto in pago.children().filter(|n| is(n, "DoctoRelacionado")) {
                let code = attr(docto, "MonedaDR")
                    .or_else(|| attr(pago, "MonedaP"))
                    .ok_or(CfdiError::Missing("DoctoRelacionado/@MonedaDR"))?;
                let c = Currency::new(&code).map_err(|e| CfdiError::Invalid {
                    field: "MonedaDR",
                    value: code.clone(),
                    reason: e.to_string(),
                })?;
                payments.push(Payment {
                    uuid: attr(docto, "IdDocumento")
                        .ok_or(CfdiError::Missing("DoctoRelacionado/@IdDocumento"))?
                        .to_ascii_uppercase(),
                    amount: money(docto, "ImpPagado", c)?,
                    date: paid_on,
                    form: form.clone(),
                });
            }
        }

        Ok(Cfdi {
            uuid,
            version,
            cfdi_type,
            issued,
            issuer,
            receiver,
            total,
            exchange_rate: attr(root, "TipoCambio"),
            payment_form: attr(root, "FormaPago"),
            payment_method: attr(root, "MetodoPago"),
            concepts,
            payments,
            warnings,
        })
    }
}

/// CFDI namespaces changed between versions (cfd/3, cfd/4), so elements match by local name.
fn is(n: &Node, local: &str) -> bool {
    n.is_element() && n.tag_name().name() == local
}

fn child<'a, 'i>(n: Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is(c, local))
}

fn descendant<'a, 'i>(n: Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> {
    n.descendants().find(|c| is(c, local))
}

fn attr(n: Node, name: &str) -> Option<String> {
    n.attribute(name)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn date(n: Node, field: &'static str) -> Result<Date, CfdiError> {
    let text = attr(n, field).ok_or(CfdiError::Missing(field))?;
    text.get(..10)
        .and_then(|d| d.parse().ok())
        .ok_or_else(|| CfdiError::Invalid {
            field,
            value: text.clone(),
            reason: "expected an ISO date and time, like 2026-03-12T11:05:33".into(),
        })
}

fn money(n: Node, field: &'static str, currency: Currency) -> Result<Money, CfdiError> {
    let text = attr(n, field).ok_or(CfdiError::Missing(field))?;
    parse_amount(&text, currency).ok_or_else(|| CfdiError::Invalid {
        field,
        value: text.clone(),
        reason: format!("not an amount in {currency}"),
    })
}

/// CFDI amounts may carry up to six decimals. Zeros past the currency's exponent are dropped;
/// anything else is rounded half up, as the CFDI standard rounds `Importe`.
pub(crate) fn parse_amount(text: &str, currency: Currency) -> Option<Money> {
    let exp = currency.exponent() as usize;
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let mut digits = String::from(whole);
    let kept: String = frac
        .chars()
        .chain(std::iter::repeat('0'))
        .take(exp)
        .collect();
    digits.push_str(&kept);
    let mut minor: i64 = digits.parse().ok()?;
    if frac.len() > exp && frac.as_bytes()[exp] >= b'5' {
        minor = minor.checked_add(1)?;
    }
    Some(Money::new(minor, currency))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts() {
        let mxn = Currency::new("MXN").unwrap();
        assert_eq!(
            parse_amount("1160.000000", mxn),
            Some(Money::new(116000, mxn))
        );
        assert_eq!(parse_amount("1160.5", mxn), Some(Money::new(116050, mxn)));
        assert_eq!(parse_amount("22.3333", mxn), Some(Money::new(2233, mxn)));
        assert_eq!(parse_amount("0.125", mxn), Some(Money::new(13, mxn)));
        assert_eq!(parse_amount("12", mxn), Some(Money::new(1200, mxn)));
        assert_eq!(parse_amount("-1", mxn), None);
        assert_eq!(parse_amount("1e3", mxn), None);
    }
}
