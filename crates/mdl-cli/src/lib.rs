//! Minimal single-file command-line driver for the MDL compiler.
//!
//! The compiler library remains purely in-memory. This crate owns argument parsing,
//! source-file I/O, diagnostic presentation, and safe datapack materialization.

mod materialize;

use std::error::Error;
use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::fs;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::diagnostic::{Diagnostics, render_diagnostics};
use mdl_compiler::frontend::{
    CompilationFailure, CompilationOptions, FrontendLimits, SourceInput, compile_source,
};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

pub use materialize::{MaterializeError, materialize_artifact};

/// Current CLI help text.
pub const HELP: &str = "\
MDL source-to-datapack compiler

Usage:
  mdl compile <input.mdl> --output <directory> \\
      --core-opt <none|baseline> --minecraft-opt <none|baseline> \\
      --namespace <namespace> --register-objective <objective> \\
      --description <text>

The Phase 1 CLI compiles exactly one source file for Minecraft Java Edition 26.2.
Every CLI-exposed choice is explicit. Remaining compiler limits and target assumptions
are fixed and documented; the compiler library performs no file or module discovery.
";

/// CLI version, kept in sync with the package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const ANALYSIS_GRAPH_ENTITY_LIMIT: usize = 100_000;
const ANALYSIS_SOLVER_UPDATE_LIMIT: usize = 100_000;

/// Runs the CLI from an argv-like sequence and returns text for standard output.
///
/// The first value is treated as the executable name. No process-global state other
/// than filesystem access is read, which keeps command-line integration tests exact.
///
/// # Errors
///
/// Returns a usage, source I/O, compilation, or artifact-materialization failure.
pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<String, CliError> {
    let command = Command::parse(arguments)?;
    match command {
        Command::Help => Ok(HELP.trim_end().to_owned()),
        Command::Version => Ok(format!("mdl {VERSION}")),
        Command::Compile(config) => compile(&config),
    }
}

fn compile(config: &CompileConfig) -> Result<String, CliError> {
    let source = fs::read_to_string(&config.input).map_err(|source| CliError::SourceIo {
        path: config.input.clone(),
        source,
    })?;
    let options = config.compilation_options()?;
    let diagnostic_name = config.input.to_string_lossy().into_owned();
    let output = compile_source(SourceInput::new(diagnostic_name, source), &options)
        .map_err(CliError::Compilation)?;

    let report = success_report(config, &output)?;
    materialize_artifact(output.emission().pack(), &config.output)
        .map_err(CliError::Materialization)?;
    Ok(report)
}

fn success_report(
    config: &CompileConfig,
    output: &mdl_compiler::frontend::CompilationOutput,
) -> Result<String, CliError> {
    let mut report = String::new();
    writeln!(
        report,
        "compiled {} for Minecraft Java Edition {}",
        config.input.display(),
        JavaEditionTarget::V26_2
    )
    .expect("writing to a String cannot fail");
    writeln!(
        report,
        "wrote {} files to {}",
        output.emission().pack().files().len(),
        config.output.display()
    )
    .expect("writing to a String cannot fail");
    match output.target_analysis() {
        Ok(_) => report.push_str("target analysis: complete\n"),
        Err(failure) => writeln!(report, "target analysis: unavailable: {failure}")
            .expect("writing to a String cannot fail"),
    }
    report.push_str("source function ABI:\n");

    for function in output.checked_frontend().function_ids() {
        let name =
            output
                .source_function_name(function)
                .ok_or_else(|| CliError::InternalAbiMismatch {
                    function: function.index(),
                })?;
        let abi =
            output
                .source_function_abi(function)
                .ok_or_else(|| CliError::InternalAbiMismatch {
                    function: function.index(),
                })?;
        writeln!(
            report,
            "  function[{}] {name} entry {}",
            function.index(),
            abi.entry_resource()
        )
        .expect("writing to a String cannot fail");
        write_homes(&mut report, "parameter", abi.parameter_homes());
        write_homes(&mut report, "result", abi.result_homes());
    }

    Ok(report.trim_end().to_owned())
}

fn write_homes(
    report: &mut String,
    label: &str,
    homes: &[(
        mdl_compiler::ir::core::CoreType,
        mdl_compiler::lower::minecraft::RegisterSlot,
    )],
) {
    if homes.is_empty() {
        writeln!(report, "    {label}s: none").expect("writing to a String cannot fail");
        return;
    }
    for (index, (ty, slot)) in homes.iter().enumerate() {
        writeln!(
            report,
            "    {label}[{index}] {ty} -> {} {}",
            slot.holder(),
            slot.objective()
        )
        .expect("writing to a String cannot fail");
    }
}

#[derive(Debug)]
enum Command {
    Help,
    Version,
    Compile(CompileConfig),
}

#[derive(Debug)]
struct CompileConfig {
    input: PathBuf,
    output: PathBuf,
    core_optimization: CoreOptimizationLevel,
    minecraft_optimization: MinecraftOptimizationLevel,
    namespace: PackNamespace,
    register_objective: ObjectiveName,
    description: String,
}

