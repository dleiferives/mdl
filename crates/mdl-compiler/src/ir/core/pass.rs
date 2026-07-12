//! Named verify-after-pass execution and diagnostic failure bundles.

use std::error::Error;
use std::fmt;

use super::{CoreProgram, DebugDumper, Diagnostics, FunctionEditor, FunctionId, verify_function};
use crate::source::SourceContext;

/// One named in-place function transformation.
pub trait FunctionPass {
    /// Stable human-readable pass name.
    fn name(&self) -> &'static str;

    /// Applies this pass through the invariant-preserving editor.
    ///
    /// # Errors
    ///
    /// Returns an internal pass failure. Expected pattern non-matches should return
    /// `Ok(())` without mutation.
    fn run(&mut self, editor: &mut FunctionEditor<'_>) -> Result<(), PassError>;
}

/// Internal pass failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassError {
    message: String,
}

impl PassError {
    /// Creates a pass failure with actionable context.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the pass-provided failure message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PassError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PassError {}

/// Complete diagnostic record for the first failed pass in a pipeline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureBundle {
    /// Name of the pass that failed or produced invalid IR.
    pub pass: &'static str,
    /// Complete configured function-pass pipeline.
    pub pipeline: Vec<&'static str>,
    /// Optional malformed-safe IR captured before the failing pass.
    pub before: Option<String>,
    /// Malformed-safe IR after the pass failure.
    pub after: String,
    /// Direct pass error, when the pass returned failure.
    pub pass_error: Option<PassError>,
    /// Verify-after-pass diagnostics, when output IR was invalid.
    pub diagnostics: Option<Diagnostics>,
}

impl fmt::Display for FailureBundle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Core pass '{}' failed", self.pass)?;
        writeln!(formatter, "pipeline: {}", self.pipeline.join(" -> "))?;
        if let Some(error) = &self.pass_error {
            writeln!(formatter, "pass error: {error}")?;
        }
        if let Some(diagnostics) = &self.diagnostics {
            writeln!(formatter, "verifier diagnostics:\n{diagnostics}")?;
        }
        if let Some(before) = &self.before {
            writeln!(formatter, "before:\n{before}")?;
        }
        write!(formatter, "after:\n{}", self.after)
    }
}

impl Error for FailureBundle {}

/// Runs a named in-place pass pipeline and stops at the first failure.
#[derive(Clone, Copy, Debug)]
pub struct PassRunner {
    capture_before: bool,
}

impl PassRunner {
    /// Creates a runner with optional pre-failure IR capture.
    #[must_use]
    pub const fn new(capture_before: bool) -> Self {
        Self { capture_before }
    }

    /// Runs all passes on one defined function, verifying after every pass.
    ///
    /// A failure stops the pipeline. The possibly-invalid body is restored to the
    /// program solely for inspection; callers must discard that compilation unit.
    ///
    /// # Errors
    ///
    /// Returns a complete failure bundle for an absent body, pass error, editor-open
    /// failure, or verify-after-pass failure.
    pub fn run(
        &self,
        program: &mut CoreProgram,
        sources: &SourceContext,
        function: FunctionId,
        passes: &mut [&mut dyn FunctionPass],
    ) -> Result<(), Box<FailureBundle>> {
        let pipeline = passes.iter().map(|pass| pass.name()).collect::<Vec<_>>();
        for pass in passes {
            let pass_name = pass.name();
            let before = self
                .capture_before
                .then(|| DebugDumper::new(program).render());
            let Some(mut body) = program.take_function_body(function) else {
                return Err(Box::new(FailureBundle {
                    pass: pass_name,
                    pipeline,
                    before,
                    after: DebugDumper::new(program).render(),
                    pass_error: Some(PassError::new(format!("function {function:?} has no body"))),
                    diagnostics: None,
                }));
            };

            let pass_result = match FunctionEditor::new(program, sources, function, &mut body) {
                Ok(mut editor) => pass.run(&mut editor),
                Err(error) => Err(PassError::new(format!("cannot open editor: {error}"))),
            };
            let diagnostics = verify_function(program, sources, function, &body).err();
            program.restore_function_body(function, body);

            if let Err(pass_error) = pass_result {
                return Err(Box::new(FailureBundle {
                    pass: pass_name,
                    pipeline,
                    before,
                    after: DebugDumper::new(program).render(),
                    pass_error: Some(pass_error),
                    diagnostics,
                }));
            }
            if let Some(diagnostics) = diagnostics {
                return Err(Box::new(FailureBundle {
                    pass: pass_name,
                    pipeline,
                    before,
                    after: DebugDumper::new(program).render(),
                    pass_error: None,
                    diagnostics: Some(diagnostics),
                }));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{FunctionPass, PassError, PassRunner};
    use crate::ir::core::{
        CoreProgram, FunctionBuilder, FunctionEditor, Terminator, TerminatorKind,
    };
    use crate::source::{OriginId, SourceContext};

    struct CorruptTerminator;

    impl FunctionPass for CorruptTerminator {
        fn name(&self) -> &'static str {
            "corrupt-terminator"
        }

        fn run(&mut self, editor: &mut FunctionEditor<'_>) -> Result<(), PassError> {
            let entry = editor.body().entry();
            editor
                .body_mut_for_test()
                .block_mut(entry)
                .expect("test body has an entry")
                .terminator = None;
            Ok(())
        }
    }

    struct Observe<'a>(&'a Cell<bool>);

    impl FunctionPass for Observe<'_> {
        fn name(&self) -> &'static str {
            "observe"
        }

        fn run(&mut self, _editor: &mut FunctionEditor<'_>) -> Result<(), PassError> {
            self.0.set(true);
            Ok(())
        }
    }

    #[test]
    fn invalid_output_stops_the_pipeline_and_is_dumped() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("bad-pass"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let observed = Cell::new(false);
        let mut corrupt = CorruptTerminator;
        let mut observe = Observe(&observed);
        let bundle = PassRunner::new(true)
            .run(
                &mut program,
                &sources,
                function,
                &mut [&mut corrupt, &mut observe],
            )
            .unwrap_err();

        assert!(bundle.pass_error.is_none());
        assert!(
            bundle
                .diagnostics
                .as_ref()
                .unwrap()
                .contains_code("core.missing-terminator")
        );
        assert!(bundle.after.contains("term=None"));
        assert!(!observed.get());
    }
}
