use std::cmp::Reverse;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use rdx_model::{
    Date, LineId, LineRef, Match, MatchStatus, Money, Receipt, ReceiptId, ReceiptKind,
    ReceiptProblem, Statement, StatementId, StatementLine, StatementProblem,
};

use crate::merchant;
use crate::policy::{Decimal, Policy, PolicyError, bps_of};

/// Candidates kept per line or receipt when searching for splits, so the search stays small.
const MAX_SPLIT_CANDIDATES: usize = 12;

/// What the engine proposes for one statement. Nothing in it is confirmed: a person (or a later
/// step they approve) turns proposals into confirmed matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reconciliation {
    pub statement: StatementId,
    /// In statement line order.
    pub proposals: Vec<Proposal>,
    /// Lines with no proposal and no confirmed match, in statement order. They stay redacted.
    pub unmatched_lines: Vec<LineId>,
    /// The supporter's receipts that explain no line, in input order.
    pub unmatched_receipts: Vec<ReceiptId>,
    /// Why some inputs could not be considered (a missing exchange rate, another supporter's
    /// receipt), so an agent can fix the input or the policy.
    pub notes: Vec<String>,
    pub summary: Summary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub lines: usize,
    /// Lines already covered by confirmed matches passed in, left alone.
    pub decided_lines: usize,
    pub proposed_lines: usize,
    /// Proposals a person must look at before confirming.
    pub to_review: usize,
    pub unmatched_lines: usize,
    pub unmatched_receipts: usize,
}

/// One explanation for one or more lines. `matches` are all `proposed`; confirming the proposal
/// means confirming all of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub kind: ProposalKind,
    /// True when a person must check it: anything but an unambiguous, exact, same-currency match.
    pub review: bool,
    /// Why it needs review, in words a reviewer can act on. Empty when `review` is false.
    pub reasons: Vec<String>,
    pub matches: Vec<Match>,
    /// One entry per line and receipt pair, in the order of `matches`.
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    /// Same amount, within the date window, merchant or reference agrees.
    Exact,
    /// The amounts differ within the policy's amount tolerance.
    Tolerance,
    /// The receipt is in another currency and matches at a policy exchange rate.
    Fx,
    /// One line paid several receipts.
    Split,
    /// One receipt paid over several lines.
    Instalments,
    /// The line pays only part of the receipt.
    Partial,
}

/// Why a line and a receipt were paired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub line: LineId,
    pub receipt: ReceiptId,
    /// Days from the receipt's date to the line's posting date (positive: the line posted later).
    pub days: i64,
    /// 0 to 100; see `rdx_match::similarity`.
    pub merchant_similarity: u8,
    /// The receipt's reference (invoice or bill number) is printed on the line.
    pub reference: bool,
    /// The line's amount (as a positive number) minus what the receipt explains, in the statement
    /// currency. For splits and instalments, of the whole group.
    pub difference: Money,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fx: Option<FxApplied>,
}