impl CompileConfig {
    fn compilation_options(&self) -> Result<CompilationOptions, CliError> {
        let lowering = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            self.namespace.clone(),
            self.register_objective.clone(),
        )
        .map_err(|error| CliError::InvalidConfiguration(error.to_string()))?
        .with_optimization_level(self.minecraft_optimization);
        let assumptions = lowering.command_limit_assumptions();
        Ok(CompilationOptions::new(
            FrontendLimits::DEFAULT,
            CoreOptimizationOptions::new(self.core_optimization),
            lowering,
            TargetExecutionAnalysisLimits::new(
                AnalysisArithmeticCaps::minimum_for(assumptions),
                ANALYSIS_GRAPH_ENTITY_LIMIT,
                ANALYSIS_SOLVER_UPDATE_LIMIT,
            ),
            EmissionOptions::new(self.description.clone()),
        ))
    }
}

impl Command {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, CliError> {
        let mut arguments = arguments.into_iter();
        let _program = arguments.next();
        let Some(subcommand) = arguments.next() else {
            return Err(CliError::Usage("missing `compile` subcommand".to_owned()));
        };
        if subcommand == "--help" || subcommand == "-h" {
            return no_trailing(arguments, Self::Help);
        }
        if subcommand == "--version" || subcommand == "-V" {
            return no_trailing(arguments, Self::Version);
        }
        if subcommand != "compile" {
            return Err(CliError::Usage(format!(
                "unknown subcommand `{}`",
                subcommand.to_string_lossy()
            )));
        }

        let values = arguments.collect::<Vec<_>>();
        if matches!(values.as_slice(), [value] if value == "--help" || value == "-h") {
            return Ok(Self::Help);
        }
        Self::parse_compile(&values).map(Self::Compile)
    }

    fn parse_compile(arguments: &[OsString]) -> Result<CompileConfig, CliError> {
        let mut input = None;
        let mut output = None;
        let mut core_optimization = None;
        let mut minecraft_optimization = None;
        let mut namespace = None;
        let mut register_objective = None;
        let mut description = None;
        let mut positional_only = false;
        let mut index = 0;

        while index < arguments.len() {
            let argument = &arguments[index];
            if !positional_only && argument == "--" {
                positional_only = true;
                index += 1;
                continue;
            }
            if !positional_only && argument.to_string_lossy().starts_with('-') {
                let flag = argument.to_str().ok_or_else(|| {
                    CliError::Usage("option names must be valid UTF-8".to_owned())
                })?;
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| CliError::Usage(format!("option `{flag}` requires a value")))?;
                match flag {
                    "--output" => set_once(&mut output, PathBuf::from(value), flag)?,
                    "--core-opt" => {
                        set_once(&mut core_optimization, parse_core_level(value, flag)?, flag)?;
                    }
                    "--minecraft-opt" => set_once(
                        &mut minecraft_optimization,
                        parse_minecraft_level(value, flag)?,
                        flag,
                    )?,
                    "--namespace" => {
                        let value = utf8_option(value, flag)?;
                        let parsed = PackNamespace::new(value).map_err(|error| {
                            CliError::Usage(format!("invalid `{flag}` value `{value}`: {error}"))
                        })?;
                        set_once(&mut namespace, parsed, flag)?;
                    }
                    "--register-objective" => {
                        let value = utf8_option(value, flag)?;
                        let parsed = ObjectiveName::new(value).map_err(|error| {
                            CliError::Usage(format!("invalid `{flag}` value `{value}`: {error}"))
                        })?;
                        set_once(&mut register_objective, parsed, flag)?;
                    }
                    "--description" => {
                        set_once(&mut description, utf8_option(value, flag)?.to_owned(), flag)?;
                    }
                    _ => return Err(CliError::Usage(format!("unknown option `{flag}`"))),
                }
                index += 2;
                continue;
            }

            if input.replace(PathBuf::from(argument)).is_some() {
                return Err(CliError::Usage(
                    "exactly one input source file is supported".to_owned(),
                ));
            }
            index += 1;
        }

        Ok(CompileConfig {
            input: required(input, "input source file")?,
            output: required(output, "--output")?,
            core_optimization: required(core_optimization, "--core-opt")?,
            minecraft_optimization: required(minecraft_optimization, "--minecraft-opt")?,
            namespace: required(namespace, "--namespace")?,
            register_objective: required(register_objective, "--register-objective")?,
            description: required(description, "--description")?,
        })
    }
}

fn no_trailing(
    mut arguments: impl Iterator<Item = OsString>,
    command: Command,
) -> Result<Command, CliError> {
    if arguments.next().is_some() {
        Err(CliError::Usage(
            "help and version do not accept trailing arguments".to_owned(),
        ))
    } else {
        Ok(command)
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), CliError> {
    if slot.replace(value).is_some() {
        Err(CliError::Usage(format!(
            "option `{name}` may only be provided once"
        )))
    } else {
        Ok(())
    }
}

fn required<T>(value: Option<T>, name: &str) -> Result<T, CliError> {
    value.ok_or_else(|| CliError::Usage(format!("missing required {name}")))
}

fn utf8_option<'a>(value: &'a OsString, flag: &str) -> Result<&'a str, CliError> {
    value
        .to_str()
        .ok_or_else(|| CliError::Usage(format!("option `{flag}` requires valid UTF-8")))
}

