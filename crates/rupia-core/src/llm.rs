use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::coerce::coerce_with_schema;
use crate::lenient;
use crate::types::{ParseResult, Validation, ValidationError, ValidationFailure};
use crate::validator;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LlmFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl LlmFunction {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    pub fn parse(&self, input: &str) -> ParseResult<Value> {
        let parsed = match lenient::parse(input) {
            ParseResult::Success(v) => v,
            ParseResult::Failure {
                data,
                input,
                errors,
            } => {
                return ParseResult::Failure {
                    data,
                    input,
                    errors,
                };
            }
        };
        ParseResult::Success(coerce_with_schema(parsed, &self.parameters))
    }

    pub fn validate(&self, value: &Value) -> Validation<Value> {
        validator::validate(value, &self.parameters)
    }

    pub fn parse_and_validate(&self, input: &str) -> Validation<Value> {
        match self.parse(input) {
            ParseResult::Success(v) => self.validate(&v),
            ParseResult::Failure { .. } => Validation::Failure(ValidationFailure {
                data: Value::Null,
                errors: vec![ValidationError {
                    path: "$input".into(),
                    expected: "valid JSON".into(),
                    value: Value::Null,
                    description: Some("failed to parse input".into()),
                }],
            }),
        }
    }

    pub fn to_openai_tool(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }

    pub fn to_claude_tool(&self) -> Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": self.parameters,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmApplication {
    pub functions: Vec<LlmFunction>,
}

impl LlmApplication {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
        }
    }

    pub fn add_function(&mut self, func: LlmFunction) {
        self.functions.push(func);
    }

    pub fn find(&self, name: &str) -> Option<&LlmFunction> {
        self.functions.iter().find(|f| f.name == name)
    }

    pub fn to_openai_tools(&self) -> Vec<Value> {
        self.functions
            .iter()
            .map(LlmFunction::to_openai_tool)
            .collect()
    }

    pub fn to_claude_tools(&self) -> Vec<Value> {
        self.functions
            .iter()
            .map(LlmFunction::to_claude_tool)
            .collect()
    }
}

impl Default for LlmApplication {
    fn default() -> Self {
        Self::new()
    }
}

type Dispatcher<T> = Box<dyn Fn(&T, Value) -> Result<Value, String>>;

pub struct LlmController<T> {
    pub name: String,
    pub application: LlmApplication,
    pub instance: T,
    dispatchers: HashMap<String, Dispatcher<T>>,
}

impl<T> LlmController<T> {
    pub fn new(name: impl Into<String>, instance: T) -> Self {
        Self {
            name: name.into(),
            application: LlmApplication::new(),
            instance,
            dispatchers: HashMap::new(),
        }
    }

    pub fn register<F>(&mut self, func: LlmFunction, dispatcher: F)
    where
        F: Fn(&T, Value) -> Result<Value, String> + 'static,
    {
        let name = func.name.clone();
        self.application.add_function(func);
        self.dispatchers.insert(name, Box::new(dispatcher));
    }

    pub fn execute(&self, function_name: &str, args: Value) -> Result<Value, String> {
        let func = self
            .application
            .find(function_name)
            .ok_or_else(|| format!("function '{function_name}' not found"))?;
        let coerced = coerce_with_schema(args, &func.parameters);
        match func.validate(&coerced) {
            Validation::Success(validated) => {
                let dispatcher = self
                    .dispatchers
                    .get(function_name)
                    .ok_or_else(|| format!("no dispatcher for '{function_name}'"))?;
                dispatcher(&self.instance, validated)
            }
            Validation::Failure(f) => Err(f.to_llm_feedback()),
        }
    }

