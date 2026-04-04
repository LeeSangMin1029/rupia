use std::fmt;

use crate::types::{ParseError, ValidationError, ValidationFailure};

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub help: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        write!(f, "[{icon}][{}] {}", self.code, self.message)?;
        if let Some(ctx) = &self.context {
            write!(f, "\n  context: {ctx}")?;
        }
        write!(f, "\n  help: {}", self.help)
    }
}

impl Diagnostic {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "severity": format!("{:?}", self.severity).to_lowercase(),
            "code": self.code,
            "message": self.message,
            "help": self.help,
            "context": self.context,
        })
    }
}

pub fn diagnose_parse_errors(errors: &[ParseError], raw_input: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for e in errors {
        let diag = match (e.path.as_str(), e.expected.as_str()) {
            ("$input", "JSON value") if raw_input.trim().is_empty() => Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-P001",
                message: "LLM returned empty output".into(),
                help: "Check that the LLM call succeeded and returned content. \
                       Verify your API key, model availability, and prompt. \
                       If using streaming, ensure the full response was collected."
                    .into(),
                context: None,
            },
            ("$input", "JSON value") => Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-P002",
                message: "LLM output contains no recognizable JSON".into(),
                help: format!(
                    "The output was not JSON and no JSON object/array was found.\n\
                     First 100 chars: \"{}\"\n\
                     Ensure your prompt asks for JSON output explicitly.\n\
                     Try adding: \"Respond with a JSON object only, no explanation.\"",
                    &raw_input[..raw_input.len().min(100)]
                ),
                context: e.description.clone(),
            },
            ("$input", "input within 16MB") => Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-P003",
                message: format!("Input exceeds size limit ({} bytes)", raw_input.len()),
                help: "The LLM output is too large. This is likely a runaway generation.\n\
                       Set max_tokens in your LLM call to limit output size.\n\
                       If you need larger outputs, configure: \
                       rupia::guard::Config { max_input_bytes: <your_limit> }"
                    .into(),
                context: None,
            },
            (path, expected) if expected.contains("':'") => Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-P004",
                message: format!("Malformed JSON at {path}: missing colon after key"),
                help: "The LLM produced malformed JSON with a missing ':' separator.\n\
                       This usually happens with weaker models (Haiku, GPT-3.5).\n\
                       Add to your prompt: \"Use standard JSON format with quoted keys and colons.\""
                    .into(),
                context: e.description.clone(),
            },
            (path, "string key") => Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-P005",
                message: format!("Invalid object key at {path}"),
                help: "The parser found an unexpected character where a key was expected.\n\
                       The JSON object might be truncated or contain binary data.\n\
                       Check that the LLM response was fully received."
                    .into(),
                context: e.description.clone(),
            },
            (path, expected) if expected.contains("max depth") => Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-P006",
                message: format!("Nesting depth exceeded at {path}"),
                help: "The JSON structure is nested too deeply (>512 levels).\n\
                       This is likely a recursive/malicious output.\n\
                       Simplify your schema to reduce nesting, or check for LLM loops."
                    .into(),
                context: None,
            },
            (path, _) => Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-P000",
                message: format!("Parse error at {path}: expected {}", e.expected),
                help: "The LLM output could not be parsed at this location.\n\
                       Run with --verbose to see the full input.\n\
                       If this persists, try a different model or simplify the schema."
                    .into(),
                context: e.description.clone(),
            },
        };
        diags.push(diag);
    }
    diags
}

pub fn diagnose_validation_failure(failure: &ValidationFailure) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for e in &failure.errors {
        let diag = categorize_validation_error(e);
        diags.push(diag);
    }
    if failure.errors.len() > 5 {
        diags.push(Diagnostic {
            severity: Severity::Warning,
            code: "RUPIA-V100",
            message: format!("{} validation errors — consider simplifying the schema", failure.errors.len()),
            help: "Many errors suggest the LLM fundamentally misunderstood the schema.\n\
                   Try: (1) Split into smaller schemas, (2) Add examples in the prompt,\n\
                   (3) Use a stronger model for the first attempt."
                .into(),
            context: None,
        });
    }
    diags
}