/// A conversion recorded with the match ([compliance](../../docs/compliance.md) R16: both
/// amounts, the rate and its date).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FxApplied {
    pub total: Money,
    pub rate: String,
    pub date: Date,
    pub source: String,
    pub converted: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MatchError {
    #[error("statement `{statement}` is not consistent: {}", join(problems))]
    Statement {
        statement: StatementId,
        problems: Vec<StatementProblem>,
    },
    #[error("receipt `{receipt}` is not consistent: {problem}")]
    Receipt {
        receipt: ReceiptId,
        problem: ReceiptProblem,
    },
    #[error("receipt id `{0}` appears more than once")]
    DuplicateReceipt(ReceiptId),
    #[error("the matching policy is invalid: {0}")]
    Policy(#[from] PolicyError),
}

fn join(problems: &[StatementProblem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// What ties one line to one receipt, before amounts are compared.
#[derive(Debug, Clone)]
struct Relation {
    line: usize,
    receipt: usize,
    days: i64,
    similarity: u8,
    reference: bool,
    /// What the receipt explains, positive, in the statement currency.
    explains: Money,
    fx: Option<FxApplied>,
}

impl Relation {
    /// Best first: a printed reference, then a closer date, then a more alike merchant.
    fn strength(&self) -> (Reverse<bool>, i64, Reverse<u8>) {
        (
            Reverse(self.reference),
            self.days.abs(),
            Reverse(self.similarity),
        )
    }

    fn evidence(&self, cx: &Context<'_>, difference: i64) -> Evidence {
        Evidence {
            line: cx.lines[self.line].id.clone(),
            receipt: cx.receipts[self.receipt].id.clone(),
            days: self.days,
            merchant_similarity: self.similarity,
            reference: self.reference,
            difference: Money::new(difference, cx.statement.currency),
            fx: self.fx.clone(),
        }
    }
}

struct Context<'a> {
    statement: &'a Statement,
    lines: &'a [StatementLine],
    receipts: Vec<&'a Receipt>,
    policy: &'a Policy,
    /// Every viable line and receipt pair, whatever the amounts.
    relations: Vec<Relation>,
    line_used: Vec<bool>,
    receipt_used: Vec<bool>,
}

impl Context<'_> {
    fn free(&self, r: &Relation) -> bool {
        !self.line_used[r.line] && !self.receipt_used[r.receipt]
    }

    fn line_abs(&self, line: usize) -> i64 {
        self.lines[line].amount.minor().abs()
    }

    /// Tolerance for an expected amount: the policy's amount tolerance, widened to the fx
    /// tolerance when a conversion is involved.
    fn tolerance(&self, explains: Money, fx: bool) -> i64 {
        let amount = self.policy.amount_tolerance(explains);
        if fx {
            amount.max(bps_of(explains.minor(), self.policy.fx.tolerance_bps))
        } else {
            amount
        }
    }

    fn line_ref(&self, line: usize) -> LineRef {
        LineRef {
            statement: self.statement.id.clone(),
            line: self.lines[line].id.clone(),
        }
    }

    fn proposed(&self, line: usize, receipt: usize, minor: i64) -> Match {
        Match {
            line: self.line_ref(line),
            receipt: self.receipts[receipt].id.clone(),
            amount: Money::new(minor, self.statement.currency),
            status: MatchStatus::Proposed,
        }
    }
}

fn normalize_reference(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Proposes matches between one statement's lines and receipts.
///
/// `receipts` may include other supporters' receipts; only the statement's supporter's are
/// considered. `decided` are earlier decisions: lines and receipts in a confirmed match are left
/// alone, and rejected pairs are never proposed again.
pub fn reconcile(
    statement: &Statement,
    receipts: &[Receipt],
    decided: &[Match],
    policy: &Policy,
) -> Result<Reconciliation, MatchError> {
    policy.validate()?;
    statement
        .validate()
        .map_err(|problems| MatchError::Statement {
            statement: statement.id.clone(),
            problems,
        })?;
    let mut seen = BTreeSet::new();
    for r in receipts {
        if !seen.insert(&r.id) {
            return Err(MatchError::DuplicateReceipt(r.id.clone()));
        }
        r.validate().map_err(|problem| MatchError::Receipt {
            receipt: r.id.clone(),
            problem,
        })?;
    }

    let mut notes = Vec::new();
    let others = receipts
        .iter()
        .filter(|r| r.supporter != statement.supporter)
        .count();
    if others > 0 {
        notes.push(format!(
            "{others} receipt(s) from other supporters were not considered: a receipt only \
             matches its own supporter's statements"
        ));
    }

    let confirmed = |m: &&Match| m.status == MatchStatus::Confirmed;
    let decided_receipts: BTreeSet<&ReceiptId> = decided
        .iter()
        .filter(confirmed)
        .map(|m| &m.receipt)
        .collect();
    let decided_lines: BTreeSet<&LineId> = decided
        .iter()
        .filter(confirmed)
        .filter(|m| m.line.statement == statement.id)
        .map(|m| &m.line.line)
        .collect();
    let rejected: BTreeSet<(&LineId, &ReceiptId)> = decided
        .iter()
        .filter(|m| m.status == MatchStatus::Rejected && m.line.statement == statement.id)
        .map(|m| (&m.line.line, &m.receipt))
        .collect();

    let mine: Vec<&Receipt> = receipts
        .iter()
        .filter(|r| r.supporter == statement.supporter)
        .collect();
    let lines = &statement.lines[..];
    let line_used: Vec<bool> = lines
        .iter()
        .map(|l| decided_lines.contains(&l.id) || l.amount.minor() == 0)
        .collect();
    let receipt_used: Vec<bool> = mine
        .iter()
        .map(|r| decided_receipts.contains(&r.id))
        .collect();

    // A receipt in another currency needs a rate for its pair; without one it is noted and left
    // out.
    let mut convertible = Vec::with_capacity(mine.len());
    for r in &mine {
        let ok = r.total.currency() == statement.currency
            || policy
                .rate(
                    r.total.currency(),
                    statement.currency,
                    statement.period.end(),
                )
                .is_some();
        if !ok {
            notes.push(format!(
                "receipt `{}` is in {} but the policy has no {} to {} rate (fx.rates)",
                r.id,
                r.total.currency(),
                r.total.currency(),
                statement.currency
            ));
        }
        convertible.push(ok);
    }

    let mut relations = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        if line_used[li] {
            continue;
        }
        for (ri, r) in mine.iter().enumerate() {
            if receipt_used[ri] || rejected.contains(&(&line.id, &r.id)) {
                continue;
            }
            if !convertible[ri] {
                continue;
            }
            let Some((explains, fx)) = explain(r, statement.currency, line.date, policy) else {
                continue;
            };
            // A refund comes back as a credit; everything else leaves as a debit.
            let credit = line.amount.minor() > 0;
            if credit != (r.kind == ReceiptKind::Refund) {
                continue;
            }
            if !policy.date.window(r.kind).contains(line.date, r.date) {
                continue;
            }
            let reference = match &r.reference {
                Some(want) => {
                    let want = normalize_reference(want);
                    let printed = line.reference.as_deref().map(normalize_reference);
                    match printed {
                        // Two different references are positive evidence against.
                        Some(p) if !want.is_empty() && p != want => continue,
                        Some(p) => !p.is_empty(),
                        None => {
                            !want.is_empty()
                                && normalize_reference(&line.description).contains(&want)
                        }
                    }
                }
                None => false,
            };
            let similarity = merchant::similarity(&r.merchant, &line.description);
            if !reference && similarity < policy.merchant.min_similarity {
                continue;
            }
            relations.push(Relation {
                line: li,
                receipt: ri,
                days: line.date.days_since(r.date),
                similarity,
                reference,
                explains,
                fx,
            });
        }
    }

    let mut cx = Context {
        statement,
        lines,
        receipts: mine,
        policy,
        relations,
        line_used,
        receipt_used,
    };

    let mut proposals = Vec::new();
    one_to_one(&mut cx, &mut proposals);
    if policy.split.max_parts >= 2 {
        splits(&mut cx, &mut proposals);
        instalments(&mut cx, &mut proposals);
    }
    if policy.partial.enabled {
        partials(&mut cx, &mut proposals);
    }

    // In statement order, by each proposal's first line.
    let position = |p: &Proposal| {
        lines
            .iter()
            .position(|l| l.id == p.matches[0].line.line)
            .unwrap_or(usize::MAX)
    };
    proposals.sort_by_key(|p| (position(p), p.matches[0].receipt.clone()));

    let covered: BTreeSet<&LineId> = proposals
        .iter()
        .flat_map(|p| p.matches.iter().map(|m| &m.line.line))
        .collect();
    let unmatched_lines: Vec<LineId> = lines
        .iter()
        .filter(|l| !covered.contains(&l.id) && !decided_lines.contains(&l.id))
        .map(|l| l.id.clone())
        .collect();
    let unmatched_receipts: Vec<ReceiptId> = cx
        .receipts
        .iter()
        .zip(&cx.receipt_used)
        .filter(|(_, used)| !**used)
        .map(|(r, _)| r.id.clone())
        .collect();

    let summary = Summary {
        lines: lines.len(),
        decided_lines: lines
            .iter()
            .filter(|l| decided_lines.contains(&l.id))
            .count(),
        proposed_lines: covered.len(),
        to_review: proposals.iter().filter(|p| p.review).count(),
        unmatched_lines: unmatched_lines.len(),
        unmatched_receipts: unmatched_receipts.len(),
    };
    Ok(Reconciliation {
        statement: statement.id.clone(),
        proposals,
        unmatched_lines,
        unmatched_receipts,
        notes,
        summary,
    })
}

/// What `r` explains in `currency`, converted at the policy rate dated closest to `on` if it is in
/// another currency. `None` if there is no rate or the result overflows.
fn explain(
    r: &Receipt,
    currency: rdx_model::Currency,
    on: Date,
    policy: &Policy,
) -> Option<(Money, Option<FxApplied>)> {
    if r.total.currency() == currency {
        return Some((r.total, None));
    }
    let rate = policy.rate(r.total.currency(), currency, on)?;
    // validate() already parsed every rate.
    let converted = Decimal::parse(&rate.rate)
        .ok()?
        .convert(r.total, currency)?;
    Some((
        converted,
        Some(FxApplied {
            total: r.total,
            rate: rate.rate.clone(),
            date: rate.date,
            source: rate.source.clone(),
            converted,
        }),
    ))
}

/// One line, one receipt, amounts within tolerance. Strongest pairs are taken first.
fn one_to_one(cx: &mut Context<'_>, out: &mut Vec<Proposal>) {
    let mut fits: Vec<(usize, i64)> = cx
        .relations
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let difference = cx.line_abs(r.line) - r.explains.minor();
            (difference.abs() <= cx.tolerance(r.explains, r.fx.is_some()))
                .then_some((i, difference))
        })
        .collect();
    fits.sort_by_key(|&(i, difference)| {
        let r = &cx.relations[i];
        (
            r.strength().0,
            difference.abs(),
            r.strength().1,
            r.strength().2,
            r.line,
            r.receipt,
        )
    });

    for &(i, difference) in &fits {
        let r = cx.relations[i].clone();
        if !cx.free(&r) {
            continue;
        }
        let rivals = fits.iter().filter(|&&(j, _)| j != i).any(|&(j, _)| {
            let o = &cx.relations[j];
            cx.free(o) && (o.line == r.line || o.receipt == r.receipt)
        });
        cx.line_used[r.line] = true;
        cx.receipt_used[r.receipt] = true;

        let currency = cx.statement.currency;
        let mut reasons = Vec::new();
        let kind = if let Some(fx) = &r.fx {
            reasons.push(format!(
                "converted from {} at {} ({}, {}) to {}",
                fx.total, fx.rate, fx.source, fx.date, fx.converted
            ));
            ProposalKind::Fx
        } else if difference != 0 {
            ProposalKind::Tolerance
        } else {
            ProposalKind::Exact
        };
        if difference != 0 {
            reasons.push(format!(
                "the line is {} {} than the receipt explains",
                Money::new(difference.abs(), currency),
                if difference > 0 { "more" } else { "less" }
            ));
        }
        if rivals {
            reasons
                .push("another unmatched line or receipt fits as well; check the pairing".into());
        }
        out.push(Proposal {
            kind,
            review: !reasons.is_empty(),
            reasons,
            // The receipt explains the whole line: any difference is a fee or tip the reviewer
            // accepts by confirming.
            matches: vec![cx.proposed(r.line, r.receipt, cx.line_abs(r.line))],
            evidence: vec![r.evidence(cx, difference)],
        });
    }
}

