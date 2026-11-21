# The organization's intake policy, checked by rdx/intake.ncl. Edit the three lines below, then
# deploy as wrangler.toml says. See ../../nickel/rdx/intake.ncl for every field and default.
let intake = import "rdx/intake.ncl" in
{
  intake_address = "receipts@receipts.example.org",
  claim_url = "https://app.example.org/claim",
  allow.domains = ["example.org"],
} | intake.Policy
