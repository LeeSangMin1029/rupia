use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub path: String,
    pub expected: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: expected {}, got {}",
            self.path, self.expected, self.value
        )
    }
}

#[derive(Debug, Clone)]
pub enum Validation<T> {
    Success(T),
    Failure(ValidationFailure),
}

impl<T> Validation<T> {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    pub fn into_result(self) -> Result<T, ValidationFailure> {
        match self {
            Self::Success(data) => Ok(data),
            Self::Failure(f) => Err(f),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidationFailure {
    pub data: serde_json::Value,
    pub errors: Vec<ValidationError>,
}

impl ValidationFailure {
    pub fn to_llm_feedback(&self) -> String {
        crate::feedback::stringify(self)
    }

    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    pub fn errors_by_path(&self) -> HashMap<String, Vec<&ValidationError>> {
        let mut map: HashMap<String, Vec<&ValidationError>> = HashMap::new();
        for e in &self.errors {
            map.entry(e.path.clone()).or_default().push(e);
        }
        map
    }
}

impl fmt::Display for ValidationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.errors {
            writeln!(f, "  ❌ {e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationFailure {}

#[derive(Debug, Clone)]
pub enum ParseResult<T> {
    Success(T),
    Failure {
        data: Option<serde_json::Value>,
        input: String,
        errors: Vec<ParseError>,
    },
}

impl<T> ParseResult<T> {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub path: String,
    pub expected: String,
    pub description: Option<String>,
}

pub trait HasSchema {
    fn json_schema() -> serde_json::Value;
}

pub struct HarnessConfig {
    pub max_retries: u32,
    pub timeout_ms: Option<u64>,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            max_retries: 10,
            timeout_ms: None,
        }
    }
}
