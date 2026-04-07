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
    /// Cross-reference schemas across public APIs or local files
    CrossRef {
        /// Domain keyword (e.g., "payment", "ecommerce")
        #[arg(short, long)]
        domain: Option<String>,
        /// Local schema files to compare
        #[arg(short, long, num_args = 1..)]
        schemas: Vec<PathBuf>,
        /// Entity name hint (e.g., "order", "payment")
        #[arg(short, long)]
        entity: Option<String>,
        /// Compare result against app schema
        #[arg(short, long)]
        compare: Option<PathBuf>,
        /// Max APIs to analyze
        #[arg(long, default_value = "5")]
        max_apis: usize,
        /// JSON output
        #[arg(long)]
        json: bool,
        /// Sync: download all domain APIs locally for offline use
        #[arg(long)]
        sync: bool,
        /// Diff: detect changes since last sync
        #[arg(long)]
        diff: bool,
    },
    /// Watch multiple domains for API changes (sync + diff)
    Watch {
        /// Config file (.rupia-watch.json)
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Domains to watch (alternative to config file)
        #[arg(short, long, num_args = 1..)]
        domains: Vec<String>,
        /// Max APIs per domain
        #[arg(long, default_value = "5")]
        max_apis: usize,
        /// Sync before diff (first run)
        #[arg(long)]
        sync_first: bool,
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
        Command::CrossRef {
            domain,
            schemas,
            entity,
            compare,
            max_apis,
            json,
            sync,
            diff,
        } => cmd_cross_ref(&CrossRefArgs {
            domain,
            schemas,
            entity,
            compare,
            max_apis,
            json_output: json,
            sync,
            diff,
        }),
        Command::Watch {
            config,
            domains,
            max_apis,
            sync_first,
        } => cmd_watch(config.as_ref(), &domains, max_apis, sync_first),
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
    rules: Vec<rupia_core::ave::JsonLogicRule>,
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
                rules: pkg.rules,
                counterexamples: pkg.counterexamples,
            });
        }
    }
    Ok(LoadedSchema {
        schema: root,
        relations: vec![],
        rules: vec![],
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
            let rule_violations = rupia_core::ave::validate_rules(&result.value, &loaded.rules);
            let ce_warnings = check_counterexamples(&loaded.schema, &loaded.counterexamples);
            if rel_violations.is_empty() && rule_violations.is_empty() {
                let mut output = if json_output {
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
                if !ce_warnings.is_empty() {
                    output["schema_warnings"] = serde_json::json!(ce_warnings);
                }
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                let mut errors: Vec<serde_json::Value> = rel_violations
                    .iter()
                    .map(|v| {
                        serde_json::json!({
                            "code": "RUPIA-REL001",
                            "message": v.description,
                            "help": format!("{} {} {} violated", v.field_a, v.operator, v.field_b),
                        })
                    })
                    .collect();
                for rv in &rule_violations {
                    errors.push(serde_json::json!({
                        "code": "RUPIA-RULE001",
                        "message": rv.description,
                        "help": "JSONLogic rule violated",
                    }));
                }
                let output = serde_json::json!({
                    "status": "invalid",
                    "error_count": errors.len(),
                    "feedback": "Cross-field constraints violated",
                    "errors": errors,
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
        let ce_warnings = check_counterexamples(&pkg.schema, &pkg.counterexamples);
        let out = serde_json::json!({
            "schema": pkg.schema,
            "relations": pkg.relations.iter().map(|r| serde_json::json!({
                "field_a": r.field_a,
                "operator": r.operator,
                "field_b": r.field_b,
            })).collect::<Vec<_>>(),
            "rules": pkg.rules.iter().map(|r| serde_json::json!({
                "description": r.description,
                "logic": r.logic,
            })).collect::<Vec<_>>(),
            "summary": pkg.summary,
            "counterexamples_count": pkg.counterexamples.len(),
            "schema_warnings": ce_warnings,
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
    let loaded = load_schema_full(&schema_path.to_path_buf())?;
    let mut diags = if loaded.relations.is_empty()
        && loaded.rules.is_empty()
        && loaded.counterexamples.is_empty()
    {
        guard::check_schema_file(&path_str)
    } else {
        rupia_core::diagnostic::diagnose_schema_value(&loaded.schema)
    };
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

struct CrossRefArgs {
    domain: Option<String>,
    schemas: Vec<PathBuf>,
    entity: Option<String>,
    compare: Option<PathBuf>,
    max_apis: usize,
    json_output: bool,
    sync: bool,
    diff: bool,
}

fn cmd_cross_ref(args: &CrossRefArgs) -> Result<()> {
    if args.sync {
        let d = args.domain.as_deref().context("--sync requires --domain")?;
        let manifest =
            rupia_core::sync::sync_domain(d, args.max_apis).map_err(|e| anyhow::anyhow!("{e}"))?;
        let output = serde_json::json!({
            "action": "sync",
            "domain": d,
            "apis_synced": manifest.apis.len(),
            "synced_at": manifest.last_sync,
            "apis": manifest.apis.keys().collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }
    if args.diff {
        let d = args.domain.as_deref().context("--diff requires --domain")?;
        let report = rupia_core::sync::detect_changes(d, args.max_apis)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let output = serde_json::to_value(&report)?;
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }
    if args.domain.is_none() && args.schemas.is_empty() {
        bail!("either --domain or --schemas is required");
    }
    let report = build_cross_ref(
        args.domain.as_deref(),
        &args.schemas,
        args.entity.as_deref(),
        args.max_apis,
    )?;
    if let Some(ref cmp_path) = args.compare {
        return print_compare(
            &report,
            cmp_path,
            args.domain.as_deref(),
            &args.schemas,
            args.max_apis,
        );
    }
    print_cross_ref(&report, args.domain.as_deref(), args.json_output);
    Ok(())
}

struct CrossRefData {
    enums: Vec<rupia_core::schema_ops::UniversalEnum>,
    constraints: Vec<rupia_core::schema_ops::UniversalConstraint>,
    divergences: Vec<rupia_core::schema_ops::Divergence>,
    apis_analyzed: Vec<String>,
    schemas_found: usize,
}

fn build_cross_ref(
    domain: Option<&str>,
    schemas: &[PathBuf],
    entity: Option<&str>,
    max_apis: usize,
) -> Result<CrossRefData> {
    if let Some(d) = domain {
        let report = rupia_core::fetch::cross_ref_by_domain(d, entity, max_apis)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(CrossRefData {
            enums: report.universal_enums,
            constraints: report.universal_constraints,
            divergences: report.divergences,
            apis_analyzed: report.apis_analyzed,
            schemas_found: report.schemas_found,
        })
    } else {
        let mut loaded: Vec<serde_json::Value> = Vec::new();
        for p in schemas {
            loaded.push(load_schema(p)?);
        }
        let result = rupia_core::schema_ops::cross_reference_schemas(&loaded);
        Ok(CrossRefData {
            enums: result.universal_enums,
            constraints: result.universal_constraints,
            divergences: result.divergences,
            apis_analyzed: schemas.iter().map(|p| p.display().to_string()).collect(),
            schemas_found: loaded.len(),
        })
    }
}

fn print_compare(
    _report: &CrossRefData,
    cmp_path: &PathBuf,
    domain: Option<&str>,
    schemas: &[PathBuf],
    max_apis: usize,
) -> Result<()> {
    let app_schema = load_schema(cmp_path)?;
    let mut all = vec![app_schema];
    if let Some(d) = domain {
        let list = rupia_core::fetch::fetch_api_list().map_err(|e| anyhow::anyhow!("{e}"))?;
        let apis = rupia_core::fetch::search_apis(&list, d, max_apis);
        for api in &apis {
            if api.openapi_url.is_empty() {
                continue;
            }
            if let Ok(spec) = rupia_core::fetch::fetch_spec(&api.openapi_url, &api.name) {
                all.push(spec);
            }
        }
    }
    for p in schemas {
        all.push(load_schema(p)?);
    }
    let cmp_result = rupia_core::schema_ops::cross_reference_schemas(&all);
    let output = serde_json::json!({
        "compare_source": cmp_path.display().to_string(),
        "divergences": cmp_result.divergences.iter().map(|d| serde_json::json!({
            "field": d.field_pattern,
            "description": d.description,
        })).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_cross_ref(data: &CrossRefData, domain: Option<&str>, json_output: bool) {
    if json_output {
        let output = serde_json::json!({
            "domain": domain,
            "apis_analyzed": data.apis_analyzed,
            "schemas_found": data.schemas_found,
            "universal_enums": data.enums.iter().map(|e| serde_json::json!({
                "field": e.field_pattern,
                "common_values": e.common_values,
                "all_values": e.all_values,
                "source_count": e.source_count,
            })).collect::<Vec<_>>(),
            "universal_constraints": data.constraints.iter().map(|c| serde_json::json!({
                "field": c.field_pattern,
                "constraint_type": c.constraint_type,
                "value": c.value,
                "agreement": format!("{}/{}", c.agreement, c.total),
            })).collect::<Vec<_>>(),
            "divergences": data.divergences.iter().map(|d| serde_json::json!({
                "field": d.field_pattern,
                "description": d.description,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("=== Cross-Reference Report ===");
        if let Some(d) = domain {
            println!("Domain: {d}");
        }
        println!(
            "APIs analyzed: {} | Schemas found: {}",
            data.apis_analyzed.len(),
            data.schemas_found
        );
        for a in &data.apis_analyzed {
            println!("  - {a}");
        }
        if !data.enums.is_empty() {
            println!("\n--- Universal Enums ---");
            for e in &data.enums {
                println!(
                    "  {}: {:?} (from {} sources)",
                    e.field_pattern, e.common_values, e.source_count
                );
            }
        }
        if !data.constraints.is_empty() {
            println!("\n--- Universal Constraints ---");
            for c in &data.constraints {
                println!(
                    "  {} [{}]: {} ({}/{})",
                    c.field_pattern, c.constraint_type, c.value, c.agreement, c.total
                );
            }
        }
        if !data.divergences.is_empty() {
            println!("\n--- Divergences ---");
            for d in &data.divergences {
                println!("  {}: {}", d.field_pattern, d.description);
            }
        }
    }
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

fn parse_watch_config(path: &std::path::Path) -> Result<(Vec<String>, usize)> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading watch config {}", path.display()))?;
    let val: serde_json::Value = serde_json::from_str(&content).context("parsing watch config")?;
    let domains = val
        .get("domains")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let max_apis = val
        .get("max_apis")
        .and_then(serde_json::Value::as_u64)
        .map_or(5, |v| usize::try_from(v).unwrap_or(5));
    Ok((domains, max_apis))
}

fn cmd_watch(
    config_path: Option<&PathBuf>,
    cli_domains: &[String],
    max_apis: usize,
    sync_first: bool,
) -> Result<()> {
    let (domains, resolved_max) = if let Some(path) = config_path {
        parse_watch_config(path)?
    } else if !cli_domains.is_empty() {
        (cli_domains.to_vec(), max_apis)
    } else {
        bail!("either --config or --domains is required");
    };
    if domains.is_empty() {
        bail!("no domains to watch");
    }
    let mut results = Vec::new();
    let mut total_breaking = 0usize;
    for domain in &domains {
        if sync_first {
            match rupia_core::sync::sync_domain(domain, resolved_max) {
                Ok(m) => {
                    eprintln!("[sync] {domain} — {} APIs synced", m.apis.len());
                }
                Err(e) => {
                    eprintln!("[sync] {domain} — error: {e}");
                    results.push(serde_json::json!({
                        "domain": domain,
                        "status": "sync_error",
                        "error": e,
                    }));
                    continue;
                }
            }
        }
        match rupia_core::sync::detect_changes(domain, resolved_max) {
            Ok(report) => {
                let breaking = report.summary.breaking_changes;
                total_breaking += breaking;
                results.push(serde_json::json!({
                    "domain": domain,
                    "status": "ok",
                    "total_apis": report.summary.total_apis,
                    "new_apis": report.summary.new_apis,
                    "updated_apis": report.summary.updated_apis,
                    "removed_apis": report.summary.removed_apis,
                    "breaking_changes": breaking,
                    "changes": report.changes.iter().filter(|c| {
                        !matches!(c.change_type, rupia_core::sync::ChangeType::Unchanged)
                    }).map(|c| serde_json::json!({
                        "api": c.api_name,
                        "type": format!("{:?}", c.change_type),
                    })).collect::<Vec<_>>(),
                }));
            }
            Err(e) => {
                if e.contains("no previous sync") {
                    eprintln!("[watch] {domain} — no sync yet, run with --sync-first");
                    results.push(serde_json::json!({
                        "domain": domain,
                        "status": "no_sync",
                        "hint": "run with --sync-first",
                    }));
                } else {
                    eprintln!("[watch] {domain} — error: {e}");
                    results.push(serde_json::json!({
                        "domain": domain,
                        "status": "error",
                        "error": e,
                    }));
                }
            }
        }
    }
    let output = serde_json::json!({
        "domains_checked": domains.len(),
        "total_breaking": total_breaking,
        "results": results,
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    if total_breaking > 0 {
        std::process::exit(1);
    }
    Ok(())
}
