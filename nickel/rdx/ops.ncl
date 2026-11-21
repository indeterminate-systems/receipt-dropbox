# Each domain crate's op constructors, merged into `ops` by api.ncl: `rdx.ops.<domain>.<op>`.
#
# One line per domain, so pieces that add domains in parallel don't conflict: .gitattributes
# merges this file with `union`, keeping both sides' lines. rdx-cli's tests/api.rs checks that
# these constructors and rdx's registered ops are the same set.
{
  envelope = import "ops/envelope.ncl",
  keys = import "ops/keys.ncl",
  matching = import "ops/matching.ncl",
  pdf = import "ops/pdf.ncl",
  receipt = import "ops/receipt.ncl",
  statement = import "ops/statement.ncl",
}
