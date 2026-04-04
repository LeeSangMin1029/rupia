use std::time::{Duration, Instant};

use crate::coerce::coerce_with_schema;
use crate::diagnostic::{
    Diagnostic, Severity, diagnose_parse_errors, diagnose_schema_file, diagnose_validation_failure,
    format_diagnostics,
};
use crate::lenient;
use crate::types::{ParseResult, Validation};
use crate::validator;

#[derive(Debug, Clone)]
pub struct Config {
    pub max_input_bytes: usize,
    pub max_retries: u32,
    pub timeout: Duration,
    pub strict: bool,
    pub verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_retries: 10,
            timeout: Duration::from_secs(30),
            strict: false,
            verbose: false,
        }
    }
}

#[derive(Debug)]
pub struct GuardResult {
    pub value: serde_json::Value,
    pub attempts: u32,
    pub total_duration: Duration,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub struct GuardError {
    pub diagnostics: Vec<Diagnostic>,
    pub attempts: u32,
    pub total_duration: Duration,
    pub last_feedback: Option<String>,
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rupia guard failed after {} attempts ({:.1}s)",
            self.attempts,
            self.total_duration.as_secs_f64()
        )?;
        if !self.diagnostics.is_empty() {
            write!(f, "\n{}", format_diagnostics(&self.diagnostics))?;
        }
        Ok(())
    }
}

impl std::error::Error for GuardError {}

pub fn check(
    raw: &str,
    schema: &serde_json::Value,
    config: &Config,
) -> Result<GuardResult, GuardError> {
    let start = Instant::now();
    let mut all_diags = Vec::new();
    if raw.len() > config.max_input_bytes {
        all_diags.push(Diagnostic {
            severity: Severity::Error,
            code: "RUPIA-G001",
            message: format!(
                "Input size {} exceeds limit {}",
                raw.len(),
                config.max_input_bytes
            ),
            help: format!(
                "Set max_tokens in your LLM call to limit output.\n\
                 Current limit: {} bytes. Override with Config {{ max_input_bytes: ... }}",
                config.max_input_bytes
            ),
            context: None,
        });
        return Err(GuardError {
            diagnostics: all_diags,
            attempts: 0,
            total_duration: start.elapsed(),
            last_feedback: None,
        });
    }
    let parsed = match lenient::parse(raw) {
        ParseResult::Success(v) => v,
        ParseResult::Failure { errors, .. } => {
            let parse_diags = diagnose_parse_errors(&errors, raw);
            all_diags.extend(parse_diags);
            return Err(GuardError {
                diagnostics: all_diags,
                attempts: 1,
                total_duration: start.elapsed(),
                last_feedback: None,
            });
        }
    };
    let coerced = coerce_with_schema(parsed, schema);
    let result = if config.strict {
        validator::validate_strict(&coerced, schema)
    } else {
        validator::validate(&coerced, schema)
    };
    match result {
        Validation::Success(data) => Ok(GuardResult {
            value: data,
            attempts: 1,
            total_duration: start.elapsed(),
            diagnostics: all_diags,
        }),
        Validation::Failure(f) => {
            let val_diags = diagnose_validation_failure(&f);
            all_diags.extend(val_diags);
            let feedback = f.to_llm_feedback();
            Err(GuardError {
                diagnostics: all_diags,
                attempts: 1,
                total_duration: start.elapsed(),
                last_feedback: Some(feedback),
            })
        }
    }
}

pub fn check_schema_file(path: &str) -> Vec<Diagnostic> {
    diagnose_schema_file(path)
}

