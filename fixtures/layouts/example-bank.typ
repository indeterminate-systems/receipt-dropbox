// The example-bank statement layout in Typst: the same layout `rdx_statement::render` writes,
// from a different PDF producer (embedded subset fonts, ToUnicode maps, object streams), so the
// PDF reader is tested on more than its own writer's output.
//
// Regenerate the committed PDF from the repository root in the dev shell:
//   typst compile --root . --input statement=fixtures/layouts/st-2026-03.s-0001.json \
//     --creation-timestamp 1767225600 fixtures/layouts/example-bank.typ \
//     fixtures/synthetic/small/statements/st-2026-03.s-0001.typst.pdf

#let st = json("/" + sys.inputs.at("statement"))

#let group(n) = {
  let s = str(n)
  let out = ""
  for (i, c) in s.clusters().enumerate() {
    if i > 0 and calc.rem(s.len() - i, 3) == 0 { out += "," }
    out += c
  }
  out
}
#let money(minor) = {
  let a = calc.abs(minor)
  let cents = calc.rem(a, 100)
  group(calc.quo(a, 100)) + "." + (if cents < 10 { "0" } else { "" }) + str(cents)
}
#let dollars(minor) = (if minor < 0 { "-" } else { "" }) + "$" + money(minor)
#let months = ("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")
#let parts(d) = d.split("-").map(int)
#let long-date(d) = {
  let (y, m, day) = parts(d)
  months.at(m - 1) + " " + str(day) + ", " + str(y)
}
#let short-date(d) = {
  let (y, m, day) = parts(d)
  (if m < 10 { "0" } else { "" }) + str(m) + "/" + (if day < 10 { "0" } else { "" }) + str(day)
}

#set page(
  paper: "us-letter",
  margin: (x: 54pt, top: 48pt, bottom: 60pt),
  header: grid(
    columns: (1fr, auto),
    text(16pt, weight: "bold")[Example Bank],
    align(right)[
      #set text(9pt)
      Statement period \
      #long-date(st.period.start) to #long-date(st.period.end)
    ],
  ),
  header-ascent: 20%,
  footer: context align(right, text(8pt)[Page #counter(page).display("1 of 1", both: true)]),
)
#set text(9pt)

#v(12pt)
Account ending in #st.account.mask \
All amounts in #st.currency

#let debits = st.lines.map(l => l.amount.minor).filter(m => m < 0).sum(default: 0)
#let credits = st.lines.map(l => l.amount.minor).filter(m => m > 0).sum(default: 0)

#text(11pt, weight: "bold")[Account summary]
#grid(
  columns: (130pt, 80pt),
  row-gutter: 6pt,
  [Beginning balance], align(right, dollars(st.opening.minor)),
  [Withdrawals], align(right, dollars(debits)),
  [Deposits], align(right, dollars(credits)),
  [Ending balance], align(right, dollars(st.closing.minor)),
)
#v(12pt)

#let rows = {
  let balance = st.opening.minor
  let out = ()
  for l in st.lines {
    balance += l.amount.minor
    let a = l.amount.minor
    out += (
      short-date(l.date),
      l.description,
      align(right, if a < 0 { money(a) } else { "" }),
      align(right, if a > 0 { money(a) } else { "" }),
      align(right, (if balance < 0 { "-" } else { "" }) + money(balance)),
    )
    if l.at("reference", default: none) != none {
      out += ([], text(8pt)[Ref #l.reference], [], [], [])
    }
  }
  out
}

#table(
  columns: (auto, 1fr, auto, auto, auto),
  stroke: none,
  column-gutter: 14pt,
  inset: (x: 0pt, y: 3pt),
  table.header(
    [*Date*], [*Description*], align(right)[*Withdrawals*], align(right)[*Deposits*], align(right)[*Balance*],
  ),
  table.hline(stroke: 0.5pt),
  ..rows,
)
