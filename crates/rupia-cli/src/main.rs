use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rupia_core::diagnostic::{diagnose_parse_errors, format_diagnostics, format_diagnostics_json};
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
        /// Verify counterexamples from AVE package are rejected by the schema
        #[arg(long)]
        verify_counterexamples: bool,
    },
    /// Generate boundary test cases from a JSON Schema
    BoundaryGen {
        /// JSON Schema file
        #[arg(short, long)]
        schema: PathBuf,
    },
    /// AVE pipeline: schema-driven validation with confidence scoring
    Ave {
        /// Domain description for schema generation
        #[arg(short, long)]
        domain: Option<String>,
        /// Existing JSON Schema file
        #[arg(short, long)]
        schema: Option<PathBuf>,
        /// Input file to validate (- or omit for stdin)
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Model tier: haiku, sonnet, opus
        #[arg(short, long, default_value = "sonnet")]
        model: String,
        /// Output as structured JSON
        #[arg(long)]
        json: bool,
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
        Command::BoundaryGen { schema } => cmd_boundary_gen(&schema),
        Command::LintSchema {
            schema,
            verify_counterexamples,
        } => cmd_lint_schema(&schema, verify_counterexamples),
        Command::Ave {
            domain,
            schema,
            input,
            model,
            json,
        } => cmd_ave(domain, schema.as_ref(), input, &model, json),
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

struct LoadedSchema {
    schema: serde_json::Value,
    relations: Vec<rupia_core::ave::FieldRelation>,
    counterexamples: Vec<serde_json::Value>,
}

fn load_schema(path: &PathBuf) -> Result<serde_json::Value> {
    Ok(load_schema_full(path)?.schema)
}

fn load_schema_full(path: &PathBuf) -> Result<LoadedSchema> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading schema {}", path.display()))?;
    let root: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parsing schema {}", path.display()))?;
    if let Some(inner) = root.get("schema") {
        if inner.get("type").is_some() || inner.get("properties").is_some() {
            let pkg = rupia_core::ave::parse_schema_package(&content)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            return Ok(LoadedSchema {
                schema: pkg.schema,
                relations: pkg.relations,
                counterexamples: pkg.counterexamples,
            });
        }
    }
    Ok(LoadedSchema {
        schema: root,
        relations: vec![],
        counterexamples: vec![],
    })
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
    let loaded = load_schema_full(schema_path)?;
    let config = guard::Config {
        strict,
        verbose,
        ..Default::default()
    };
    match guard::check(&raw, &loaded.schema, &config) {
        Ok(result) => {
            let rel_violations =
                rupia_core::ave::validate_relations(&result.value, &loaded.relations);
            if rel_violations.is_empty() {
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
            } else {
                let rel_errors: Vec<serde_json::Value> = rel_violations
                    .iter()
                    .map(|v| {
                        serde_json::json!({
                            "code": "RUPIA-REL001",
                            "message": v.description,
                            "help": format!("{} {} {} violated", v.field_a, v.operator, v.field_b),
                        })
                    })
                    .collect();
                let output = serde_json::json!({
                    "status": "invalid",
                    "error_count": rel_violations.len(),
                    "feedback": "Cross-field relation constraints violated",
                    "errors": rel_errors,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
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

fn parse_model_tier(s: &str) -> Result<rupia_core::ave::ModelTier> {
    match s.to_lowercase().as_str() {
        "haiku" => Ok(rupia_core::ave::ModelTier::Haiku),
        "sonnet" => Ok(rupia_core::ave::ModelTier::Sonnet),
        "opus" => Ok(rupia_core::ave::ModelTier::Opus),
        _ => bail!("unknown model tier '{s}': expected haiku, sonnet, or opus"),
    }
}

fn cmd_ave(
    domain: Option<String>,
    schema_path: Option<&PathBuf>,
    input: Option<PathBuf>,
    model: &str,
    json_output: bool,
) -> Result<()> {
    let tier = parse_model_tier(model)?;
    let loaded = if let Some(p) = schema_path {
        load_schema_full(p)?
    } else {
        let domain = domain.context("either --domain or --schema is required")?;
        let prompt = rupia_core::ave::generate_schema_prompt(&domain);
        if !json_output {
            println!("--- Schema Generation Prompt ---\n{prompt}\n---");
            println!("Provide the LLM response as input (stdin or --input):");
        }
        let raw = read_input(input.clone())?;
        let config = rupia_core::ave::AveConfig {
            domain,
            model_tier: tier,
            ..Default::default()
        };
        let pkg = rupia_core::ave::schema_resolve(&raw, &config).map_err(|e| anyhow::anyhow!(e))?;
        let out = serde_json::json!({
            "schema": pkg.schema,
            "relations": pkg.relations.iter().map(|r| serde_json::json!({
                "field_a": r.field_a,
                "operator": r.operator,
                "field_b": r.field_b,
            })).collect::<Vec<_>>(),
            "summary": pkg.summary,
            "counterexamples_count": pkg.counterexamples.len(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    };
    let raw = read_input(input)?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).or_else(|_| match rupia_core::lenient::parse(&raw) {
            ParseResult::Success(v) => Ok(v),
            ParseResult::Failure { errors, .. } => {
                bail!("parse failed with {} error(s)", errors.len())
            }
        })?;
    let results =
        rupia_core::ave::validate_with_confidence(&parsed, &loaded.schema, &loaded.relations);
    if json_output {
        let valid = results.iter().all(|r| {
            r.status == rupia_core::ave::FieldStatus::Valid
                || r.status == rupia_core::ave::FieldStatus::Coerced
        });
        let output = serde_json::json!({
            "status": if valid { "valid" } else { "invalid" },
            "fields": results.iter().map(|r| serde_json::json!({
                "field": r.field,
                "status": format!("{:?}", r.status),
                "confidence": r.confidence,
                "coercion": r.coercion,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let output: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "field": r.field,
                    "status": format!("{:?}", r.status),
                    "confidence": r.confidence,
                    "coercion": r.coercion,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    }
    Ok(())
}

fn cmd_boundary_gen(schema_path: &PathBuf) -> Result<()> {
    let schema = load_schema(schema_path)?;
    let cases = rupia_core::boundary::generate_boundary_cases(&schema);
    let output: Vec<serde_json::Value> = cases
        .iter()
        .map(|c| {
            serde_json::json!({
                "field": c.field,
                "value": c.value,
                "description": c.description,
                "expected_valid": c.expected_valid,
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn cmd_lint_schema(schema_path: &std::path::Path, verify_counterexamples: bool) -> Result<()> {
    let path_str = schema_path.to_string_lossy();
    let mut diags = guard::check_schema_file(&path_str);
    let loaded = load_schema_full(&schema_path.to_path_buf())?;
    if verify_counterexamples || !loaded.counterexamples.is_empty() {
        let ce_warnings = check_counterexamples(&loaded.schema, &loaded.counterexamples);
        for w in &ce_warnings {
            diags.push(rupia_core::diagnostic::Diagnostic {
                severity: rupia_core::diagnostic::Severity::Warning,
                code: "RUPIA-CE001",
                message: w.clone(),
                help: "Schema may be too loose — this counterexample should be rejected"
                    .to_string(),
                context: None,
            });
        }
    }
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

fn check_counterexamples(
    schema: &serde_json::Value,
    counterexamples: &[serde_json::Value],
) -> Vec<String> {
    let mut warnings = Vec::new();
    let config = guard::Config::default();
    for (i, ce) in counterexamples.iter().enumerate() {
        let Ok(ce_str) = serde_json::to_string(ce) else {
            continue;
        };
        if guard::check(&ce_str, schema, &config).is_ok() {
            warnings.push(format!(
                "Counterexample {} passed validation — schema may be too loose",
                i + 1
            ));
        }
    }
    warnings
}