pub async fn guarded_loop<F, Fut>(
    schema: &serde_json::Value,
    llm_fn: F,
    config: &Config,
) -> Result<GuardResult, GuardError>
where
    F: Fn(Option<&str>) -> Fut,
    Fut: std::future::Future<Output = String>,
{
    let start = Instant::now();
    let mut feedback: Option<String> = None;
    let mut all_diags: Vec<Diagnostic> = Vec::new();
    let mut attempts = 0u32;
    for _ in 0..=config.max_retries {
        if start.elapsed() > config.timeout {
            all_diags.push(Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-G002",
                message: format!("Timeout after {:.1}s", config.timeout.as_secs_f64()),
                help: "The self-healing loop did not converge within the timeout.\n\
                       Increase Config.timeout or reduce Config.max_retries.\n\
                       Consider using a stronger model or simplifying the schema."
                    .into(),
                context: Some(format!("attempts so far: {attempts}")),
            });
            break;
        }
        let raw = llm_fn(feedback.as_deref()).await;
        attempts += 1;
        match check(&raw, schema, config) {
            Ok(mut result) => {
                result.attempts = attempts;
                result.total_duration = start.elapsed();
                result.diagnostics = all_diags;
                return Ok(result);
            }
            Err(e) => {
                for d in &e.diagnostics {
                    if config.verbose {
                        eprintln!("{d}");
                    }
                }
                all_diags.extend(e.diagnostics);
                feedback = e.last_feedback;
            }
        }
    }
    if feedback.is_none() {
        all_diags.push(Diagnostic {
            severity: Severity::Error,
            code: "RUPIA-G003",
            message: format!("Failed to converge after {attempts} attempts"),
            help: "The LLM could not produce valid output matching the schema.\n\
                   Possible fixes:\n\
                   1. Simplify the schema (fewer required fields, simpler types)\n\
                   2. Use a stronger model (Opus/Sonnet instead of Haiku)\n\
                   3. Add examples to your prompt\n\
                   4. Increase max_retries (current limit may be too low)"
                .into(),
            context: None,
        });
    }
    Err(GuardError {
        diagnostics: all_diags,
        attempts,
        total_duration: start.elapsed(),
        last_feedback: feedback,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn check_valid() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        });
        let result = check(r#"{"name": "test"}"#, &schema, &Config::default()).unwrap();
        assert_eq!(result.value["name"], "test");
        assert_eq!(result.attempts, 1);
    }

    #[test]
    fn check_invalid_returns_diagnostics() {
        let schema = json!({
            "type": "object",
            "properties": {"age": {"type": "number", "minimum": 0}},
            "required": ["age"]
        });
        let err = check(r#"{"age": -5}"#, &schema, &Config::default()).unwrap_err();
        assert!(!err.diagnostics.is_empty());
        assert!(err.last_feedback.is_some());
        assert!(err.diagnostics.iter().any(|d| d.code == "RUPIA-V002"));
    }

    #[test]
    fn check_empty_input() {
        let schema = json!({"type": "object"});
        let err = check("", &schema, &Config::default()).unwrap_err();
        assert!(err.diagnostics.iter().any(|d| d.code == "RUPIA-P001"));
    }

    #[test]
    fn check_oversized() {
        let schema = json!({"type": "object"});
        let config = Config {
            max_input_bytes: 10,
            ..Default::default()
        };
        let err = check("a]repeating very long string", &schema, &config).unwrap_err();
        assert!(err.diagnostics.iter().any(|d| d.code == "RUPIA-G001"));
    }

    #[test]
    fn check_with_coercion() {
        let schema = json!({
            "type": "object",
            "properties": {"count": {"type": "number"}},
            "required": ["count"]
        });
        let result = check(r#"{"count": "42"}"#, &schema, &Config::default()).unwrap();
        assert_eq!(result.value["count"], 42);
    }

    #[test]
    fn check_malformed_recovers() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        });
        let raw = "Here is the result:\n```json\n{\"name\": \"test\"}\n```";
        let result = check(raw, &schema, &Config::default()).unwrap();
        assert_eq!(result.value["name"], "test");
    }

    #[tokio::test]
    async fn guarded_loop_converges() {
        let schema = json!({
            "type": "object",
            "properties": {"x": {"type": "number", "minimum": 0}},
            "required": ["x"]
        });
        let call = std::sync::atomic::AtomicU32::new(0);
        let result = guarded_loop(
            &schema,
            |fb| {
                let n = call.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let has_fb = fb.is_some();
                async move {
                    if n == 0 && !has_fb {
                        r#"{"x": -1}"#.to_owned()
                    } else {
                        r#"{"x": 42}"#.to_owned()
                    }
                }
            },
            &Config::default(),
        )
        .await
        .unwrap();
        assert_eq!(result.value["x"], 42);
        assert!(result.attempts >= 2);
    }

    #[tokio::test]
    async fn guarded_loop_exhausted() {
        let schema = json!({"type": "object", "properties": {"x": {"type": "number", "minimum": 999}}});
        let config = Config {
            max_retries: 1,
            ..Default::default()
        };
        let err = guarded_loop(
            &schema,
            |_| async { r#"{"x": 1}"#.to_owned() },
            &config,
        )
        .await
        .unwrap_err();
        assert!(err.attempts >= 2);
    }
}