/// Subsets of `values` (2 to `max_parts` of them, in index order) whose sum is within `tolerance`
/// of `target`. Returns the first one found and whether there was more than one.
fn find_subset(
    values: &[i64],
    target: i64,
    tolerance: impl Fn(i64) -> i64,
    max_parts: usize,
) -> Option<(Vec<usize>, bool)> {
    struct Search<'a> {
        values: &'a [i64],
        target: i64,
        tolerance: &'a dyn Fn(i64) -> i64,
        max_parts: usize,
        picked: Vec<usize>,
        found: Vec<Vec<usize>>,
    }

    impl Search<'_> {
        fn walk(&mut self, start: usize, sum: i64) {
            // Two solutions are enough to know it is ambiguous.
            if self.found.len() >= 2 {
                return;
            }
            if self.picked.len() >= 2 && (self.target - sum).abs() <= (self.tolerance)(sum) {
                self.found.push(self.picked.clone());
            }
            if self.picked.len() == self.max_parts {
                return;
            }
            for i in start..self.values.len() {
                let next = sum.saturating_add(self.values[i]);
                // Values are positive: a sum already past the target only grows.
                if next - self.target > (self.tolerance)(next) {
                    continue;
                }
                self.picked.push(i);
                self.walk(i + 1, next);
                self.picked.pop();
            }
        }
    }

    let mut search = Search {
        values,
        target,
        tolerance: &tolerance,
        max_parts,
        picked: Vec::new(),
        found: Vec::new(),
    };
    search.walk(0, 0);
    let ambiguous = search.found.len() > 1;
    search.found.into_iter().next().map(|s| (s, ambiguous))
}