#[expect(clippy::too_many_lines, reason = "dispatch table, splitting would reduce readability")]
fn categorize_validation_error(e: &ValidationError) -> Diagnostic {
    if e.expected.contains("Format<") {
        let format = e
            .expected
            .split("Format<\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("unknown");
        return Diagnostic {
            severity: Severity::Error,
            code: "RUPIA-V001",
            message: format!("{}: format violation ({})", e.path, format),
            help: format!(
                "Value \"{}\" does not match format \"{format}\".\n\
                 Add to prompt: \"The {field} field must be a valid {format}.\"\n\
                 For email: must contain @ and domain. For URI: must start with http(s)://.",
                e.value,
                field = e.path.rsplit('.').next().unwrap_or(&e.path)
            ),
            context: None,
        };
    }
    if e.expected.contains("Minimum<") || e.expected.contains("Maximum<") {
        return Diagnostic {
            severity: Severity::Error,
            code: "RUPIA-V002",
            message: format!("{}: range violation", e.path),
            help: format!(
                "Value {} is outside allowed range ({}).\n\
                 Add to prompt: \"The {field} must be {expected}.\"",
                e.value,
                e.expected,
                field = e.path.rsplit('.').next().unwrap_or(&e.path),
                expected = e.expected
            ),
            context: None,
        };
    }
    if e.expected.contains("one of") || e.expected.contains("enum") {
        return Diagnostic {
            severity: Severity::Error,
            code: "RUPIA-V003",
            message: format!("{}: invalid enum value", e.path),
            help: format!(
                "Value {} is not in the allowed set: {}.\n\
                 Add to prompt: \"The {field} must be one of: {expected}.\"",
                e.value,
                e.expected,
                field = e.path.rsplit('.').next().unwrap_or(&e.path),
                expected = e.expected
            ),
            context: None,
        };
    }
    if e.description.as_ref().is_some_and(|d| d.contains("undefined")) {
        return Diagnostic {
            severity: Severity::Error,
            code: "RUPIA-V004",
            message: format!("{}: required property missing", e.path),
            help: format!(
                "The field \"{}\" is required but was not provided.\n\
                 Add to prompt: \"You MUST include the {field} field.\"",
                e.path,
                field = e.path.rsplit('.').next().unwrap_or(&e.path)
            ),
            context: e.description.clone(),
        };
    }
    let actual_type = match &e.value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    };
    if actual_type != e.expected {
        return Diagnostic {
            severity: Severity::Error,
            code: "RUPIA-V005",
            message: format!("{}: type mismatch (got {actual_type}, expected {})", e.path, e.expected),
            help: format!(
                "The LLM returned {actual_type} but the schema expects {}.\n\
                 This should have been auto-coerced. If it persists:\n\
                 (1) Check if the schema type is correct\n\
                 (2) The value \"{}\" might not be coercible to {}",
                e.expected, e.value, e.expected
            ),
            context: None,
        };
    }
    Diagnostic {
        severity: Severity::Error,
        code: "RUPIA-V000",
        message: format!("{}: {}", e.path, e.expected),
        help: format!(
            "Validation failed. Expected: {}\nGot: {}\n\
             Check your schema definition and the LLM prompt.",
            e.expected, e.value
        ),
        context: e.description.clone(),
    }
}

