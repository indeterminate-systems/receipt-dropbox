//! Whether each expense has the proof the organization's policy asks for (docs/compliance.md R0
//! to R6 and R17).
//!
//! Every problem is a [`Finding`] with a stable `code`, and the worst of them decides the
//! expense's [`Proof`]. Nothing here decides a tax question: where the law is unsettled the
//! finding says "review" and names who should look.

use std::collections::BTreeMap;

use rdx_model::{Currency, Money, Receipt, ReceiptId, ReceiptKind};
use serde::{Deserialize, Serialize};

use crate::cfdi::{Cfdi, CfdiError};
use crate::policy::{Kind, Policy, PolicyError, Profile};

/// RFC the SAT reserves for sales to the general public.
pub const PUBLICO_EN_GENERAL: &str = "XAXX010101000";

/// One expense to check: a receipt and whatever fiscal evidence goes with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expense {
    pub receipt: Receipt,
    /// The CFDI's XML text.
    #[serde(default)]
    pub cfdi: Option<String>,
    /// XML of Pagos complements that pay the CFDI (for `PPD` invoices).
    #[serde(default)]
    pub payments: Vec<String>,
    /// A foreign vendor's invoice, when the vendor can't issue a CFDI (R17).
    #[serde(default)]
    pub foreign: Option<ForeignInvoice>,
    /// The SAT status check of the CFDI (R3), from whatever host ran it.
    #[serde(default)]
    pub sat: Option<SatCheck>,
    /// How the supporter paid, from the statement line the receipt matched.
    #[serde(default)]
    pub paid_with: Option<PaidWith>,
    /// What the expense was for (non-profits: inside the organization's purpose; US: business
    /// purpose).
    #[serde(default)]
    pub purpose: Option<String>,
    /// For a donataria autorizada: administration or mission.
    #[serde(default)]
    pub class: Option<Class>,
}