/// The free relations for which `pick` holds, strongest first, capped.
fn candidates(cx: &Context<'_>, pick: impl Fn(&Relation) -> bool) -> Vec<Relation> {
    let mut v: Vec<Relation> = cx
        .relations
        .iter()
        .filter(|r| cx.free(r) && pick(r))
        .cloned()
        .collect();
    v.sort_by_key(|r| (r.strength(), r.line, r.receipt));
    v.truncate(MAX_SPLIT_CANDIDATES);
    // Index order for the subset search, so the result doesn't depend on ties in strength.
    v.sort_by_key(|r| (r.line, r.receipt));
    v
}

/// One line that paid several receipts at once.
fn splits(cx: &mut Context<'_>, out: &mut Vec<Proposal>) {
    let max_parts = cx.policy.split.max_parts as usize;
    for line in 0..cx.lines.len() {
        if cx.line_used[line] {
            continue;
        }
        let target = cx.line_abs(line);
        let parts = candidates(cx, |r| r.line == line && r.explains.minor() < target);
        let values: Vec<i64> = parts.iter().map(|r| r.explains.minor()).collect();
        let fx = parts.iter().any(|r| r.fx.is_some());
        let currency = cx.statement.currency;
        let Some((chosen, ambiguous)) = find_subset(
            &values,
            target,
            |sum| cx.tolerance(Money::new(sum, currency), fx),
            max_parts,
        ) else {
            continue;
        };
        let chosen: Vec<&Relation> = chosen.iter().map(|&i| &parts[i]).collect();
        let explained: i64 = chosen.iter().map(|r| r.explains.minor()).sum();
        let difference = target - explained;

        let mut matches = Vec::new();
        let mut evidence = Vec::new();
        for (n, r) in chosen.iter().enumerate() {
            // The last part absorbs any difference, so the matches cover the line exactly.
            let amount = r.explains.minor() + if n + 1 == chosen.len() { difference } else { 0 };
            matches.push(cx.proposed(line, r.receipt, amount));
            evidence.push(r.evidence(cx, difference));
        }
        let mut reasons = vec![format!(
            "one line pays {} receipts; check they were paid together",
            chosen.len()
        )];
        if difference != 0 {
            reasons.push(format!(
                "the receipts add up to {} less than the line",
                Money::new(difference, currency)
            ));
        }
        if ambiguous {
            reasons.push("more than one combination of receipts adds up to this line".into());
        }
        cx.line_used[line] = true;
        for r in &chosen {
            cx.receipt_used[r.receipt] = true;
        }
        out.push(Proposal {
            kind: ProposalKind::Split,
            review: true,
            reasons,
            matches,
            evidence,
        });
    }
}

