//! A deterministic generator of synthetic statements, receipts and the matches between them.
//!
//! Public datasets cover statements or receipts, never both for the same payments, so tests for
//! matching, redaction and the ledger use this. Every merchant, supporter and bank is made up.
//! The generator takes a seed instead of a randomness capability, so the same seed gives the same
//! data on every platform.

// Amounts are written as `dollars_cents` (`5_00` is 5.00) so they read as money.
#![allow(clippy::inconsistent_digit_grouping)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    Account, AccountRef, Currency, Date, Digest, LineId, LineRef, Match, MatchStatus, Money,
    Period, Receipt, ReceiptId, ReceiptItem, ReceiptKind, Statement, StatementId, StatementLine,
    Supporter, SupporterId,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub seed: u64,
    pub supporters: usize,
    /// Monthly statements per supporter.
    pub months: u32,
    /// The first statement's month (any day in it).
    pub start: Date,
    pub currency: Currency,
}

impl Config {
    /// The scenario committed under `fixtures/synthetic/small`.
    pub fn small() -> Self {
        Config {
            seed: 1,
            supporters: 2,
            months: 1,
            start: Date::new(2026, 3, 1).unwrap(),
            currency: Currency::USD,
        }
    }
}

/// A situation the matching, redaction or ledger code must handle, and where it occurs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case {
    pub kind: CaseKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<LineRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<ReceiptId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseKind {
    /// The receipt is dated 1 to 3 days before the line posted.
    DateLag,
    /// A bill whose number the bank prints on the line.
    BillReference,
    /// A credit on the statement matched by a refund receipt.
    Refund,
    /// An org merchant's debit with no receipt yet: stays redacted.
    MissingReceipt,
    /// A receipt paid from some other account: matches no line.
    OrphanReceipt,
    /// A personal line: never matched.
    Personal,
}

/// Generated data plus the ground truth: which matches are right and how each is booked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    pub config: Config,
    pub supporters: Vec<Supporter>,
    pub statements: Vec<Statement>,
    pub receipts: Vec<Receipt>,
    /// The correct matches, all confirmed.
    pub matches: Vec<Match>,
    /// The expense account each matched receipt books to.
    pub accounts: BTreeMap<ReceiptId, Account>,
    pub cases: Vec<Case>,
}

/// SplitMix64: tiny, well distributed, and identical everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// Uniform in `lo..=hi`.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.range(0, items.len() as i64 - 1) as usize]
    }
}

struct OrgMerchant {
    name: &'static str,
    descriptor: &'static str,
    account: &'static str,
}

const ORG_MERCHANTS: &[OrgMerchant] = &[
    OrgMerchant {
        name: "Paperleaf Supply",
        descriptor: "PAPERLEAF SUPPLY",
        account: "Expenses:Office:Supplies",
    },
    OrgMerchant {
        name: "Brightline Print Shop",
        descriptor: "BRIGHTLINE PRINT",
        account: "Expenses:Office:Printing",
    },
    OrgMerchant {
        name: "Harbor Hardware",
        descriptor: "HARBOR HARDWARE",
        account: "Expenses:Facilities:Maintenance",
    },
    OrgMerchant {
        name: "Metro Rideshare",
        descriptor: "METRO RIDESHARE TRIP",
        account: "Expenses:Travel:Local",
    },
    OrgMerchant {
        name: "Lantern Cafe Catering",
        descriptor: "LANTERN CAFE CATERING",
        account: "Expenses:Events:Catering",
    },
];

const BILLERS: &[OrgMerchant] = &[
    OrgMerchant {
        name: "Northwind Hosting",
        descriptor: "NORTHWIND HOSTING",
        account: "Expenses:Software:Hosting",
    },
    OrgMerchant {
        name: "Clearwave Telecom",
        descriptor: "CLEARWAVE TELECOM",
        account: "Expenses:Utilities:Phone",
    },
];

const PERSONAL: &[&str] = &[
    "CORNER GROCERY",
    "CITY TRANSIT FARE",
    "STREAMVIEW SUBSCRIPTION",
    "MAPLE PHARMACY",
    "BEANERY COFFEE",
    "GREENLEAF MARKET",
    "CINEMA PLAZA",
    "FUEL STOP",
];

/// A line before it has an id: lines are sorted by date, then numbered.
struct Draft {
    date: Date,
    description: String,
    amount: i64,
    reference: Option<String>,
    /// Receipt (index into the scenario's receipts) this line pays, if any.
    receipt: Option<usize>,
    case: Option<CaseKind>,
}

