# Email intake policy: who may send to the organization's receipts address, and how long mail
# from senders who haven't set up keys waits for them. Used by `intake.*` (crate rdx-intake,
# piece 13) and, exported to JSON, by the Cloudflare Worker (deploy/cloudflare/policy.ncl).
#
# `intake_address` and `claim_url` have no defaults: they name the organization's own domains.
# Everything else does, so an organization writes only what differs:
#
#   let intake = import "rdx/intake.ncl" in
#   {
#     intake_address = "receipts@receipts.example.org",
#     claim_url = "https://app.example.org/claim",
#     allow.domains = ["example.org"],
#   } | intake.Policy
#
# The Rust side (`rdx_intake::Policy`) has no defaults of its own: this file is the single source.

let Address =
  std.contract.from_validator (fun a =>
    if std.is_string a && std.string.is_match "^[^\\s/@<>,;]+@[^\\s/@<>,;]+\\.[^\\s/@<>,;]+$" a then
      'Ok
    else
      'Error {
        message = "expected one email address, like `ana@example.org`",
      }
  )
in

let Domain =
  std.contract.from_validator (fun d =>
    if std.is_string d && std.string.is_match "^[a-z0-9-]+(\\.[a-z0-9-]+)+$" d then
      'Ok
    else
      'Error {
        message = "expected a lowercase domain, like `example.org`",
        notes = ["A domain allows every address at it, not its subdomains: list those separately."],
      }
  )
in

let HttpsUrl =
  std.contract.from_validator (fun u =>
    if std.is_string u && std.string.is_match "^https://[^\\s#]+$" u then
      'Ok
    else if std.is_string u && std.string.is_match "^http://(localhost|127\\.0\\.0\\.1)(:[0-9]+)?(/[^\\s#]*)?$" u then
      'Ok
    else
      'Error {
        message = "expected an https:// URL with no `#` (http:// only for localhost)",
        notes = ["The claim link adds `#address=…&key=…` to it; the key must never travel over plain http."],
      }
  )
in

let Days =
  std.contract.from_validator (fun n =>
    if std.is_number n && std.number.is_integer n && n >= 1 && n <= 90 then
      'Ok
    else
      'Error { message = "expected a whole number of days from 1 to 90" }
  )
in

let Bytes =
  std.contract.from_validator (fun n =>
    if std.is_number n && std.number.is_integer n && n >= 1024 && n <= 25 * 1024 * 1024 then
      'Ok
    else
      'Error {
        message = "expected a size in bytes from 1 KiB to 25 MiB",
        notes = ["Cloudflare Email Routing refuses messages over 25 MiB before the Worker sees them."],
      }
  )
in

let PolicyC = {
  intake_address
    | Address
    | doc "The address supporters send statements and bills to; verification replies come from it. Use a subdomain only intake receives mail for, like receipts@receipts.example.org.",

  claim_url
    | HttpsUrl
    | doc "The PWA page that claims quarantined mail. The reply's link is this URL plus `#address=…&key=…`.",

  allow
    | doc "Senders the organization accepts. Everyone else is bounced. A sender on the list who hasn't set up keys yet is quarantined, not bounced."
    = {
      addresses
        | Array Address
        | doc "Single addresses, such as a volunteer's personal mailbox."
        | default
        = [],
      domains
        | Array Domain
        | doc "Whole domains, such as the organization's own."
        | default
        = [],
    },

  authserv_id
    | String
    | doc "The server whose Authentication-Results field (the topmost one) is trusted for DMARC. Cloudflare Email Routing writes `mx.cloudflare.net`."
    | default
    = "mx.cloudflare.net",

  quarantine_days
    | Days
    | doc "How long mail from a sender without keys waits for them to open their link before it is deleted."
    | default
    = 7,

  reply_interval_days
    | Days
    | doc "The least time between two verification replies to one address, so intake can't be used to flood a mailbox."
    | default
    = 7,

  max_bytes
    | Bytes
    | doc "Largest email accepted. The Worker holds the email and its encrypted copy in memory (128 MB), and the copy is about twice the size."
    | default
    = 10 * 1024 * 1024,
}
in

{
  Policy = PolicyC,
}