/// One receipt paid over several lines.
fn instalments(cx: &mut Context<'_>, out: &mut Vec<Proposal>) {
    let max_parts = cx.policy.split.max_parts as usize;
    for receipt in 0..cx.receipts.len() {
        if cx.receipt_used[receipt] {
            continue;
        }
        let parts = candidates(cx, |r| {
            r.receipt == receipt && cx.line_abs(r.line) < r.explains.minor()
        });
        let Some(first) = parts.first() else {
            continue;
        };
        let (explains, fx) = (first.explains, first.fx.is_some());
        let values: Vec<i64> = parts.iter().map(|r| cx.line_abs(r.line)).collect();
        let Some((chosen, ambiguous)) = find_subset(
            &values,
            explains.minor(),
            |_| cx.tolerance(explains, fx),
            max_parts,
        ) else {
            continue;
        };
        let chosen: Vec<&Relation> = chosen.iter().map(|&i| &parts[i]).collect();
        let paid: i64 = chosen.iter().map(|r| cx.line_abs(r.line)).sum();
        let difference = paid - explains.minor();
        let currency = cx.statement.currency;

        let mut reasons = vec![format!(
            "one receipt paid over {} lines; check they are instalments of it",
            chosen.len()
        )];
        if difference != 0 {
            reasons.push(format!(
                "the lines add up to {} {} than the receipt",
                Money::new(difference.abs(), currency),
                if difference > 0 { "more" } else { "less" }
            ));
        }
        if ambiguous {
            reasons.push("more than one combination of lines adds up to this receipt".into());
        }
        let matches = chosen
            .iter()
            .map(|r| cx.proposed(r.line, receipt, cx.line_abs(r.line)))
            .collect();
        let evidence = chosen.iter().map(|r| r.evidence(cx, difference)).collect();
        cx.receipt_used[receipt] = true;
        for r in &chosen {
            cx.line_used[r.line] = true;
        }
        out.push(Proposal {
            kind: ProposalKind::Instalments,
            review: true,
            reasons,
            matches,
            evidence,
        });
    }
}

/// A line that pays part of a receipt, with nothing else left to explain the rest.
fn partials(cx: &mut Context<'_>, out: &mut Vec<Proposal>) {
    for line in 0..cx.lines.len() {
        if cx.line_used[line] {
            continue;
        }
        let paid = cx.line_abs(line);
        let best = cx
            .relations
            .iter()
            .filter(|r| r.line == line && cx.free(r) && paid < r.explains.minor())
            .min_by_key(|r| (r.strength(), r.receipt))
            .cloned();
        let Some(r) = best else { continue };
        let difference = paid - r.explains.minor();
        let currency = cx.statement.currency;
        cx.line_used[line] = true;
        cx.receipt_used[r.receipt] = true;
        out.push(Proposal {
            kind: ProposalKind::Partial,
            review: true,
            reasons: vec![format!(
                "the line pays {} of a {} receipt; check where the other {} was paid",
                Money::new(paid, currency),
                r.explains,
                Money::new(-difference, currency)
            )],
            matches: vec![cx.proposed(line, r.receipt, paid)],
            evidence: vec![r.evidence(cx, difference)],
        });
    }
}
