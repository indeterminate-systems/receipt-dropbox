//! Evaluating Nickel plans with `nickel-embed`.

use std::path::{Path, PathBuf};

use nickel_embed::{Diagnostic, ExportFormat, Program, Source};

use crate::plan::{Plan, PlanError};

/// A Nickel program that evaluates to a record with a `plan` field (a skill's `scripts/main.ncl`).
pub struct PlanSource {
    source: Source,
    import_paths: Vec<PathBuf>,
    overrides: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// Parse, typecheck or contract errors, with source locations an agent can act on.
    #[error("{message}")]
    Nickel {
        message: String,
        diagnostics: Vec<Diagnostic>,
    },
    #[error("the program's `plan` field is not a plan: {0}")]
    Shape(String),
    #[error(transparent)]
    Invalid(#[from] PlanError),
}

impl PlanSource {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(Source::file(path.into()))
    }

    pub fn code(code: impl Into<String>) -> Self {
        Self::new(Source::code(code))
    }

    fn new(source: Source) -> Self {
        Self {
            source,
            import_paths: Vec::new(),
            overrides: Vec::new(),
        }
    }

    /// A directory searched by `import "…"`; add the repo's `nickel/` so `rdx/api.ncl` resolves.
    pub fn import_path(mut self, dir: impl AsRef<Path>) -> Self {
        self.import_paths.push(dir.as_ref().to_path_buf());
        self
    }

    /// Sets a top-level input field (such as `statement`) to a Nickel expression.
    pub fn input(mut self, field: impl Into<String>, nickel_code: impl Into<String>) -> Self {
        self.overrides.push((field.into(), nickel_code.into()));
        self
    }

    /// Evaluates the program, checks the plan's contracts and structure, and returns it.
    pub fn load(self) -> Result<Plan, LoadError> {
        let mut builder = Program::builder()
            .source(self.source)
            .import_paths(self.import_paths)
            .field(parse_path("plan")?);
        for (field, code) in self.overrides {
            builder = builder.override_value(parse_path(&field)?, code);
        }
        let json = builder
            .build()
            .and_then(|mut program| program.export(ExportFormat::Json))
            .map_err(|e| LoadError::Nickel {
                message: e.to_string(),
                diagnostics: e.diagnostics(),
            })?;
        let plan: Plan =
            serde_json::from_str(&json).map_err(|e| LoadError::Shape(e.to_string()))?;
        plan.validate()?;
        Ok(plan)
    }
}

fn parse_path(path: &str) -> Result<nickel_embed::FieldPath, LoadError> {
    path.parse()
        .map_err(|e: nickel_embed::Error| LoadError::Nickel {
            message: e.to_string(),
            diagnostics: e.diagnostics(),
        })
}
