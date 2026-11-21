// The viewable statement. Its only input is `sys.inputs.data`: JSON that rdx-render built from
// an envelope whose signature it had just verified. Every figure arrives as finished text, so this
// template does no arithmetic and no crypto: it lays out what Rust already checked. Data is read as
// JSON, never spliced into this source, so a description can't inject markup.
#let d = json(bytes(sys.inputs.data))

#set document(title: "Viewable statement " + d.statement, date: none)
#set page(
  paper: "a4",
  margin: (x: 2cm, top: 2cm, bottom: 2.2cm),
  footer: context [
    #set text(8pt, fill: luma(90))
    #d.statement #h(1fr) Root #raw(d.root) #h(1fr)
    Page #counter(page).display() of #counter(page).final().first()
  ],
)
#set text(font: "Libertinus Serif", size: 10pt, lang: "en")
#show raw: set text(font: "DejaVu Sans Mono", size: 7.5pt)

#let money(s) = align(right, s)
#let muted = luma(90)

= Viewable statement

#grid(
  columns: (auto, 1fr),
  column-gutter: 1em,
  row-gutter: 0.5em,
  [Statement], [#d.statement],
  [Supporter], [#d.supporter],
  [Account], [#d.institution, ending #d.mask],
  [Period], [#d.period_start to #d.period_end],
  [Currency], [#d.currency],
  ..if d.original != none { ([Original], raw(d.original)) },
)

#v(1em)

#table(
  columns: (auto, 1fr, auto),
  stroke: (x, y) => if y == 0 { (bottom: 0.6pt) } else { (bottom: 0.2pt + luma(200)) },
  inset: (x: 4pt, y: 5pt),
  table.header([*Date*], [*Description*], align(right)[*Amount*]),
  ..d.lines
    .map(l => (
      l.date,
      if l.reference != none [#l.description #text(fill: muted)[(#l.reference)]] else [#l.description],
      money(l.amount),
    ))
    .flatten(),
  ..range(d.redacted_rows)
    .map(_ => ([—], text(fill: muted)[Personal, redacted], money(text(fill: muted)[—])))
    .flatten(),
)

#text(8pt, fill: muted)[
  Shown lines have receipts or bills covering their whole amount. Every other line of the
  statement is personal: its date, description and amount are removed from the signed envelope.
  Redacted rows come after the shown lines because their order is hidden too.
]

#v(1em)

#table(
  columns: (1fr, auto),
  stroke: none,
  inset: (x: 4pt, y: 4pt),
  [Opening balance], money(d.opening),
  [Shown lines], money(d.shown),
  [Total redacted (#d.redacted_rows #if d.redacted_rows == 1 [row] else [rows])], money(d.redacted),
  table.hline(stroke: 0.6pt),
  [*Closing balance*], money[*#d.closing*],
)

#v(1.5em)

== Verify this statement

#grid(
  columns: if d.qr != none { (1fr, 5.5cm) } else { (1fr,) },
  column-gutter: 1.5em,
  [
    The supporter signed this statement as a Gordian Envelope. Removing personal lines
    doesn't change its root digest:

    #raw(d.root)

    To check it, read the envelope from the QR code, from the file attached to this PDF
    (`envelope.ur`), or from the last page, and run the `verify-statement` skill with the
    supporter's public key from the organization's key registry. It confirms the signature and
    every figure above. #if d.qr == none [The envelope is too large for one QR code; use the
    attachment or the last page.]
  ],
  ..if d.qr != none {
    (image(bytes(d.qr), format: "svg", width: 5.5cm, alt: "QR code of the statement envelope"),)
  },
)

#pdf.attach(
  "envelope.ur",
  bytes(d.envelope),
  mime-type: "text/plain",
  description: "The signed, redacted statement envelope (ur:envelope)",
)

#pagebreak()

== Envelope

#text(8pt, fill: muted)[
  The signed envelope as a UR, in lines of #d.envelope_width characters. Join the lines without
  spaces to get the `ur:envelope/…` string.
]

#for chunk in d.envelope_lines [
  #raw(chunk) \
]
