//! `fiscal.check` in a Nickel plan: the policy contract's defaults and errors, and the op's output.

use std::path::{Path, PathBuf};

use rdx_plan::{LoadError, PlanSource, Registry, StepStatus, execute};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(policy: &str) -> Result<rdx_plan::Plan, LoadError> {
    let cfdi = repo().join("fixtures/receipts/cfdi-ingreso.xml");
    let code = format!(
        r#"let rdx = import "rdx/api.ncl" in
let fiscal = import "rdx/fiscal.ncl" in
{{
  plan = rdx.plan {{
    skill = "test",
    goal = "Check fiscal proof",
    steps = [
      rdx.ops.fiscal.check {{
        id = "proof",
        policy = {policy},
        expenses = [
          {{
            receipt = {{
              id = "r-1",
              supporter = "s-0001",
              kind = "bill",
              merchant = "PAPELERÍA EJEMPLO",
              date = "2026-03-12",
              total = {{ minor = 116000, currency = "MXN" }},
              source = "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            }},
            cfdi = import "{}" as 'Text,
            sat = {{ status = "vigente", checked = "2026-03-20T10:00:00Z" }},
          }},
        ],
      }},
    ],
  }},
}}"#,
        cfdi.display()
    );
    PlanSource::code(code)
        .import_path(repo().join("nickel"))
        .load()
}

const PROFILE: &str = r#"profile = { rfc = "XIA190128J61", name = "ORGANIZACIÓN DE PRUEBA", postcode = "06000", regime = "603" }"#;

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_fiscal::register(&mut r);
    r
}

#[test]
fn the_years_rules_are_the_default() {
    let plan = load(&format!(
        "{{ kind = \"company\", fiscal_year = 2026, {PROFILE} }}"
    ))
    .unwrap();
    let policy = &plan.steps[0].args["policy"];
    assert_eq!(policy["rules"]["cash"]["threshold"]["minor"], 200000);
    assert_eq!(policy["rules"]["confirmed"], false);
    assert_eq!(policy["profile"]["uso_cfdi"][0], "G03");
    assert_eq!(policy["sat_registered"], true);

    let report = execute(&plan, &registry(), &mut rdx_plan::rdx_host::Host::new()).unwrap();
    assert!(report.ok, "{report:?}");
    let step = &report.steps[0];
    assert_eq!(
        step.status,
        StepStatus::Done,
        "no effects, so it runs in a dry run"
    );
    let out = step.output.as_ref().unwrap();
    assert_eq!(out["assessments"][0]["proof"], "supported", "{out}");
    assert_eq!(out["summary"]["supported"], 1);
}

#[test]
fn one_rule_can_change() {
    let plan = load(&format!(
        "{{ kind = \"company\", fiscal_year = 2026, {PROFILE}, rules = fiscal.by_year.\"2026\" & {{ cash.threshold.minor = 100000 }} }}"
    ))
    .unwrap();
    let rules = &plan.steps[0].args["policy"]["rules"];
    assert_eq!(rules["cash"]["threshold"]["minor"], 100000);
    assert_eq!(rules["cash"]["forms"][0], "01");
    assert_eq!(rules["fuel_product_codes"][0], "151015");
}

#[test]
fn a_year_without_rules_is_an_error() {
    let e = load(&format!(
        "{{ kind = \"company\", fiscal_year = 2031, {PROFILE} }}"
    ))
    .unwrap_err()
    .to_string();
    assert!(e.contains("no proof rules for fiscal year 2031"), "{e}");
}

#[test]
fn an_unknown_kind_is_an_error() {
    let e = load("{ kind = \"charity\", fiscal_year = 2026 }")
        .unwrap_err()
        .to_string();
    assert!(e.contains("unknown proof policy"), "{e}");
}

#[test]
fn a_bad_rfc_is_an_error() {
    let e = load(r#"{ kind = "company", fiscal_year = 2026, profile = { rfc = "xia190128j61", name = "X", postcode = "06000", regime = "603" } }"#)
        .unwrap_err()
        .to_string();
    assert!(e.contains("invalid RFC"), "{e}");
}

#[test]
fn a_missing_profile_fails_the_step_with_how_to_fix_it() {
    let plan = load("{ kind = \"nonprofit\", fiscal_year = 2026 }").unwrap();
    let report = execute(&plan, &registry(), &mut rdx_plan::rdx_host::Host::new()).unwrap();
    assert!(!report.ok);
    let e = report.steps[0].error.as_deref().unwrap();
    assert!(
        e.contains("needs `profile`") && e.contains("informal-group"),
        "{e}"
    );
}