fn parse_core_level(value: &OsString, flag: &str) -> Result<CoreOptimizationLevel, CliError> {
    match utf8_option(value, flag)? {
        "none" => Ok(CoreOptimizationLevel::None),
        "baseline" => Ok(CoreOptimizationLevel::Baseline),
        invalid => Err(CliError::Usage(format!(
            "invalid `{flag}` value `{invalid}`; expected `none` or `baseline`"
        ))),
    }
}

fn parse_minecraft_level(
    value: &OsString,
    flag: &str,
) -> Result<MinecraftOptimizationLevel, CliError> {
    match utf8_option(value, flag)? {
        "none" => Ok(MinecraftOptimizationLevel::None),
        "baseline" => Ok(MinecraftOptimizationLevel::Baseline),
        invalid => Err(CliError::Usage(format!(
            "invalid `{flag}` value `{invalid}`; expected `none` or `baseline`"
        ))),
    }
}

/// One complete CLI failure with a stable process exit category.
#[derive(Debug)]
#[non_exhaustive]
pub enum CliError {
    /// Invalid command-line usage.
    Usage(String),
    /// A validated option combination was rejected.
    InvalidConfiguration(String),
    /// The source file could not be read as UTF-8 text.
    SourceIo {
        /// Source path.
        path: PathBuf,
        /// Underlying filesystem error.
        source: std::io::Error,
    },
    /// The source program failed compilation.
    Compilation(CompilationFailure),
    /// A complete artifact could not be safely materialized.
    Materialization(MaterializeError),
    /// Successful products disagreed about a source function ABI.
    InternalAbiMismatch {
        /// Compilation-local source declaration index.
        function: u32,
    },
}

impl CliError {
    /// Returns the process exit code selected for this failure category.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) | Self::InvalidConfiguration(_) => 2,
            Self::SourceIo { .. }
            | Self::Compilation(_)
            | Self::Materialization(_)
            | Self::InternalAbiMismatch { .. } => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(formatter, "mdl: {message}\n\n{}", HELP.trim_end()),
            Self::InvalidConfiguration(message) => {
                write!(formatter, "mdl: invalid compiler configuration: {message}")
            }
            Self::SourceIo { path, source } => {
                write!(
                    formatter,
                    "mdl: failed to read {}: {source}",
                    path.display()
                )
            }
            Self::Compilation(failure) => {
                formatter.write_str("mdl: compilation failed")?;
                match (compilation_diagnostics(failure), failure.sources()) {
                    (Some(diagnostics), Some(sources)) => {
                        write!(formatter, ":\n{}", render_diagnostics(diagnostics, sources))
                    }
                    _ => write!(formatter, ": {failure}"),
                }
            }
            Self::Materialization(error) => write!(formatter, "mdl: {error}"),
            Self::InternalAbiMismatch { function } => write!(
                formatter,
                "mdl: internal source/lowering ABI mismatch for function[{function}]"
            ),
        }
    }
}

fn compilation_diagnostics(failure: &CompilationFailure) -> Option<&Diagnostics> {
    failure
        .diagnostics()
        .or_else(|| {
            failure
                .minecraft_lowering_failure()
                .map(mdl_compiler::lower::minecraft::LoweringFailure::diagnostics)
        })
        .or_else(|| {
            failure
                .core_optimization_failure()
                .map(mdl_compiler::opt::core::CoreOptimizationFailure::diagnostics)
        })
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SourceIo { source, .. } => Some(source),
            Self::Compilation(source) => Some(source),
            Self::Materialization(source) => Some(source),
            Self::Usage(_) | Self::InvalidConfiguration(_) | Self::InternalAbiMismatch { .. } => {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{CliError, Command, HELP};

    fn parse(arguments: &[&str]) -> Result<Command, CliError> {
        Command::parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn help_is_complete_and_does_not_compile() {
        assert!(matches!(parse(&["mdl", "--help"]), Ok(Command::Help)));
        assert!(HELP.contains("--minecraft-opt <none|baseline>"));
        assert!(HELP.contains("Minecraft Java Edition 26.2"));
    }

    #[test]
    fn compile_requires_every_output_affecting_choice() {
        let error = parse(&["mdl", "compile", "input.mdl"]).unwrap_err();
        assert!(error.to_string().contains("missing required --output"));
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn compile_rejects_unknown_and_duplicate_options() {
        let unknown = parse(&["mdl", "compile", "--wat", "x"]).unwrap_err();
        assert!(unknown.to_string().contains("unknown option `--wat`"));

        let duplicate = parse(&[
            "mdl",
            "compile",
            "input.mdl",
            "--output",
            "one",
            "--output",
            "two",
        ])
        .unwrap_err();
        assert!(
            duplicate
                .to_string()
                .contains("option `--output` may only be provided once")
        );
    }
}
