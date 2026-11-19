use serde::{Deserialize, Serialize};

use crate::{Date, Digest, Money, ReceiptId, SupporterId};

/// Evidence that a payment was for the organization: a receipt, an emailed bill, or a refund note.
///
/// The field set is the common ground of receipt and invoice extraction datasets (merchant, date,
/// total, reference, line items), so extracted documents map onto it directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub id: ReceiptId,
    /// Who submitted it, and so whose statement it should match.
    pub supporter: SupporterId,
    pub kind: ReceiptKind,
    pub merchant: String,
    /// The date printed on the document. Card payments often post a few days later.
    pub date: Date,
    /// Always positive. A `refund` matches a credit on the statement, the others a debit.
    pub total: Money,
    /// Invoice, bill or order number, which banks sometimes print on the statement line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ReceiptItem>,
    /// Digest of the original document (PDF, image or email) as received.
    pub source: Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptKind {
    Receipt,
    Bill,
    Refund,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptItem {
    pub description: String,
    pub amount: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReceiptProblem {
    #[error("the total must be positive (a refund is a `refund` receipt with a positive total)")]
    NotPositive,
    #[error("line items add up to {items}, but the total is {total}")]
    ItemsDoNotAddUp { items: Money, total: Money },
    #[error("a line item is not in the receipt's currency")]
    ItemCurrency,
}

impl Receipt {
    pub fn validate(&self) -> Result<(), ReceiptProblem> {
        if self.total.minor() <= 0 {
            return Err(ReceiptProblem::NotPositive);
        }
        if self.items.is_empty() {
            return Ok(());
        }
        let items = Money::sum(self.total.currency(), self.items.iter().map(|i| &i.amount))
            .map_err(|_| ReceiptProblem::ItemCurrency)?;
        if items != self.total {
            return Err(ReceiptProblem::ItemsDoNotAddUp {
                items,
                total: self.total,
            });
        }
        Ok(())
    }

    /// The statement amount this receipt explains: negative (a debit) unless it is a refund.
    pub fn expected_line_amount(&self) -> Money {
        match self.kind {
            ReceiptKind::Refund => self.total,
            // Totals are positive and well below i64::MAX, so negation cannot overflow.
            _ => Money::new(-self.total.minor(), self.total.currency()),
        }
    }
}
