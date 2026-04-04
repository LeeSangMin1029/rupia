use std::io::Read;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
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
}

#[derive(Subcommand)]
enum Command {
    /// Parse lenient JSON (handles markdown blocks, trailing commas, incomplete keywords)
    Parse {
        /// Input file (- or omit for stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output format: json (default) or raw
        #[arg(short, long, default_value = "json")]
        format: String,
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
    /// Full pipeline: parse → coerce → validate → feedback
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
    },
    /// Generate JSON Schema from a Rust type (via schemars)
    Schema {
        /// Schema file to pretty-print
        #[arg(short, long)]
        input: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Parse { input, format } => cmd_parse(input, &format),
        Command::Validate {
            schema,
            input,
            strict,
        } => cmd_validate(&schema, input, strict),
        Command::Feedback { schema, input } => cmd_feedback(&schema, input),
        Command::Check {
            schema,
            input,
            strict,
        } => cmd_check(&schema, input, strict),
        Command::Schema { input } => cmd_schema(&input),
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

fn cmd_parse(input: Option<PathBuf>, format: &str) -> Result<()> {
    let raw = read_input(input)?;
    match rupia_core::lenient::parse(&raw) {
        ParseResult::Success(v) => {
            match format {
                "raw" => println!("{v}"),
                _ => println!("{}", serde_json::to_string_pretty(&v)?),
            }
            Ok(())
        }
        ParseResult::Failure { errors, .. } => {
            for e in &errors {
                eprintln!("error: {}: expected {}", e.path, e.expected);
                if let Some(desc) = &e.description {
                    eprintln!("  {desc}");
                }
            }
            bail!("parse failed with {} error(s)", errors.len())
        }
    }
}

fn cmd_validate(schema_path: &PathBuf, input: Option<PathBuf>, strict: bool) -> Result<()> {
    let raw = read_input(input)?;
    let schema = load_schema(schema_path)?;
    let parsed = match rupia_core::lenient::parse(&raw) {
        ParseResult::Success(v) => v,
        ParseResult::Failure { errors, .. } => {
            for e in &errors {
                eprintln!("parse error: {}: {}", e.path, e.expected);
            }
            bail!("input is not valid JSON")
        }
    };
    let coerced = rupia_core::coerce::coerce_with_schema(parsed, &schema);
    let result = if strict {
        rupia_core::validator::validate_strict(&coerced, &schema)
    } else {
        rupia_core::validator::validate(&coerced, &schema)
    };
    match result {
        Validation::Success(_) => {
            println!("valid");
            Ok(())
        }
        Validation::Failure(f) => {
            for e in &f.errors {
                eprintln!("❌ {e}");
            }
            bail!("{} validation error(s)", f.errors.len())
        }
    }
}

fn cmd_feedback(schema_path: &PathBuf, input: Option<PathBuf>) -> Result<()> {
    let raw = read_input(input)?;
    let schema = load_schema(schema_path)?;
    let parsed = match rupia_core::lenient::parse(&raw) {
        ParseResult::Success(v) => v,
        ParseResult::Failure { .. } => {
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

fn cmd_check(schema_path: &PathBuf, input: Option<PathBuf>, strict: bool) -> Result<()> {
    let raw = read_input(input)?;
    let schema = load_schema(schema_path)?;
    let parsed = match rupia_core::lenient::parse(&raw) {
        ParseResult::Success(v) => v,
        ParseResult::Failure { errors, .. } => {
            let output = serde_json::json!({
                "status": "parse_error",
                "errors": errors.iter().map(|e| serde_json::json!({
                    "path": e.path,
                    "expected": e.expected,
                    "description": e.description,
                })).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
            return Ok(());
        }
    };
    let coerced = rupia_core::coerce::coerce_with_schema(parsed, &schema);
    let result = if strict {
        rupia_core::validator::validate_strict(&coerced, &schema)
    } else {
        rupia_core::validator::validate(&coerced, &schema)
    };
    match result {
        Validation::Success(data) => {
            let output = serde_json::json!({
                "status": "valid",
                "data": data,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
            Ok(())
        }
        Validation::Failure(f) => {
            let output = serde_json::json!({
                "status": "invalid",
                "error_count": f.error_count(),
                "feedback": f.to_llm_feedback(),
                "errors": f.errors.iter().map(|e| serde_json::json!({
                    "path": e.path,
                    "expected": e.expected,
                    "value": e.value,
                })).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
            Ok(())
        }
    }
}

fn cmd_schema(input: &PathBuf) -> Result<()> {
    let schema = load_schema(input)?;
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}