impl Scenario {
    pub fn generate(config: &Config) -> Scenario {
        let mut rng = Rng(config.seed);
        let cur = config.currency;
        let money = |minor: i64| Money::new(minor, cur);
        let mut s = Scenario {
            config: config.clone(),
            supporters: Vec::new(),
            statements: Vec::new(),
            receipts: Vec::new(),
            matches: Vec::new(),
            accounts: BTreeMap::new(),
            cases: Vec::new(),
        };

        for n in 1..=config.supporters {
            let supporter = SupporterId::new(format!("s-{n:04}")).unwrap();
            s.supporters.push(Supporter {
                id: supporter.clone(),
                display_name: format!("Test Supporter {n}"),
                full_name: Some(format!("Test Supporter Number {n}")),
                alias: Some(format!("Tester {n}")),
            });
            let mut balance = rng.range(500_00, 3_000_00);
            let mut month_start = Date::new(config.start.year(), config.start.month(), 1).unwrap();

            for _ in 0..config.months {
                let end = month_start.end_of_month();
                let days = end.day() as i64;
                let day = |rng: &mut Rng| month_start.add_days(rng.range(0, days - 1));
                let tag = format!("{:04}-{:02}", month_start.year(), month_start.month());
                let statement_id = StatementId::new(format!("st-{tag}.{supporter}")).unwrap();
                let receipt_id = |k: usize| ReceiptId::new(format!("r-{tag}.{supporter}.{k:02}"));
                let mut drafts = Vec::new();
                let new_receipt =
                    |s: &mut Scenario, kind, m: &OrgMerchant, date, total: i64, reference| {
                        let id = receipt_id(s.receipts.len() + 1).unwrap();
                        let items = if kind == ReceiptKind::Receipt {
                            // Roughly 8% sales tax, split out the way printed receipts do.
                            let tax = total * 8 / 108;
                            vec![
                                ReceiptItem {
                                    description: "Items".into(),
                                    amount: money(total - tax),
                                },
                                ReceiptItem {
                                    description: "Sales tax".into(),
                                    amount: money(tax),
                                },
                            ]
                        } else {
                            vec![]
                        };
                        s.receipts.push(Receipt {
                            source: Digest::of_bytes(format!("synthetic document {id}").as_bytes()),
                            id,
                            supporter: supporter.clone(),
                            kind,
                            merchant: m.name.into(),
                            date,
                            total: money(total),
                            reference,
                            items,
                        });
                        s.receipts.len() - 1
                    };

                // Org purchases with receipts, some dated before the line posts.
                let mut purchases = Vec::new();
                for _ in 0..rng.range(2, 4) {
                    let m = rng.pick(ORG_MERCHANTS);
                    let posted = day(&mut rng);
                    let lag = rng.range(0, 3);
                    let total = rng.range(5_00, 300_00);
                    let r = new_receipt(
                        &mut s,
                        ReceiptKind::Receipt,
                        m,
                        posted.add_days(-lag),
                        total,
                        None,
                    );
                    s.accounts
                        .insert(s.receipts[r].id.clone(), Account::new(m.account).unwrap());
                    purchases.push((posted, total, r, m));
                    drafts.push(Draft {
                        date: posted,
                        description: format!("{} #{:04}", m.descriptor, rng.range(1, 9999)),
                        amount: -total,
                        reference: None,
                        receipt: Some(r),
                        case: (lag > 0).then_some(CaseKind::DateLag),
                    });
                }

                // A bill paid by card, its number printed on the line.
                let biller = rng.pick(BILLERS);
                let posted = day(&mut rng);
                let total = rng.range(20_00, 150_00);
                let invoice = format!("INV-{:06}", rng.range(1, 999_999));
                let r = new_receipt(
                    &mut s,
                    ReceiptKind::Bill,
                    biller,
                    posted.add_days(-rng.range(5, 15)),
                    total,
                    Some(invoice.clone()),
                );
                s.accounts.insert(
                    s.receipts[r].id.clone(),
                    Account::new(biller.account).unwrap(),
                );
                drafts.push(Draft {
                    date: posted,
                    description: format!("{} {invoice}", biller.descriptor),
                    amount: -total,
                    reference: Some(invoice),
                    receipt: Some(r),
                    case: Some(CaseKind::BillReference),
                });

                // A partial refund of the first purchase, posted on or after it.
                let (bought, paid, _, m) = purchases[0];
                let refunded = rng.range(1_00, paid.max(1_00));
                let posted = bought.add_days(rng.range(1, 5)).min(end);
                let r = new_receipt(&mut s, ReceiptKind::Refund, m, posted, refunded, None);
                s.accounts
                    .insert(s.receipts[r].id.clone(), Account::new(m.account).unwrap());
                drafts.push(Draft {
                    date: posted,
                    description: format!("{} REFUND", m.descriptor),
                    amount: refunded,
                    reference: None,
                    receipt: Some(r),
                    case: Some(CaseKind::Refund),
                });

                // An org merchant's debit whose receipt never arrived.
                let m = rng.pick(ORG_MERCHANTS);
                drafts.push(Draft {
                    date: day(&mut rng),
                    description: format!("{} #{:04}", m.descriptor, rng.range(1, 9999)),
                    amount: -rng.range(5_00, 80_00),
                    reference: None,
                    receipt: None,
                    case: Some(CaseKind::MissingReceipt),
                });

                // A receipt paid from some other account.
                let m = rng.pick(ORG_MERCHANTS);
                let r = new_receipt(
                    &mut s,
                    ReceiptKind::Receipt,
                    m,
                    day(&mut rng),
                    rng.range(5_00, 60_00),
                    None,
                );
                s.cases.push(Case {
                    kind: CaseKind::OrphanReceipt,
                    line: None,
                    receipt: Some(s.receipts[r].id.clone()),
                });

                // Personal spending, and a personal deposit large enough to cover the month.
                for _ in 0..rng.range(3, 6) {
                    drafts.push(Draft {
                        date: day(&mut rng),
                        description: format!("{} #{:04}", rng.pick(PERSONAL), rng.range(1, 9999)),
                        amount: -rng.range(2_00, 120_00),
                        reference: None,
                        receipt: None,
                        case: Some(CaseKind::Personal),
                    });
                }
                drafts.push(Draft {
                    date: day(&mut rng),
                    description: "TRANSFER IN".into(),
                    amount: rng.range(500_00, 1_500_00),
                    reference: None,
                    receipt: None,
                    case: Some(CaseKind::Personal),
                });

                // Stable order: by date, then by generation order.
                drafts.sort_by_key(|d| d.date);
                let opening = balance;
                let mut lines = Vec::new();
                for (i, d) in drafts.into_iter().enumerate() {
                    let id = LineId::new(format!("l{:02}", i + 1)).unwrap();
                    let at = LineRef {
                        statement: statement_id.clone(),
                        line: id.clone(),
                    };
                    if let Some(r) = d.receipt {
                        s.matches.push(Match {
                            line: at.clone(),
                            receipt: s.receipts[r].id.clone(),
                            amount: money(d.amount.abs()),
                            status: MatchStatus::Confirmed,
                        });
                    }
                    if let Some(kind) = d.case {
                        s.cases.push(Case {
                            kind,
                            line: Some(at),
                            receipt: d.receipt.map(|r| s.receipts[r].id.clone()),
                        });
                    }
                    balance += d.amount;
                    lines.push(StatementLine {
                        id,
                        date: d.date,
                        description: d.description,
                        amount: money(d.amount),
                        reference: d.reference,
                    });
                }
                s.statements.push(Statement {
                    original: Some(Digest::of_bytes(
                        format!("synthetic encrypted original {statement_id}").as_bytes(),
                    )),
                    id: statement_id,
                    supporter: supporter.clone(),
                    account: AccountRef {
                        institution: "Example Bank".into(),
                        mask: format!("{:04}", n),
                    },
                    currency: cur,
                    period: Period::new(month_start, end).unwrap(),
                    opening: money(opening),
                    closing: money(balance),
                    lines,
                });
                month_start = end.add_days(1);
            }
        }
        s
    }
}

/// A plain CSV rendering of a statement (`date,description,amount,balance`), the simplest input
/// for statement parsers (roadmap piece 4).
pub fn statement_csv(statement: &Statement) -> String {
    let mut out = String::from("date,description,amount,balance\n");
    let mut balance = statement.opening;
    for line in &statement.lines {
        // Same currency: the statement validated when it was generated.
        balance = balance.checked_add(line.amount).unwrap();
        out.push_str(&format!(
            "{},{},{},{}\n",
            line.date,
            line.description,
            line.amount.decimal(),
            balance.decimal()
        ));
    }
    out
}
