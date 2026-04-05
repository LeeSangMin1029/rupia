use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rupia_core::diagnostic::{
    diagnose_parse_errors, format_diagnostics, format_diagnostics_json,
};
use rupia_core::guard;
use rupia_core::types::{ParseResult, Validation};

/// rupia — LLM output validation harness
///
/// Lenient JSON parsing, schema-based coercion, structured feedback for self-healing loops.
/// Language-agnostic: use from Go, Python, or any language via JSON Schema files.
#[derive(Parser)]
#[command(name = "rupia", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Verbose: print diagnostics to stderr
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Parse lenient JSON (handles markdown blocks, trailing commas, incomplete keywords)
    Parse {
        /// Input file (- or omit for stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
    },
    /// Validate input against a JSON Schema
    Validate {
        /// JSON Schema file
        #[arg(short, long)]
        schema: PathBuf,
        /// Input file (- or omit for stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Strict mode: reject extra properties
        #[arg(long)]
        strict: bool,
    },
    /// Generate LLM feedback from validation errors
    Feedback {
        /// JSON Schema file
        #[arg(short, long)]
        schema: PathBuf,
        /// Input file (- or omit for stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
    },
    /// Full pipeline: parse → coerce → validate → diagnostics
    Check {
        /// JSON Schema file
        #[arg(short, long)]
        schema: PathBuf,
        /// Input file (- or omit for stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Strict mode
        #[arg(long)]
        strict: bool,
        /// Output diagnostics as JSON
        #[arg(long)]
        json: bool,
    },
    /// Generate random data matching a JSON Schema
    Random {
        /// JSON Schema file
        #[arg(short, long)]
        schema: PathBuf,
        /// Number of samples to generate
        #[arg(short, long, default_value = "1")]
        count: u32,
    },
    /// Validate a schema file itself (check for common issues)
    LintSchema {
        /// Schema file to lint
        #[arg(short, long)]
        schema: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Parse { input } => cmd_parse(input),
        Command::Validate {
            schema,
            input,
            strict,
        } => cmd_validate(&schema, input, strict, cli.verbose),
        Command::Feedback { schema, input } => cmd_feedback(&schema, input),
        Command::Check {
            schema,
            input,
            strict,
            json,
        } => cmd_check(&schema, input, strict, json, cli.verbose),
        Command::Random { schema, count } => cmd_random(&schema, count),
        Command::LintSchema { schema } => cmd_lint_schema(&schema),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[error] {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn read_input(path: Option<PathBuf>) -> Result<String> {
    match path {
        Some(p) if p.to_string_lossy() != "-" => {
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))
        }
        _ => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading stdin")?;
            Ok(buf)
        }
    }
}

fn load_schema(path: &PathBuf) -> Result<serde_json::Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading schema {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parsing schema {}", path.display()))
}

fn cmd_parse(input: Option<PathBuf>) -> Result<()> {
    let raw = read_input(input)?;
    match rupia_core::lenient::parse(&raw) {
        ParseResult::Success(v) => {
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        ParseResult::Failure { errors, .. } => {
            let diags = diagnose_parse_errors(&errors, &raw);
            eprint!("{}", format_diagnostics(&diags));
            bail!("parse failed with {} error(s)", errors.len())
        }
    }
}

fn cmd_validate(
    schema_path: &PathBuf,
    input: Option<PathBuf>,
    strict: bool,
    verbose: bool,
) -> Result<()> {
    let raw = read_input(input)?;
    let schema = load_schema(schema_path)?;
    let config = guard::Config {
        strict,
        verbose,
        ..Default::default()
    };
    match guard::check(&raw, &schema, &config) {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result.value)?);
            Ok(())
        }
        Err(e) => {
            eprint!("{}", format_diagnostics(&e.diagnostics));
            if let Some(fb) = &e.last_feedback {
                eprintln!("\n--- LLM Feedback ---\n{fb}");
            }
            bail!("validation failed")
        }
    }
}

fn cmd_feedback(schema_path: &PathBuf, input: Option<PathBuf>) -> Result<()> {
    let raw = read_input(input)?;
    let schema = load_schema(schema_path)?;
    let parsed = match rupia_core::lenient::parse(&raw) {
        ParseResult::Success(v) => v,
        ParseResult::Failure { errors, .. } => {
            let diags = diagnose_parse_errors(&errors, &raw);
            eprint!("{}", format_diagnostics(&diags));
            println!("JSON parse failed. Please return valid JSON matching the schema.");
            return Ok(());
        }
    };
    let coerced = rupia_core::coerce::coerce_with_schema(parsed, &schema);
    match rupia_core::validator::validate(&coerced, &schema) {
        Validation::Success(_) => {
            println!("valid — no feedback needed");
            Ok(())
        }
        Validation::Failure(f) => {
            println!("{}", f.to_llm_feedback());
            Ok(())
        }
    }
}

fn cmd_check(
    schema_path: &PathBuf,
    input: Option<PathBuf>,
    strict: bool,
    json_output: bool,
    verbose: bool,
) -> Result<()> {
    let raw = read_input(input)?;
    let schema = load_schema(schema_path)?;
    let config = guard::Config {
        strict,
        verbose,
        ..Default::default()
    };
    match guard::check(&raw, &schema, &config) {
        Ok(result) => {
            let output = if json_output {
                serde_json::json!({
                    "status": "valid",
                    "data": result.value,
                    "diagnostics": format_diagnostics_json(&result.diagnostics),
                })
            } else {
                serde_json::json!({
                    "status": "valid",
                    "data": result.value,
                })
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
            Ok(())
        }
        Err(e) => {
            let feedback = e.last_feedback.as_deref().unwrap_or("");
            let output = if json_output {
                serde_json::json!({
                    "status": "invalid",
                    "error_count": e.diagnostics.iter().filter(|d| d.severity == rupia_core::diagnostic::Severity::Error).count(),
                    "diagnostics": format_diagnostics_json(&e.diagnostics),
                    "feedback": feedback,
                })
            } else {
                serde_json::json!({
                    "status": "invalid",
                    "error_count": e.diagnostics.len(),
                    "feedback": feedback,
                    "errors": e.diagnostics.iter().map(|d| serde_json::json!({
                        "code": d.code,
                        "message": d.message,
                        "help": d.help,
                    })).collect::<Vec<_>>(),
                })
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
            Ok(())
        }
    }
}

fn cmd_random(schema_path: &PathBuf, count: u32) -> Result<()> {
    let schema = load_schema(schema_path)?;
    for _ in 0..count {
        let value = rupia_core::random::generate(&schema);
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}

fn cmd_lint_schema(schema_path: &std::path::Path) -> Result<()> {
    let path_str = schema_path.to_string_lossy();
    let diags = guard::check_schema_file(&path_str);
    if diags.is_empty() {
        println!("schema OK: {path_str}");
        return Ok(());
    }
    eprint!("{}", format_diagnostics(&diags));
    let errors = diags
        .iter()
        .filter(|d| d.severity == rupia_core::diagnostic::Severity::Error)
        .count();
    if errors > 0 {
        bail!("{errors} schema error(s)")
    }
    Ok(())
}
