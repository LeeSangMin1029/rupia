use std::future::Future;

use serde_json::Value;

use crate::coerce::coerce_with_schema;
use crate::feedback::stringify;
use crate::lenient;
use crate::types::{HarnessConfig, ParseResult, Validation, ValidationFailure};
use crate::validator::validate;

pub struct HarnessResult {
    pub value: Value,
    pub attempts: u32,
    pub errors_per_attempt: Vec<usize>,
}

pub async fn run<F, Fut>(
    schema: &Value,
    llm_fn: F,
    config: HarnessConfig,
) -> Result<HarnessResult, ValidationFailure>
where
    F: Fn(Option<&str>) -> Fut,
    Fut: Future<Output = String>,
{
    let mut feedback: Option<String> = None;
    let mut errors_per_attempt = Vec::new();
    for attempt in 0..=config.max_retries {
        let raw = llm_fn(feedback.as_deref()).await;
        let parsed = match lenient::parse(&raw) {
            ParseResult::Success(v) => v,
            ParseResult::Failure { .. } => {
                errors_per_attempt.push(1);
                feedback = Some(format!(
                    "JSON parse failed. Please return valid JSON matching the schema.\n\nYour output:\n```\n{raw}\n```"
                ));
                continue;
            }
        };
        let coerced = coerce_with_schema(parsed, schema);
        match validate(&coerced, schema) {
            Validation::Success(data) => {
                errors_per_attempt.push(0);
                return Ok(HarnessResult {
                    value: data,
                    attempts: attempt + 1,
                    errors_per_attempt,
                });
            }
            Validation::Failure(f) => {
                let error_count = f.error_count();
                errors_per_attempt.push(error_count);
                feedback = Some(stringify(&f));
            }
        }
    }
    let raw = llm_fn(feedback.as_deref()).await;
    if let ParseResult::Success(parsed) = lenient::parse(&raw) {
        let coerced = coerce_with_schema(parsed, schema);
        if let Validation::Success(data) = validate(&coerced, schema) {
            errors_per_attempt.push(0);
            return Ok(HarnessResult {
                value: data,
                attempts: config.max_retries + 2,
                errors_per_attempt,
            });
        }
    }
    Err(ValidationFailure {
        data: Value::Null,
        errors: vec![crate::types::ValidationError {
            path: "$input".into(),
            expected: "valid output after retries".into(),
            value: Value::Null,
            description: Some(format!(
                "Failed to converge after {} attempts",
                config.max_retries + 1
            )),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn converges_first_try() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        });
        let result = run(
            &schema,
            |_feedback| async { r#"{"name": "test"}"#.to_owned() },
            HarnessConfig::default(),
        )
        .await
        .unwrap();
        assert_eq!(result.value["name"], "test");
        assert_eq!(result.attempts, 1);
    }

    #[tokio::test]
    async fn converges_after_retry() {
        let attempt = AtomicU32::new(0);
        let schema = json!({
            "type": "object",
            "properties": {"age": {"type": "number", "minimum": 0}},
            "required": ["age"]
        });
        let result = run(
            &schema,
            |feedback| {
                let n = attempt.fetch_add(1, Ordering::SeqCst);
                let has_feedback = feedback.is_some();
                async move {
                    if n == 0 {
                        assert!(!has_feedback);
                        r#"{"age": -5}"#.to_owned()
                    } else {
                        assert!(has_feedback);
                        r#"{"age": 25}"#.to_owned()
                    }
                }
            },
            HarnessConfig {
                max_retries: 3,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(result.value["age"], 25);
        assert!(result.attempts > 1);
    }
}