impl Expense {
    pub fn new(receipt: Receipt) -> Self {
        Expense {
            receipt,
            cfdi: None,
            payments: Vec::new(),
            foreign: None,
            sat: None,
            paid_with: None,
            purpose: None,
            class: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForeignInvoice {
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub tax_id: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// The organization as the invoice names it.
    #[serde(default)]
    pub buyer: Option<String>,
    /// ISO 3166 country of the vendor.
    pub country: String,
    /// A foreign digital platform registered with the SAT (LIVA art. 18-B), which charges IVA
    /// itself.
    #[serde(default)]
    pub digital_platform: bool,
}

impl ForeignInvoice {
    fn field(&self, name: &str) -> Option<&str> {
        match name {
            "vendor" => self.vendor.as_deref(),
            "tax_id" => self.tax_id.as_deref(),
            "address" => self.address.as_deref(),
            "description" => self.description.as_deref(),
            "buyer" => self.buyer.as_deref(),
            _ => None,
        }
        .filter(|v| !v.trim().is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SatCheck {
    pub status: SatStatus,
    #[serde(default)]
    pub efos: Efos,
    /// When the check ran (RFC 3339), kept as the ledger's `sat-checked`.
    pub checked: String,
}

/// The SAT verification service's `Estado`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SatStatus {
    Vigente,
    Cancelado,
    NoEncontrado,
}

impl SatStatus {
    fn as_str(&self) -> &'static str {
        match self {
            SatStatus::Vigente => "vigente",
            SatStatus::Cancelado => "cancelado",
            SatStatus::NoEncontrado => "no-encontrado",
        }
    }
}

/// The issuer's standing on the SAT's art. 69-B list (EFOS).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Efos {
    #[default]
    None,
    Presunto,
    Definitivo,
    Desvirtuado,
    SentenciaFavorable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaidWith {
    Cash,
    Card,
    Transfer,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Class {
    Administration,
    Mission,
}

impl Class {
    fn as_str(&self) -> &'static str {
        match self {
            Class::Administration => "administration",
            Class::Mission => "mission",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Recorded for the audit trail; nothing to do.
    Info,
    /// A person (often a contador) should look before the expense counts.
    Review,
    /// The evidence doesn't meet the policy.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Proof {
    /// The evidence meets the policy.
    Supported,
    /// Nothing is wrong that a person can't settle, but someone must look.
    NeedsReview,
    /// The policy needs a CFDI and there is none: the statement line shows a payment happened,
    /// which doesn't make it a supported expense (R4).
    PaymentEvidenceOnly,
    NotSupported,
}

impl Proof {
    fn as_str(&self) -> &'static str {
        match self {
            Proof::Supported => "supported",
            Proof::NeedsReview => "needs_review",
            Proof::PaymentEvidenceOnly => "payment_evidence_only",
            Proof::NotSupported => "not_supported",
        }
    }
}

/// What the proof rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Cfdi,
    ForeignInvoice,
    Receipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Assessment {
    pub receipt: ReceiptId,
    pub proof: Proof,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<Basis>,
    pub findings: Vec<Finding>,
    /// Ledger metadata for the expense entry (R6), keyed as the ledger will write it.
    pub meta: BTreeMap<&'static str, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicySummary {
    pub kind: Kind,
    pub fiscal_year: i32,
    pub rules_confirmed: bool,
    pub requires_cfdi: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub supported: usize,
    pub needs_review: usize,
    pub payment_evidence_only: usize,
    pub not_supported: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub policy: PolicySummary,
    /// About the policy itself, for every run.
    pub warnings: Vec<String>,
    pub assessments: Vec<Assessment>,
    pub summary: Summary,
}

/// Checks each expense against the policy.
pub fn check(policy: &Policy, expenses: &[Expense]) -> Result<Report, PolicyError> {
    policy.validate()?;
    let mut warnings = Vec::new();
    if !policy.rules.confirmed {
        warnings.push(format!(
            "The {} proof rules are not confirmed by a contador yet: treat these results as research, not advice (docs/compliance.md).",
            policy.fiscal_year
        ));
    }
    if policy.kind == Kind::Individual && !policy.sat_registered {
        warnings.push(
            "The owner is not registered with the SAT for this activity: CFDIs to their RFC aren't possible yet, so receipts are the proof. Unregistered business activity, above all with employees, is a compliance risk for the owner; see a contador (docs/compliance.md, open question 10).".into(),
        );
    }

    let mut summary = Summary::default();
    let assessments: Vec<Assessment> = expenses
        .iter()
        .map(|e| {
            let a = Checker::new(policy, e).run();
            match a.proof {
                Proof::Supported => summary.supported += 1,
                Proof::NeedsReview => summary.needs_review += 1,
                Proof::PaymentEvidenceOnly => summary.payment_evidence_only += 1,
                Proof::NotSupported => summary.not_supported += 1,
            }
            a
        })
        .collect();

    Ok(Report {
        policy: PolicySummary {
            kind: policy.kind,
            fiscal_year: policy.fiscal_year,
            rules_confirmed: policy.rules.confirmed,
            requires_cfdi: policy.requires_cfdi(),
        },
        warnings,
        assessments,
        summary,
    })
}

struct Checker<'a> {
    policy: &'a Policy,
    expense: &'a Expense,
    findings: Vec<Finding>,
    meta: BTreeMap<&'static str, String>,
    /// Severity of a CFDI problem: an error where the CFDI is the proof, a review where the
    /// receipt is and the CFDI is extra.
    hard: Severity,
}

impl<'a> Checker<'a> {
    fn new(policy: &'a Policy, expense: &'a Expense) -> Self {
        Checker {
            policy,
            expense,
            findings: Vec::new(),
            meta: BTreeMap::new(),
            hard: if policy.requires_cfdi() {
                Severity::Error
            } else {
                Severity::Review
            },
        }
    }

    fn find(
        &mut self,
        code: &'static str,
        severity: Severity,
        message: String,
        fix: Option<String>,
    ) {
        self.findings.push(Finding {
            code,
            severity,
            message,
            fix,
        });
    }

    fn receipt(&self) -> &'a Receipt {
        &self.expense.receipt
    }

    fn run(mut self) -> Assessment {
        let basis = match (&self.expense.cfdi, &self.expense.foreign) {
            (Some(xml), _) => self.check_cfdi(xml),
            (None, Some(foreign)) if self.policy.requires_cfdi() => {
                self.check_foreign(foreign);
                Some(Basis::ForeignInvoice)
            }
            (None, _) if self.policy.requires_cfdi() => {
                self.missing_cfdi();
                None
            }
            (None, _) => Some(Basis::Receipt),
        };
        // An unreadable CFDI says nothing more worth checking.
        let readable = !self
            .findings
            .iter()
            .any(|f| matches!(f.code, "cfdi-unreadable" | "cfdi-unstamped"));
        if readable {
            if self.expense.cfdi.is_none() {
                self.check_cash_without_cfdi();
            }
            self.check_purpose();
            self.check_year();
        }

        let proof = if self
            .findings
            .iter()
            .any(|f| f.severity == Severity::Error && f.code != "cfdi-missing")
        {
            Proof::NotSupported
        } else if self.findings.iter().any(|f| f.code == "cfdi-missing") {
            Proof::PaymentEvidenceOnly
        } else if self.findings.iter().any(|f| f.severity == Severity::Review) {
            Proof::NeedsReview
        } else {
            Proof::Supported
        };
        if let Some(purpose) = &self.expense.purpose {
            self.meta.insert("purpose", purpose.clone());
        }
        if let Some(class) = self.expense.class {
            self.meta.insert("class", class.as_str().into());
        }
        self.meta.insert("proof", proof.as_str().into());
        Assessment {
            receipt: self.receipt().id.clone(),
            proof,
            basis,
            findings: self.findings,
            meta: self.meta,
        }
    }

    fn check_cfdi(&mut self, xml: &str) -> Option<Basis> {
        let cfdi = match Cfdi::parse(xml) {
            Ok(c) => c,
            Err(CfdiError::Unstamped) => {
                self.find(
                    "cfdi-unstamped",
                    self.hard,
                    CfdiError::Unstamped.to_string(),
                    Some("Ask the issuer for the stamped XML (with its folio fiscal); a draft or a PDF printout isn't the invoice.".into()),
                );
                return self.fallback_basis();
            }
            Err(e) => {
                self.find(
                    "cfdi-unreadable",
                    self.hard,
                    format!("the CFDI can't be read: {e}"),
                    Some("Upload the XML file the issuer sent or the SAT portal provides, not the PDF.".into()),
                );
                return self.fallback_basis();
            }
        };
        self.meta.insert("cfdi", cfdi.uuid.clone());
        self.meta.insert("rfc-emisor", cfdi.issuer.rfc.clone());
        self.meta.insert("rfc-receptor", cfdi.receiver.rfc.clone());
        if let Some(f) = &cfdi.payment_form {
            self.meta.insert("forma-pago", f.clone());
        }
        if let Some(m) = &cfdi.payment_method {
            self.meta.insert("metodo-pago", m.clone());
        }
        for w in &cfdi.warnings {
            self.find("cfdi-version", Severity::Review, w.clone(), None);
        }

        self.check_type(&cfdi);
        self.check_amount_and_date(&cfdi);
        if self.policy.requires_cfdi() {
            self.check_receiver(&cfdi);
        } else {
            self.find(
                "cfdi-receiver",
                Severity::Info,
                format!(
                    "the CFDI names RFC {}{}; the organization has no RFC, so the receipt is the proof and the CFDI is kept as its holder's own record",
                    cfdi.receiver.rfc,
                    cfdi.receiver.name.as_deref().map(|n| format!(" ({n})")).unwrap_or_default()
                ),
                None,
            );
        }
        self.check_cash(&cfdi);
        self.check_ppd(&cfdi);
        self.check_sat();
        if self.policy.requires_cfdi() {
            Some(Basis::Cfdi)
        } else {
            Some(Basis::Receipt)
        }
    }

    fn fallback_basis(&self) -> Option<Basis> {
        (!self.policy.requires_cfdi()).then_some(Basis::Receipt)
    }

    fn check_type(&mut self, cfdi: &Cfdi) {
        let refund = self.receipt().kind == ReceiptKind::Refund;
        match cfdi.cfdi_type.as_str() {
            "I" if !refund => {}
            "E" if refund => {}
            "I" | "E" => self.find(
                "cfdi-kind-mismatch",
                self.hard,
                format!(
                    "the CFDI is of type {} ({}), but the receipt is {}",
                    cfdi.cfdi_type,
                    if cfdi.cfdi_type == "I" { "an invoice" } else { "a credit note" },
                    if refund { "a refund" } else { "a purchase" },
                ),
                None,
            ),
            other => self.find(
                "cfdi-not-an-expense",
                self.hard,
                format!(
                    "a CFDI of type {other} ({}) doesn't prove an expense; only type I (or E for a refund) does",
                    match other {
                        "P" => "a payment complement",
                        "T" => "a transfer of goods",
                        "N" => "payroll",
                        _ => "unknown",
                    }
                ),
                (other == "P").then(|| {
                    "Pass the invoice it pays as `cfdi` and this complement in `payments`.".into()
                }),
            ),
        }
    }

    fn check_amount_and_date(&mut self, cfdi: &Cfdi) {
        let receipt = self.receipt();
        if let Some(total) = cfdi.total {
            if total.currency() == receipt.total.currency() {
                if total != receipt.total {
                    self.find(
                        "cfdi-total-mismatch",
                        self.hard,
                        format!("the CFDI's total is {total}, the receipt's {}", receipt.total),
                        Some("Check that this CFDI is for this purchase; a CFDI covering several purchases needs each receipt matched to it.".into()),
                    );
                }
            } else {
                self.find(
                    "cfdi-currency-differs",
                    Severity::Review,
                    format!(
                        "the CFDI is in {} and the receipt in {}; compare them at the CFDI's exchange rate{}",
                        total.currency(),
                        receipt.total.currency(),
                        cfdi.exchange_rate.as_deref().map(|r| format!(" ({r})")).unwrap_or_default()
                    ),
                    None,
                );
            }
        }
        let window = &self.policy.rules.cfdi_window;
        let days = cfdi.issued.days_since(receipt.date);
        if days < -window.days_before || days > window.days_after {
            self.find(
                "cfdi-date-outside-window",
                Severity::Review,
                format!(
                    "the CFDI is dated {}, {} days {} the receipt ({}); the policy expects {} before to {} after",
                    cfdi.issued,
                    days.abs(),
                    if days < 0 { "before" } else { "after" },
                    receipt.date,
                    window.days_before,
                    window.days_after
                ),
                None,
            );
        }
    }

    fn check_receiver(&mut self, cfdi: &Cfdi) {
        let profile = self
            .policy
            .profile
            .as_ref()
            .expect("validated: requires_cfdi has a profile");
        let fix = Some(reissue(profile));
        let r = &cfdi.receiver;
        if r.rfc == PUBLICO_EN_GENERAL {
            self.find(
                "cfdi-to-general-public",
                Severity::Error,
                format!(
                    "the CFDI is issued to público en general ({PUBLICO_EN_GENERAL}), not to the organization's RFC {}",
                    profile.rfc
                ),
                fix,
            );
            return;
        }
        if r.rfc != profile.rfc {
            self.find(
                "cfdi-to-other-rfc",
                Severity::Error,
                format!(
                    "the CFDI is issued to RFC {}{}, not to the organization's RFC {}: it is that taxpayer's expense, not the organization's",
                    r.rfc,
                    r.name.as_deref().map(|n| format!(" ({n})")).unwrap_or_default(),
                    profile.rfc
                ),
                Some(format!(
                    "{} If the issuer can't, the supporter would have to invoice the organization themselves; ask a contador before doing that.",
                    reissue(profile)
                )),
            );
            return;
        }
        if r.name.as_deref().map(normalize) != Some(normalize(&profile.name)) {
            self.find(
                "receiver-name-differs",
                Severity::Review,
                format!(
                    "the CFDI names the organization `{}`, the profile `{}`; CFDI 4.0 requires the name exactly as the SAT has it",
                    r.name.as_deref().unwrap_or("(none)"),
                    profile.name
                ),
                Some("If the profile is out of date, correct it; otherwise ask the issuer to reissue.".into()),
            );
        }
        if r.postcode.as_deref() != Some(profile.postcode.as_str()) {
            self.find(
                "receiver-postcode-differs",
                Severity::Review,
                format!(
                    "the CFDI's receiver postcode is {}, the profile's {}",
                    r.postcode.as_deref().unwrap_or("(none)"),
                    profile.postcode
                ),
                None,
            );
        }
        if r.regime.as_deref() != Some(profile.regime.as_str()) {
            self.find(
                "receiver-regime-differs",
                Severity::Review,
                format!(
                    "the CFDI's receiver regime is {}, the profile's {}",
                    r.regime.as_deref().unwrap_or("(none)"),
                    profile.regime
                ),
                None,
            );
        }
        match r.uso.as_deref() {
            Some(uso) if profile.uso_cfdi.iter().any(|u| u == uso) => {}
            uso => self.find(
                "uso-cfdi-not-allowed",
                Severity::Error,
                format!(
                    "the CFDI's UsoCFDI is {}, but the organization accepts {}",
                    uso.unwrap_or("(none)"),
                    profile.uso_cfdi.join(", ")
                ),
                fix,
            ),
        }
    }

    /// R5: cash over the threshold, and fuel in cash at any amount.
    fn check_cash(&mut self, cfdi: &Cfdi) {
        let rules = &self.policy.rules;
        let Some(form) = cfdi.payment_form.as_deref() else {
            return;
        };
        let cash = rules.cash.forms.iter().any(|f| f == form);
        if let Some(paid_with) = self.expense.paid_with
            && cash != (paid_with == PaidWith::Cash)
            && paid_with != PaidWith::Other
        {
            self.find(
                "payment-form-differs",
                Severity::Review,
                format!(
                    "the CFDI says FormaPago {form}, but the payment was by {}",
                    serde_json::to_value(paid_with).unwrap().as_str().unwrap()
                ),
                None,
            );
        }
        if !cash {
            return;
        }
        if let Some(fuel) = cfdi.concepts.iter().find(|c| {
            rules
                .fuel_product_codes
                .iter()
                .any(|p| c.product_code.starts_with(p.as_str()))
        }) {
            self.find(
                "fuel-paid-cash",
                self.hard,
                format!(
                    "fuel (`{}`, ClaveProdServ {}) was paid in cash, which is never deductible, whatever the amount",
                    fuel.description, fuel.product_code
                ),
                Some("Pay for fuel by card or transfer.".into()),
            );
        }
        if let Some(total) = cfdi.total {
            self.cash_over_threshold(total, cfdi.exchange_rate.as_deref());
        }
    }

    fn check_cash_without_cfdi(&mut self) {
        if self.expense.paid_with == Some(PaidWith::Cash) {
            self.cash_over_threshold(self.receipt().total, None);
        }
    }

    fn cash_over_threshold(&mut self, total: Money, rate: Option<&str>) {
        let threshold = self.policy.rules.cash.threshold;
        let Some(amount) = convert(total, threshold.currency(), rate) else {
            self.find(
                "cash-threshold-unconverted",
                Severity::Review,
                format!(
                    "paid in cash, {total}, with no exchange rate to compare it to the {threshold} threshold"
                ),
                None,
            );
            return;
        };
        if amount.minor() > threshold.minor() {
            self.find(
                "cash-over-threshold",
                self.hard,
                format!(
                    "{total} was paid in cash; over {threshold}, a cash payment isn't deductible even with a CFDI (LISR art. 27 fr. III)"
                ),
                Some("Pay amounts over the threshold by card or transfer.".into()),
            );
        }
    }

    /// A PPD invoice is paid later; the payment is proven by Pagos complements.
    fn check_ppd(&mut self, cfdi: &Cfdi) {
        if cfdi.payment_method.as_deref() != Some("PPD") {
            return;
        }
        let Some(total) = cfdi.total else {
            return;
        };
        let mut paid = Money::zero(total.currency());
        for xml in &self.expense.payments {
            match Cfdi::parse(xml) {
                Ok(p) => {
                    for pay in p.payments.iter().filter(|p| p.uuid == cfdi.uuid) {
                        if let Ok(sum) = paid.checked_add(pay.amount) {
                            paid = sum;
                        }
                    }
                }
                Err(e) => self.find(
                    "payment-unreadable",
                    Severity::Review,
                    format!("a payment complement can't be read: {e}"),
                    None,
                ),
            }
        }
        if paid.minor() < total.minor() {
            self.find(
                "ppd-unpaid",
                Severity::Review,
                format!(
                    "the CFDI is PPD (paid later) for {total}; payment complements cover {paid}"
                ),
                Some("Ask the issuer for the Pagos complement (CFDI type P) for each payment and pass it in `payments`.".into()),
            );
        }
    }

    /// R3: the SAT's status and the issuer's art. 69-B standing.
    fn check_sat(&mut self) {
        if !self.policy.requires_cfdi() && self.expense.sat.is_none() {
            return;
        }
        let Some(sat) = self.expense.sat.clone() else {
            self.meta.insert("sat-status", "unchecked".into());
            self.find(
                "sat-unchecked",
                Severity::Review,
                "the CFDI's status hasn't been checked with the SAT yet".into(),
                Some("Check it with the SAT verification service (UUID, issuer and receiver RFC, total) and pass the result as `sat`.".into()),
            );
            return;
        };
        self.meta.insert("sat-status", sat.status.as_str().into());
        self.meta.insert("sat-checked", sat.checked.clone());
        match sat.status {
            SatStatus::Vigente => {}
            SatStatus::Cancelado => self.find(
                "cfdi-cancelled",
                self.hard,
                format!(
                    "the SAT reports the CFDI cancelled (checked {})",
                    sat.checked
                ),
                Some("Ask the issuer for the CFDI that replaced it.".into()),
            ),
            SatStatus::NoEncontrado => self.find(
                "cfdi-not-found",
                self.hard,
                format!("the SAT doesn't know this CFDI (checked {})", sat.checked),
                Some(
                    "Check the UUID, RFCs and total; a CFDI the SAT doesn't know isn't valid."
                        .into(),
                ),
            ),
        }
        match sat.efos {
            Efos::None | Efos::Desvirtuado | Efos::SentenciaFavorable => {}
            Efos::Presunto => self.find(
                "issuer-69b-presumed",
                Severity::Review,
                "the issuer is on the SAT's art. 69-B list as presumed to invoice simulated operations".into(),
                Some("Keep evidence that the purchase was real, and ask a contador.".into()),
            ),
            Efos::Definitivo => self.find(
                "issuer-69b-definitive",
                self.hard,
                "the issuer is on the SAT's art. 69-B list as definitively invoicing simulated operations: its CFDIs don't count unless the organization proves the purchase was real".into(),
                Some("Ask a contador; proving the operation was real goes through the SAT.".into()),
            ),
        }
        if sat.efos != Efos::None {
            self.meta.insert(
                "efos",
                serde_json::to_value(sat.efos)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .into(),
            );
        }
    }

    fn missing_cfdi(&mut self) {
        let profile = self
            .policy
            .profile
            .as_ref()
            .expect("validated: requires_cfdi has a profile");
        let cost = if self.policy.kind.is_nonprofit() {
            "; for a non-profit an expense without its CFDI counts as remanente distribuible, on which the organization pays ISR"
        } else {
            ", and isn't deductible"
        };
        self.find(
            "cfdi-missing",
            Severity::Error,
            format!(
                "no CFDI: the statement line shows the supporter paid, but under the `{}` policy that is payment evidence only{cost}",
                self.policy.kind.as_str()
            ),
            Some(format!(
                "Ask {} to issue a CFDI to RFC {}, {}, postcode {}, regime {}, UsoCFDI {}.",
                self.receipt().merchant,
                profile.rfc,
                profile.name,
                profile.postcode,
                profile.regime,
                profile.uso_cfdi.first().map(String::as_str).unwrap_or("G03")
            )),
        );
    }

    /// R17: a foreign vendor's invoice in place of a CFDI.
    fn check_foreign(&mut self, foreign: &ForeignInvoice) {
        let missing: Vec<&str> = self
            .policy
            .rules
            .foreign_invoice_fields
            .iter()
            .map(String::as_str)
            .filter(|f| foreign.field(f).is_none())
            .collect();
        if !missing.is_empty() {
            self.find(
                "foreign-invoice-incomplete",
                Severity::Error,
                format!("the foreign invoice doesn't show: {}", missing.join(", ")),
                Some("Ask the vendor for an invoice that shows them, naming the organization as buyer.".into()),
            );
        }
        let mut questions = vec![
            if foreign.digital_platform {
                "the vendor is a SAT-registered digital platform, which charges IVA itself"
            } else {
                "IVA on an imported service may have to be self-assessed (LIVA arts. 24 and 26)"
            },
            "the payment may carry ISR withholding (LISR Title V)",
        ];
        if foreign.country.eq_ignore_ascii_case("MX") {
            questions
                .push("the vendor's country is MX: a Mexican vendor should issue a CFDI instead");
        }
        self.find(
            "foreign-invoice-review",
            Severity::Review,
            format!(
                "a foreign vendor's invoice ({}) stands in for a CFDI; for the contador: {}",
                foreign.country,
                questions.join("; ")
            ),
            None,
        );
        self.meta
            .insert("foreign-vendor", foreign.country.to_ascii_uppercase());
    }

    fn check_purpose(&mut self) {
        if !self.policy.kind.is_nonprofit() {
            return;
        }
        if self
            .expense
            .purpose
            .as_deref()
            .is_none_or(|p| p.trim().is_empty())
        {
            self.find(
                "purpose-missing",
                Severity::Review,
                "no purpose recorded: a non-profit's expense outside its purpose counts as remanente distribuible".into(),
                Some("Record what the expense was for, in terms of the organization's purpose.".into()),
            );
        }
        if self.policy.kind == Kind::DonatariaAutorizada && self.expense.class.is_none() {
            self.find(
                "class-missing",
                Severity::Review,
                "no class: a donataria autorizada keeps administration and mission spending apart, for its administration cap".into(),
                Some("Set `class` to `administration` or `mission`.".into()),
            );
        }
    }

    fn check_year(&mut self) {
        let date = self.receipt().date;
        if date.year() != self.policy.fiscal_year {
            self.find(
                "outside-fiscal-year",
                Severity::Review,
                format!(
                    "the receipt is dated {date}, outside fiscal year {}; check it under that year's policy",
                    self.policy.fiscal_year
                ),
                None,
            );
        }
    }
}

fn reissue(p: &Profile) -> String {
    format!(
        "Ask the issuer to cancel it and issue it again to RFC {}, {}, postcode {}, regime {}, UsoCFDI {}.",
        p.rfc,
        p.name,
        p.postcode,
        p.regime,
        p.uso_cfdi.first().map(String::as_str).unwrap_or("G03")
    )
}

/// Case and spacing don't matter when comparing names; everything else does.
fn normalize(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// `money` in `to`, at `rate` units of `to` per unit of its currency (a CFDI's `TipoCambio`).
fn convert(money: Money, to: Currency, rate: Option<&str>) -> Option<Money> {
    if money.currency() == to {
        return Some(money);
    }
    let rate = rate?;
    let (whole, frac) = rate.split_once('.').unwrap_or((rate, ""));
    let num: i128 = format!("{whole}{frac}").parse().ok()?;
    let den: i128 = 10i128.checked_pow(frac.len() as u32)?;
    let scale = |e: u8| 10i128.pow(e as u32);
    let minor = i128::from(money.minor()) * num * scale(to.exponent())
        / (den * scale(money.currency().exponent()));
    Some(Money::new(i64::try_from(minor).ok()?, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_at_the_cfdi_rate() {
        let usd = Money::new(15000, Currency::USD);
        let mxn = Currency::new("MXN").unwrap();
        assert_eq!(
            convert(usd, mxn, Some("17.5")),
            Some(Money::new(262500, mxn))
        );
        assert_eq!(convert(usd, mxn, None), None);
        assert_eq!(convert(usd, Currency::USD, None), Some(usd));
    }

    #[test]
    fn names_compare_without_case_or_spacing() {
        assert_eq!(
            normalize("Organización  de prueba "),
            "ORGANIZACIÓN DE PRUEBA"
        );
    }
}