pub fn diagnose_schema_file(schema_path: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let content = match std::fs::read_to_string(schema_path) {
        Ok(c) => c,
        Err(e) => {
            diags.push(Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-S001",
                message: format!("Cannot read schema file: {schema_path}"),
                help: format!(
                    "Error: {e}\n\
                     Ensure the schema file exists and is readable.\n\
                     For Rust: cargo run --example generate_schema > schema.json\n\
                     For Go: go run github.com/invopop/jsonschema/cmd/jsonschema -type MyType > schema.json"
                ),
                context: None,
            });
            return diags;
        }
    };
    let schema: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            diags.push(Diagnostic {
                severity: Severity::Error,
                code: "RUPIA-S002",
                message: "Schema file is not valid JSON".into(),
                help: format!(
                    "Parse error: {e}\n\
                     Regenerate the schema from your type definitions.\n\
                     Do not manually edit generated schema files."
                ),
                context: Some(format!("file: {schema_path}")),
            });
            return diags;
        }
    };
    if schema.get("type").is_none()
        && schema.get("anyOf").is_none()
        && schema.get("oneOf").is_none()
        && schema.get("$ref").is_none()
    {
        diags.push(Diagnostic {
            severity: Severity::Warning,
            code: "RUPIA-S003",
            message: "Schema has no top-level type constraint".into(),
            help: "The schema does not define a \"type\" field at the root.\n\
                   This means any JSON value will pass validation.\n\
                   Add \"type\": \"object\" (or array/string/etc) to constrain."
                .into(),
            context: None,
        });
    }
    if let Some(obj) = schema.as_object() {
        if obj.get("properties").is_some() && obj.get("required").is_none() {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                code: "RUPIA-S004",
                message: "Schema has properties but no \"required\" array".into(),
                help: "Without \"required\", all properties are optional.\n\
                       LLMs may omit fields. Add a \"required\" array with essential field names."
                    .into(),
                context: None,
            });
        }
    }
    diags
}

pub fn format_diagnostics(diags: &[Diagnostic]) -> String {
    if diags.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for (i, d) in diags.iter().enumerate() {
        if i > 0 {
            output.push('\n');
        }
        output.push_str(&d.to_string());
        output.push('\n');
    }
    output
}

pub fn format_diagnostics_json(diags: &[Diagnostic]) -> serde_json::Value {
    serde_json::Value::Array(diags.iter().map(Diagnostic::to_json).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diagnose_empty_input() {
        let errors = vec![ParseError {
            path: "$input".into(),
            expected: "JSON value".into(),
            description: Some("empty input".into()),
        }];
        let diags = diagnose_parse_errors(&errors, "");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "RUPIA-P001");
        assert!(diags[0].help.contains("API key"));
    }

    #[test]
    fn diagnose_no_json() {
        let errors = vec![ParseError {
            path: "$input".into(),
            expected: "JSON value".into(),
            description: Some("some text".into()),
        }];
        let diags = diagnose_parse_errors(&errors, "Hello world, no JSON here");
        assert_eq!(diags[0].code, "RUPIA-P002");
        assert!(diags[0].help.contains("First 100 chars"));
    }

    #[test]
    fn diagnose_format_violation() {
        let failure = ValidationFailure {
            data: json!({"email": "bad"}),
            errors: vec![ValidationError {
                path: "$input.email".into(),
                expected: "string & Format<\"email\">".into(),
                value: json!("bad"),
                description: None,
            }],
        };
        let diags = diagnose_validation_failure(&failure);
        assert_eq!(diags[0].code, "RUPIA-V001");
        assert!(diags[0].help.contains('@'));
    }

    #[test]
    fn diagnose_missing_required() {
        let failure = ValidationFailure {
            data: json!({}),
            errors: vec![ValidationError {
                path: "$input.name".into(),
                expected: "string".into(),
                value: json!(null),
                description: Some("The value at this path is `undefined`.".into()),
            }],
        };
        let diags = diagnose_validation_failure(&failure);
        assert_eq!(diags[0].code, "RUPIA-V004");
    }

    #[test]
    fn diagnose_many_errors_warns() {
        let failure = ValidationFailure {
            data: json!({}),
            errors: (0..8)
                .map(|i| ValidationError {
                    path: format!("$input.field_{i}"),
                    expected: "string".into(),
                    value: json!(null),
                    description: Some("The value at this path is `undefined`.".into()),
                })
                .collect(),
        };
        let diags = diagnose_validation_failure(&failure);
        assert!(diags.iter().any(|d| d.code == "RUPIA-V100"));
    }

    #[test]
    fn diagnostics_json_format() {
        let diags = vec![Diagnostic {
            severity: Severity::Error,
            code: "RUPIA-T001",
            message: "test".into(),
            help: "fix it".into(),
            context: None,
        }];
        let json = format_diagnostics_json(&diags);
        assert!(json[0]["code"].as_str() == Some("RUPIA-T001"));
    }
}
