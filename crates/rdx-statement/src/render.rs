//! Statements rendered as documents, so synthetic fixtures exist in every format the parsers
//! read: OFX, and PDF in the example-bank layout. Deterministic: same statement, same bytes.

use rdx_model::{Date, Money, Statement};
use rdx_pdf::write::{Pdf, Style, text_width};

use crate::example_bank::INSTITUTION;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn long_date(d: Date) -> String {
    format!(
        "{} {}, {}",
        MONTHS[d.month() as usize - 1],
        d.day(),
        d.year()
    )
}

/// `1234.5` → `1,234.50`, without sign.
fn grouped(m: Money) -> String {
    let dec = m.abs().decimal();
    let (whole, frac) = dec
        .split_once('.')
        .map_or((dec.as_str(), None), |(w, f)| (w, Some(f)));
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

fn signed(m: Money) -> String {
    let sign = if m.is_negative() { "-" } else { "" };
    format!("{sign}${}", grouped(m))
}

const LEFT: f64 = 54.0;
const DESCRIPTION: f64 = 100.0;
const WITHDRAWALS: f64 = 420.0;
const DEPOSITS: f64 = 490.0;
const BALANCE: f64 = 558.0;
const ROW: f64 = 14.0;

/// The statement as an Example Bank PDF, with at most `rows_per_page` table rows a page
/// (continuation rows count).
pub fn example_bank_pdf(statement: &Statement, rows_per_page: usize) -> Vec<u8> {
    let rows_per_page = rows_per_page.max(2);
    // Table rows: each line, plus a `Ref` row under lines with a reference.
    enum Row<'a> {
        Line(&'a rdx_model::StatementLine, Money),
        Ref(&'a str),
    }
    let mut rows = Vec::new();
    let mut balance = statement.opening;
    for line in &statement.lines {
        balance = balance.checked_add(line.amount).expect("a valid statement");
        rows.push(Row::Line(line, balance));
        if let Some(r) = &line.reference {
            rows.push(Row::Ref(r));
        }
    }
    let chunks: Vec<&[Row]> = if rows.is_empty() {
        vec![&[]]
    } else {
        rows.chunks(rows_per_page).collect()
    };
    let n_pages = chunks.len();

    let debits = Money::new(
        statement
            .lines
            .iter()
            .map(|l| l.amount.minor())
            .filter(|m| *m < 0)
            .sum(),
        statement.currency,
    );
    let credits = Money::new(
        statement
            .lines
            .iter()
            .map(|l| l.amount.minor())
            .filter(|m| *m > 0)
            .sum(),
        statement.currency,
    );

    let mut pdf = Pdf::new();
    for (p, chunk) in chunks.iter().enumerate() {
        let page = pdf.page();
        page.text(Style::Bold, 16.0, LEFT, 740.0, INSTITUTION);
        page.text_right(Style::Regular, 9.0, BALANCE, 742.0, "Statement period");
        page.text_right(
            Style::Regular,
            9.0,
            BALANCE,
            730.0,
            &format!(
                "{} to {}",
                long_date(statement.period.start()),
                long_date(statement.period.end())
            ),
        );
        page.text(
            Style::Regular,
            9.0,
            LEFT,
            716.0,
            &format!("Account ending in {}", statement.account.mask),
        );
        page.text(
            Style::Regular,
            9.0,
            LEFT,
            704.0,
            &format!("All amounts in {}", statement.currency),
        );

        let mut y = 680.0;
        if p == 0 {
            page.text(Style::Bold, 11.0, LEFT, y, "Account summary");
            y -= 16.0;
            for (label, amount) in [
                ("Beginning balance", signed(statement.opening)),
                ("Withdrawals", signed(debits)),
                ("Deposits", signed(credits)),
                ("Ending balance", signed(statement.closing)),
            ] {
                page.text(Style::Regular, 9.0, LEFT, y, label);
                page.text_right(Style::Regular, 9.0, 260.0, y, &amount);
                y -= ROW;
            }
            y -= 20.0;
        }

        page.text(Style::Bold, 9.0, LEFT, y, "Date");
        page.text(Style::Bold, 9.0, DESCRIPTION, y, "Description");
        page.text_right(Style::Bold, 9.0, WITHDRAWALS, y, "Withdrawals");
        page.text_right(Style::Bold, 9.0, DEPOSITS, y, "Deposits");
        page.text_right(Style::Bold, 9.0, BALANCE, y, "Balance");
        page.rule(LEFT, BALANCE, y - 4.0);
        y -= ROW + 2.0;

        for row in chunk.iter() {
            match row {
                Row::Line(line, balance) => {
                    let d = line.date;
                    page.text(
                        Style::Regular,
                        9.0,
                        LEFT,
                        y,
                        &format!("{:02}/{:02}", d.month(), d.day()),
                    );
                    page.text(Style::Regular, 9.0, DESCRIPTION, y, &line.description);
                    let column = if line.amount.is_negative() {
                        WITHDRAWALS
                    } else {
                        DEPOSITS
                    };
                    page.text_right(Style::Regular, 9.0, column, y, &grouped(line.amount));
                    let b = if balance.is_negative() {
                        format!("-{}", grouped(*balance))
                    } else {
                        grouped(*balance)
                    };
                    page.text_right(Style::Regular, 9.0, BALANCE, y, &b);
                }
                Row::Ref(r) => {
                    page.text(Style::Regular, 8.0, DESCRIPTION, y, &format!("Ref {r}"));
                }
            }
            y -= ROW;
        }
        let footer = format!("Page {} of {n_pages}", p + 1);
        page.text(
            Style::Regular,
            8.0,
            BALANCE - text_width(&footer, 8.0),
            40.0,
            &footer,
        );
    }
    pdf.finish()
}

fn ofx_date(d: Date) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The statement as an OFX 1.02 (SGML) bank statement download.
pub fn ofx(statement: &Statement) -> String {
    let end = statement.period.end();
    let mut o = String::from(
        "OFXHEADER:100\nDATA:OFXSGML\nVERSION:102\nSECURITY:NONE\nENCODING:USASCII\n\
         CHARSET:1252\nCOMPRESSION:NONE\nOLDFILEUID:NONE\nNEWFILEUID:NONE\n\n<OFX>\n",
    );
    o.push_str(&format!(
        "<SIGNONMSGSRSV1><SONRS><STATUS><CODE>0<SEVERITY>INFO</STATUS>\
         <DTSERVER>{}<LANGUAGE>ENG<FI><ORG>{}<FID>1001</FI></SONRS></SIGNONMSGSRSV1>\n",
        ofx_date(end.add_days(1)),
        escape(&statement.account.institution)
    ));
    o.push_str(&format!(
        "<BANKMSGSRSV1><STMTTRNRS><TRNUID>1<STATUS><CODE>0<SEVERITY>INFO</STATUS>\n\
         <STMTRS><CURDEF>{}\n\
         <BANKACCTFROM><BANKID>000000000<ACCTID>9999{}<ACCTTYPE>CHECKING</BANKACCTFROM>\n\
         <BANKTRANLIST><DTSTART>{}<DTEND>{}\n",
        statement.currency,
        statement.account.mask,
        ofx_date(statement.period.start()),
        ofx_date(end)
    ));
    for line in &statement.lines {
        let kind = if line.amount.is_negative() {
            "DEBIT"
        } else {
            "CREDIT"
        };
        o.push_str(&format!(
            "<STMTTRN><TRNTYPE>{kind}<DTPOSTED>{}<TRNAMT>{}<FITID>{}.{}",
            ofx_date(line.date),
            line.amount.decimal(),
            statement.id,
            line.id
        ));
        if let Some(r) = &line.reference {
            o.push_str(&format!("<REFNUM>{}", escape(r)));
        }
        o.push_str(&format!("<NAME>{}</STMTTRN>\n", escape(&line.description)));
    }
    o.push_str(&format!(
        "</BANKTRANLIST>\n<LEDGERBAL><BALAMT>{}<DTASOF>{}</LEDGERBAL>\n\
         </STMTRS></STMTTRNRS></BANKMSGSRSV1>\n</OFX>\n",
        statement.closing.decimal(),
        ofx_date(end)
    ));
    o
}
