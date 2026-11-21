//! The Nickel API and `rdx`'s registry offer the same ops: every op `rdx` registers has a
//! constructor at `rdx.ops.<domain>.<name>` in nickel/rdx/api.ncl, and every constructor names an
//! op `rdx` can run. Domain crates add both independently (Cargo.toml dependency, a line in
//! nickel/rdx/ops.ncl), so this catches one without the other.

use std::path::PathBuf;

use nickel_embed::{ExportFormat, Program, Source};

#[test]
fn every_registered_op_has_a_constructor_and_back() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let code = r#"
        let ops = (import "rdx/api.ncl").ops in
        std.record.fields ops
        |> std.array.flat_map (fun d => std.record.fields ops."%{d}" |> std.array.map (fun o => "%{d}.%{o}"))
    "#;
    let json = Program::builder()
        .source(Source::code(code))
        .import_paths(vec![root.join("nickel")])
        .build()
        .and_then(|mut p| p.export(ExportFormat::Json))
        .unwrap_or_else(|e| panic!("evaluating nickel/rdx/api.ncl: {e}"));
    let mut constructors: Vec<String> = serde_json::from_str(&json).unwrap();
    constructors.sort();

    let registered: Vec<String> = rdx_cli::registry()
        .ids()
        .into_iter()
        .map(String::from)
        .collect();

    let missing_ctor: Vec<_> = registered
        .iter()
        .filter(|id| !constructors.contains(id))
        .collect();
    let missing_op: Vec<_> = constructors
        .iter()
        .filter(|id| !registered.contains(id))
        .collect();
    assert!(
        missing_ctor.is_empty() && missing_op.is_empty(),
        "ops without a constructor in nickel/rdx (add `ops/<domain>.ncl` and a line in ops.ncl): \
         {missing_ctor:?}\nconstructors without a registered op (add the crate to \
         crates/rdx-cli/Cargo.toml with `[package.metadata.rdx] ops = true`): {missing_op:?}"
    );
}
