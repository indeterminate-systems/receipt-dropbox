//! The proof policy (`nickel/rdx/fiscal.ncl` `Policy`), which has every default; this side only
//! reads what Nickel exports.

use rdx_model::Money;
use serde::{Deserialize, Serialize};

/// What kind of organization this is (docs/compliance.md R0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Company,
    Nonprofit,
    DonatariaAutorizada,
    Individual,
    InformalGroup,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Company => "company",
            Kind::Nonprofit => "nonprofit",
            Kind::DonatariaAutorizada => "donataria-autorizada",
            Kind::Individual => "individual",
            Kind::InformalGroup => "informal-group",
        }
    }

    /// Non-profits pay ISR on expenses without a CFDI or outside their purpose.
    pub fn is_nonprofit(&self) -> bool {
        matches!(self, Kind::Nonprofit | Kind::DonatariaAutorizada)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub kind: Kind,
    pub sat_registered: bool,
    pub fiscal_year: i32,
    #[serde(default)]
    pub profile: Option<Profile>,
    pub rules: Rules,
}

/// The organization as a CFDI's receptor must name it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub rfc: String,
    pub name: String,
    pub postcode: String,
    pub regime: String,
    pub uso_cfdi: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub confirmed: bool,
    pub cash: CashRule,
    pub fuel_product_codes: Vec<String>,
    pub cfdi_window: Window,
    pub foreign_invoice_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashRule {
    pub forms: Vec<String>,
    pub threshold: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub days_before: i64,
    pub days_after: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error(
        "proof policy `{0}` needs `profile` (the organization's RFC, name, postcode and regime as the SAT has them); a group with no RFC is `informal-group`"
    )]
    NoProfile(&'static str),
}

impl Policy {
    /// Whether an expense needs a CFDI to the organization's RFC: everyone with an RFC, except an
    /// individual not yet registered for the activity, who can't receive one.
    pub fn requires_cfdi(&self) -> bool {
        match self.kind {
            Kind::InformalGroup => false,
            Kind::Individual => self.sat_registered,
            _ => true,
        }
    }

    pub fn validate(&self) -> Result<(), PolicyError> {
        if self.requires_cfdi() && self.profile.is_none() {
            return Err(PolicyError::NoProfile(self.kind.as_str()));
        }
        Ok(())
    }
}