    pub fn execute_raw(&self, function_name: &str, raw_input: &str) -> Result<Value, String> {
        let func = self
            .application
            .find(function_name)
            .ok_or_else(|| format!("function '{function_name}' not found"))?;
        match func.parse_and_validate(raw_input) {
            Validation::Success(validated) => {
                let dispatcher = self
                    .dispatchers
                    .get(function_name)
                    .ok_or_else(|| format!("no dispatcher for '{function_name}'"))?;
                dispatcher(&self.instance, validated)
            }
            Validation::Failure(f) => Err(f.to_llm_feedback()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "item": {"type": "string"},
                "quantity": {"type": "integer", "minimum": 1}
            },
            "required": ["item", "quantity"]
        })
    }

    #[test]
    fn llm_function_parse_and_validate() {
        let func = LlmFunction::new("create_order", "Create a new order", test_schema());
        let result = func.parse_and_validate(r#"{"item": "widget", "quantity": 5}"#);
        assert!(result.is_success());
    }

    #[test]
    fn llm_function_coerces_types() {
        let func = LlmFunction::new("create_order", "Create order", test_schema());
        let result = func.parse_and_validate(r#"{"item": "widget", "quantity": "5"}"#);
        assert!(result.is_success());
        if let Validation::Success(v) = result {
            assert_eq!(v["quantity"], 5);
        }
    }

    #[test]
    fn llm_function_rejects_invalid() {
        let func = LlmFunction::new("create_order", "Create order", test_schema());
        let result = func.parse_and_validate(r#"{"item": "widget", "quantity": 0}"#);
        assert!(!result.is_success());
    }

    #[test]
    fn openai_tool_format() {
        let func = LlmFunction::new(
            "get_weather",
            "Get weather for a city",
            json!({
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }),
        );
        let tool = func.to_openai_tool();
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "get_weather");
    }

    #[test]
    fn claude_tool_format() {
        let func = LlmFunction::new(
            "get_weather",
            "Get weather",
            json!({
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }),
        );
        let tool = func.to_claude_tool();
        assert_eq!(tool["name"], "get_weather");
        assert!(tool["input_schema"].is_object());
    }

    #[test]
    fn application_tools() {
        let mut app = LlmApplication::new();
        app.add_function(LlmFunction::new("func_a", "A", json!({"type": "object"})));
        app.add_function(LlmFunction::new("func_b", "B", json!({"type": "object"})));
        assert_eq!(app.to_openai_tools().len(), 2);
        assert_eq!(app.to_claude_tools().len(), 2);
        assert!(app.find("func_a").is_some());
        assert!(app.find("func_c").is_none());
    }

    struct Calculator;
    impl Calculator {
        fn add(args: &Value) -> Result<Value, String> {
            let a = args["a"].as_f64().ok_or("missing a")?;
            let b = args["b"].as_f64().ok_or("missing b")?;
            Ok(json!(a + b))
        }
    }

    #[test]
    fn controller_execute() {
        let mut ctrl = LlmController::new("calculator", Calculator);
        ctrl.register(
            LlmFunction::new(
                "add",
                "Add two numbers",
                json!({
                    "type": "object",
                    "properties": {
                        "a": {"type": "number"},
                        "b": {"type": "number"}
                    },
                    "required": ["a", "b"]
                }),
            ),
            |_calc, args| Calculator::add(&args),
        );
        let result = ctrl.execute("add", json!({"a": 3, "b": 4})).unwrap();
        assert_eq!(result, json!(7.0));
    }

    #[test]
    fn controller_execute_raw() {
        let mut ctrl = LlmController::new("calculator", Calculator);
        ctrl.register(
            LlmFunction::new(
                "add",
                "Add",
                json!({
                    "type": "object",
                    "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
                    "required": ["a", "b"]
                }),
            ),
            |_calc, args| Calculator::add(&args),
        );
        let result = ctrl.execute_raw("add", r#"{"a": "3", "b": "4"}"#).unwrap();
        assert_eq!(result, json!(7.0));
    }

    #[test]
    fn controller_validates_before_execute() {
        let mut ctrl = LlmController::new("calc", Calculator);
        ctrl.register(
            LlmFunction::new(
                "add",
                "Add",
                json!({
                    "type": "object",
                    "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
                    "required": ["a", "b"]
                }),
            ),
            |_calc, args| Calculator::add(&args),
        );
        let result = ctrl.execute("add", json!({"a": 3}));
        assert!(result.is_err());
    }

    #[test]
    fn controller_unknown_function() {
        let ctrl = LlmController::new("calc", Calculator);
        let result = ctrl.execute("unknown", json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }
}
