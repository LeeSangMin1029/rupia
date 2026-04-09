pub use rupia_core::coerce::coerce_with_schema;
pub use rupia_core::diagnostic::{self, Diagnostic, Severity};
pub use rupia_core::feedback::stringify as feedback_stringify;
pub use rupia_core::format;
pub use rupia_core::guard::{self, Config as GuardConfig, GuardError, GuardResult};
pub use rupia_core::harness::{self, HarnessResult, run as harness_loop};
pub use rupia_core::lenient::parse as lenient_parse;
pub use rupia_core::llm::{LlmApplication, LlmController, LlmFunction};
pub use rupia_core::random;
pub use rupia_core::schema_ops;
pub use rupia_core::sync;
pub use rupia_core::types::{
    HarnessConfig, HasSchema, ParseError, ParseResult, Validation, ValidationError,
    ValidationFailure,
};
pub use rupia_core::validator::{validate, validate_strict};
pub use rupia_derive::Harness;

pub fn parse_validate(raw: &str, schema: &serde_json::Value) -> Validation<serde_json::Value> {
    let parsed = match lenient_parse(raw) {
        ParseResult::Success(v) => v,
        ParseResult::Failure { .. } => {
            return Validation::Failure(ValidationFailure {
                data: serde_json::Value::Null,
                errors: vec![ValidationError {
                    path: "$input".into(),
                    expected: "valid JSON".into(),
                    value: serde_json::Value::Null,
                    description: Some("failed to parse input as JSON".into()),
                }],
            });
        }
    };
    let coerced = coerce_with_schema(parsed, schema);
    validate(&coerced, schema)
}

pub fn parse_validate_typed<T: HasSchema + serde::de::DeserializeOwned>(
    raw: &str,
) -> Result<T, ValidationFailure> {
    let schema = T::rupia_schema();
    match parse_validate(raw, &schema) {
        Validation::Success(v) => serde_json::from_value(v).map_err(|e| ValidationFailure {
            data: serde_json::Value::Null,
            errors: vec![ValidationError {
                path: "$input".into(),
                expected: std::any::type_name::<T>().into(),
                value: serde_json::Value::Null,
                description: Some(e.to_string()),
            }],
        }),
        Validation::Failure(f) => Err(f),
    }
}
