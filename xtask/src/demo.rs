//! `cargo xtask demo [OUT]`: runs the whole pipeline on synthetic documents with the local host,
//! chaining skills the way an agent would, and leaves every report, the redacted PDF, the
//! Beancount ledger, the XLSX export and a transcript in OUT (default `target/demo`).
//!
//! The statement is the scanned `example-bank` PDF and one receipt is a photo, so both go
//! through OCR. Sealing and opening a statement have no product skill yet, so the demo runs the
//! two small plans in `xtask/demo/plans`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::Value;

const ADDRESS: &str = "supporter1@example.org";
const SUPPORTER: &str = "s-0001";
// The scanned statement's Paperleaf line reads `PAPERI FAF SUPPLY` after OCR, which scores 50
// on merchant similarity against the bill; the default threshold (60) would leave it unmatched.
const POLICY: &str = "{ merchant.min_similarity = 50 }";

pub fn run(root: &Path, out: Option<&str>) -> ExitCode {
    let out = out.map_or_else(|| root.join("target/demo"), PathBuf::from);
    match Demo::new(root, &out).and_then(|mut d| d.run()) {
        Ok(()) => {
            println!("demo: outputs in {}", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("demo: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Demo {
    root: PathBuf,
    out: PathBuf,
    state: PathBuf,
    step: usize,
    transcript: String,
}

fn q(s: &str) -> String {
    format!("\"{s}\"")
}

fn import(path: &Path) -> String {
    format!("import \"{}\"", path.display())
}

fn import_text(path: &Path) -> String {
    format!("import \"{}\" as 'Text", path.display())
}

fn money(v: &Value) -> String {
    let minor = v["minor"].as_i64().unwrap_or(0);
    let sign = if minor < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02} {}",
        minor.abs() / 100,
        minor.abs() % 100,
        v["currency"].as_str().unwrap_or("")
    )
}

impl Demo {
    fn new(root: &Path, out: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let _ = fs::remove_dir_all(out);
        fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
        let out = out.canonicalize().map_err(|e| e.to_string())?;
        Ok(Demo {
            state: out.join("state"),
            root,
            out,
            step: 0,
            transcript: String::from("# Receipt Dropbox demo transcript\n\n"),
        })
    }

    fn note(&mut self, text: &str) {
        let _ = writeln!(self.transcript, "{text}\n");
    }

    /// Runs `rdx` in-process, shows the command in the transcript, saves the report and returns it.
    fn rdx(&mut self, name: &str, args: &[String]) -> Result<Value, String> {
        let mut full = vec![
            "--root".to_owned(),
            self.root.display().to_string(),
            "--state".to_owned(),
            self.state.display().to_string(),
        ];
        full.extend_from_slice(args);
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = rdx_cli::main(&full, &mut stdout, &mut stderr);
        let shown: Vec<String> = args
            .iter()
            .map(|a| {
                if a.contains(' ') || a.contains('"') || a.contains('\'') {
                    format!("'{}'", a.replace('\'', "'\\''"))
                } else {
                    a.clone()
                }
            })
            .collect();
        let stdout = String::from_utf8_lossy(&stdout).into_owned();
        let stderr = String::from_utf8_lossy(&stderr).into_owned();
        let _ = writeln!(
            self.transcript,
            "```console\n$ rdx {}\n{}```\n",
            shown.join(" "),
            stderr
        );
        if args.first().map(String::as_str) == Some("blob") {
            let _ = writeln!(self.transcript, "```text\n{}```\n", stdout);
            return if code == 0 {
                Ok(Value::String(stdout.trim().to_owned()))
            } else {
                Err(format!("rdx {}: {stderr}", args.join(" ")))
            };
        }
        self.step += 1;
        let file = format!("{:02}-{name}.json", self.step);
        fs::write(self.out.join(&file), &stdout).map_err(|e| e.to_string())?;
        let _ = writeln!(self.transcript, "Report: `{file}`\n");
        let report: Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("rdx {}: no report ({e}): {stderr}", args.join(" ")))?;
        Ok(report)
    }

    /// Like `rdx`, but a failed run is an error.
    fn ok(&mut self, name: &str, args: &[String]) -> Result<Value, String> {
        let report = self.rdx(name, args)?;
        if report["ok"] != Value::Bool(true) {
            return Err(format!(
                "{name} failed; see its report in {}",
                self.out.display()
            ));
        }
        Ok(report)
    }

    fn write(&self, name: &str, text: &str) -> Result<PathBuf, String> {
        let path = self.out.join(name);
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    fn json(&self, name: &str, value: &Value) -> Result<PathBuf, String> {
        self.write(name, &serde_json::to_string_pretty(value).expect("json"))
    }

    fn blob(&self, digest: &str, name: &str) -> Result<PathBuf, String> {
        let from = self
            .state
            .join("blobs")
            .join(digest.trim_start_matches("sha256:"));
        let to = self.out.join(name);
        fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
        Ok(to)
    }

    fn run(&mut self) -> Result<(), String> {
        let root = self.root.clone();
        let demo = root.join("xtask/demo");
        let models = std::env::var("RDX_OCR_MODELS")
            .map_err(|_| "RDX_OCR_MODELS is not set: run it in the dev shell (nix develop)")?;

        self.note(
            "## 1. Onboard the supporter\n\nKeys for this device and 2-of-3 recovery shares.",
        );
        let r = self.ok(
            "onboard",
            &[
                "run".into(),
                "onboard-supporter".into(),
                "--input".into(),
                format!("address={}", q(ADDRESS)),
                "--input".into(),
                format!("supporter={}", q(SUPPORTER)),
                "--input".into(),
                "device=\"laptop\"".into(),
                "--execute".into(),
            ],
        )?;
        let shares = r["steps"][0]["output"]["shares"]
            .as_array()
            .map_or(0, Vec::len);
        self.note(&format!(
            "{shares} recovery shares were shown to the supporter (state/secrets/shown/)."
        ));

        self.note("## 2. Read the scanned statement (OCR)");
        for m in ["text-detection.rten", "text-recognition.rten"] {
            self.rdx("", &["blob".into(), "put".into(), format!("{models}/{m}")])?;
        }
        let scan = root.join("fixtures/scans/st-2026-03.s-0001.scan.pdf");
        let doc = self.rdx(
            "",
            &["blob".into(), "put".into(), scan.display().to_string()],
        )?;
        let doc = doc.as_str().unwrap_or_default().to_owned();
        let r = self.ok(
            "statement",
            &[
                "run".into(),
                "ingest-statement".into(),
                "--input".into(),
                format!("document={}", q(&doc)),
                "--input".into(),
                format!("supporter={}", q(SUPPORTER)),
            ],
        )?;
        let text = &r["steps"][0]["output"];
        let parsed = &r["steps"][1]["output"];
        let statement = parsed["statement"].clone();
        self.note(&format!(
            "Read by `{}` with {} line(s); checks: {}; OCR warnings: {}.",
            text["source"].as_str().unwrap_or("?"),
            statement["lines"].as_array().map_or(0, Vec::len),
            parsed["checks"],
            text["warnings"]
        ));
        let statement_file = self.json("statement.json", &statement)?;

        self.note("## 3. Seal the statement\n\nSigned by the device key, encrypted to the supporter's keys, kept in the blob store.");
        let r = self.ok(
            "seal",
            &[
                "run".into(),
                demo.join("plans/seal-statement").display().to_string(),
                "--input".into(),
                format!("statement={}", import(&statement_file)),
                "--input".into(),
                format!("address={}", q(ADDRESS)),
                "--execute".into(),
            ],
        )?;
        let sealed = r["steps"][0]["output"].clone();
        let signer = sealed["signer"].as_str().unwrap_or_default().to_owned();
        let signer_file = self.write("signer.ur", &signer)?;
        self.note(&format!(
            "Encrypted envelope `{}`, root `{}`.",
            sealed["blob"].as_str().unwrap_or("?"),
            sealed["root"].as_str().unwrap_or("?")
        ));

        self.note("## 4. Read the receipts");
        let mut receipts = Vec::new();
        let mut inputs: Vec<(String, Vec<String>)> = [
            "lantern-catering.txt",
            "metro-rideshare.txt",
            "clearwave-bill.txt",
            "paperleaf-refund.txt",
        ]
        .iter()
        .map(|f| {
            (
                f.trim_end_matches(".txt").to_owned(),
                vec![
                    format!("document={}", import_text(&demo.join("inbox").join(f))),
                    "media_type=\"text/plain\"".into(),
                ],
            )
        })
        .collect();
        // The emailed bill prints `$` alone, so the organization's currency comes as a hint.
        inputs.push((
            "paperleaf-bill".into(),
            vec![
                format!(
                    "document={}",
                    import_text(&root.join("fixtures/receipts/bill-email.eml"))
                ),
                "hints={ currency = \"USD\" }".into(),
            ],
        ));
        let photo = root.join("fixtures/scans/receipt-photo.png");
        let photo = self.rdx(
            "",
            &["blob".into(), "put".into(), photo.display().to_string()],
        )?;
        inputs.push((
            "hardware-photo".into(),
            vec![
                format!("document={}", q(photo.as_str().unwrap_or_default())),
                "media_type=\"image/png\"".into(),
                "hints={ currency = \"MXN\" }".into(),
            ],
        ));
        for (name, extra) in inputs {
            let mut args = vec![
                "run".into(),
                "ingest-receipt".into(),
                "--input".into(),
                format!("supporter={}", q(SUPPORTER)),
            ];
            for e in extra {
                args.push("--input".into());
                args.push(e);
            }
            let r = self.ok(&format!("receipt-{name}"), &args)?;
            let out = r["steps"]
                .as_array()
                .and_then(|s| s.last())
                .map(|s| s["output"].clone());
            let out = out.unwrap_or_default();
            if out["status"] != "complete" {
                return Err(format!("receipt {name} needs review: {}", out["missing"]));
            }
            let rc = &out["receipt"];
            self.note(&format!(
                "`{}`: {} on {}, {} ({}).",
                rc["id"].as_str().unwrap_or("?"),
                rc["merchant"].as_str().unwrap_or("?"),
                rc["date"].as_str().unwrap_or("?"),
                money(&rc["total"]),
                rc["kind"].as_str().unwrap_or("?")
            ));
            receipts.push(rc.clone());
        }
        let receipts_file = self.json("receipts.json", &Value::Array(receipts))?;

        self.note("## 5. Propose matches");
        let base = vec![
            "run".to_owned(),
            "reconcile-statement".into(),
            "--input".into(),
            format!("statement={}", import(&statement_file)),
            "--input".into(),
            format!("receipts={}", import(&receipts_file)),
            "--input".into(),
            format!("policy={POLICY}"),
        ];
        let r = self.ok("proposals", &base)?;
        let p = &r["steps"][0]["output"];
        let mut confirmed = Vec::new();
        for prop in p["proposals"].as_array().into_iter().flatten() {
            for m in prop["matches"].as_array().into_iter().flatten() {
                let mut m = m.clone();
                m["status"] = "confirmed".into();
                confirmed.push(m);
            }
        }
        self.note(&format!(
            "{} proposal(s); unmatched lines {}; unmatched receipts {}; notes {}.\n\nThe demo confirms every proposal, as the supporter would after reviewing them.",
            p["proposals"].as_array().map_or(0, Vec::len),
            p["unmatched_lines"],
            p["unmatched_receipts"],
            p["notes"]
        ));
        let confirmed_file = self.json("confirmed.json", &Value::Array(confirmed))?;

        self.note("## 6. Book the confirmed matches to the ledger");
        let mut book = base.clone();
        book.extend([
            "--input".into(),
            format!("decided={}", import(&confirmed_file)),
            "--input".into(),
            format!("accounts={}", import(&demo.join("accounts.ncl"))),
            "--execute".into(),
        ]);
        let r = self.ok("book", &book)?;
        let summary = &r["steps"][1]["output"]["summary"];
        self.note(&format!(
            "{} entries; the organization owes the supporter {}.",
            summary["entries"],
            money(&summary["reimbursable"])
        ));

        self.note(
            "## 7. Open the sealed statement on the supporter's device and publish it redacted",
        );
        let r = self.ok(
            "open",
            &[
                "run".into(),
                demo.join("plans/open-statement").display().to_string(),
                "--input".into(),
                format!("blob={}", q(sealed["blob"].as_str().unwrap_or_default())),
                "--input".into(),
                format!("address={}", q(ADDRESS)),
            ],
        )?;
        let full = r["steps"][0]["output"]["envelope"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let full_file = self.write("full.ur", &full)?;
        let r = self.ok(
            "publish",
            &[
                "run".into(),
                "publish-redacted-statement".into(),
                "--input".into(),
                format!("envelope={}", import_text(&full_file)),
                "--input".into(),
                format!("signer={}", import_text(&signer_file)),
                "--input".into(),
                format!("matches={}", import(&confirmed_file)),
                "--execute".into(),
            ],
        )?;
        let redacted = r["steps"][0]["output"]["envelope"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let redacted_file = self.write("redacted.ur", &redacted)?;
        let pdf = r["steps"][2]["output"]["document"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        self.blob(&pdf, "redacted-statement.pdf")?;
        let s = &r["steps"][1]["output"]["summary"];
        self.note(&format!(
            "`redacted-statement.pdf`: opening {} + shown {} + redacted {} = closing {}.",
            money(&s["opening"]),
            money(&s["shown"]),
            money(&s["redacted"]),
            money(&s["closing"])
        ));

        self.note("## 8. Verify as the accountant would\n\nWith the supporter's key, then with the wrong key.");
        self.ok(
            "verify",
            &[
                "run".into(),
                "verify-statement".into(),
                "--input".into(),
                format!("envelope={}", import_text(&redacted_file)),
                "--input".into(),
                format!("signer={}", import_text(&signer_file)),
            ],
        )?;
        // The skill's default signer is the published synthetic key, not this supporter's.
        let r = self.rdx(
            "verify-wrong-key",
            &[
                "run".into(),
                "verify-statement".into(),
                "--input".into(),
                format!("envelope={}", import_text(&redacted_file)),
            ],
        )?;
        if r["ok"] != Value::Bool(false) {
            return Err("verify-statement accepted the wrong key".into());
        }

        self.note("## 9. Export the ledger");
        let r = self.ok(
            "export",
            &[
                "run".into(),
                "export-ledger".into(),
                "--input".into(),
                "format='xlsx".into(),
                "--execute".into(),
            ],
        )?;
        self.write(
            "ledger.beancount",
            r["steps"][0]["output"]["text"].as_str().unwrap_or_default(),
        )?;
        let xlsx = r["steps"][1]["output"]["document"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        self.blob(&xlsx, "ledger.xlsx")?;

        self.note("## 10. Check the audit log");
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let code = rdx_cli::main(
            &[
                "--root".into(),
                self.root.display().to_string(),
                "--state".into(),
                self.state.display().to_string(),
                "audit".into(),
                "verify".into(),
            ],
            &mut o,
            &mut e,
        );
        let _ = writeln!(
            self.transcript,
            "```console\n$ rdx audit verify\n{}{}```\n",
            String::from_utf8_lossy(&o),
            String::from_utf8_lossy(&e)
        );
        // Paths relative to the repository, so the transcript reads the same on any machine.
        let transcript = std::mem::take(&mut self.transcript)
            .replace(&self.out.display().to_string(), "OUT")
            .replace(&self.root.display().to_string(), ".")
            .replace(&models, "$RDX_OCR_MODELS");
        self.write("transcript.md", &transcript)?;
        if code != 0 {
            return Err("the audit log does not verify".into());
        }
        Ok(())
    }
}
