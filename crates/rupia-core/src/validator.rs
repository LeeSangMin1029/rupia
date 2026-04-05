use serde_json::Value;

use crate::types::{Validation, ValidationError, ValidationFailure};

pub fn validate(value: &Value, schema: &Value) -> Validation<Value> {
    validate_with_options(value, schema, false)
}

pub fn validate_strict(value: &Value, schema: &Value) -> Validation<Value> {
    validate_with_options(value, schema, true)
}

fn validate_with_options(value: &Value, schema: &Value, equals: bool) -> Validation<Value> {
    let defs = schema.get("$defs").or_else(|| schema.get("definitions"));
    let mut errors = Vec::new();
    validate_value(value, schema, defs, "$input", true, equals, &mut errors);
    if errors.is_empty() {
        Validation::Success(value.clone())
    } else {
        Validation::Failure(ValidationFailure {
            data: value.clone(),
            errors,
        })
    }
}

fn validate_value(
    value: &Value,
    schema: &Value,
    defs: Option<&Value>,
    path: &str,
    required: bool,
    equals: bool,
    errors: &mut Vec<ValidationError>,
) {
    if schema.get("type").is_none()
        && schema.get("$ref").is_none()
        && schema.get("anyOf").is_none()
        && schema.get("oneOf").is_none()
        && schema.get("const").is_none()
        && schema.get("enum").is_none()
    {
        return;
    }
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let key = r
            .strip_prefix("#/$defs/")
            .or_else(|| r.strip_prefix("#/definitions/"));
        if let Some(resolved) = key.and_then(|k| defs?.get(k)) {
            validate_value(value, resolved, defs, path, required, equals, errors);
            return;
        }
        return;
    }
    if let Some(variants) = schema.get("anyOf").or_else(|| schema.get("oneOf")) {
        if let Some(arr) = variants.as_array() {
            validate_one_of(value, arr, defs, path, required, equals, errors);
            return;
        }
    }
    if let Some(const_val) = schema.get("const") {
        if value != const_val {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("const {const_val}"),
                value: value.clone(),
                description: None,
            });
        }
        return;
    }
    if let Some(enum_vals) = schema.get("enum").and_then(Value::as_array) {
        if !enum_vals.contains(value) {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("one of {}", Value::Array(enum_vals.clone())),
                value: value.clone(),
                description: None,
            });
        }
        return;
    }
    match value {
        Value::Null => {
            if !is_type(schema, "null") {
                errors.push(ValidationError {
                    path: path.to_owned(),
                    expected: expected_type(schema),
                    value: value.clone(),
                    description: None,
                });
            }
        }
        Value::Bool(_) => {
            if !is_type(schema, "boolean") {
                errors.push(ValidationError {
                    path: path.to_owned(),
                    expected: expected_type(schema),
                    value: value.clone(),
                    description: None,
                });
            }
        }
        Value::Number(n) => validate_number(n, schema, path, errors),
        Value::String(s) => validate_string(s, schema, path, errors),
        Value::Array(arr) => validate_array(arr, schema, defs, path, equals, errors),
        Value::Object(obj) => validate_object(obj, schema, defs, path, equals, errors),
    }
}

fn validate_one_of(
    value: &Value,
    variants: &[Value],
    defs: Option<&Value>,
    path: &str,
    required: bool,
    equals: bool,
    errors: &mut Vec<ValidationError>,
) {
    for variant in variants {
        let mut sub_errors = Vec::new();
        validate_value(
            value,
            variant,
            defs,
            path,
            required,
            equals,
            &mut sub_errors,
        );
        if sub_errors.is_empty() {
            return;
        }
    }
    let expected: Vec<String> = variants.iter().map(expected_type).collect();
    errors.push(ValidationError {
        path: path.to_owned(),
        expected: format!("({})", expected.join(" | ")),
        value: value.clone(),
        description: None,
    });
}

fn validate_number(
    n: &serde_json::Number,
    schema: &Value,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    let ty = schema
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("number");
    if ty == "integer" {
        if n.as_i64().is_none() && n.as_u64().is_none() {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: "integer".into(),
                value: Value::Number(n.clone()),
                description: None,
            });
            return;
        }
    } else if ty != "number" {
        errors.push(ValidationError {
            path: path.to_owned(),
            expected: expected_type(schema),
            value: Value::Number(n.clone()),
            description: None,
        });
        return;
    }
    let val = n.as_f64().unwrap_or(0.0);
    if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
        if val < min {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("{ty} & Minimum<{min}>"),
                value: Value::Number(n.clone()),
                description: None,
            });
        }
    }
    if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
        if val > max {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("{ty} & Maximum<{max}>"),
                value: Value::Number(n.clone()),
                description: None,
            });
        }
    }
    if let Some(emin) = schema.get("exclusiveMinimum").and_then(Value::as_f64) {
        if val <= emin {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("{ty} & ExclusiveMinimum<{emin}>"),
                value: Value::Number(n.clone()),
                description: None,
            });
        }
    }
    if let Some(emax) = schema.get("exclusiveMaximum").and_then(Value::as_f64) {
        if val >= emax {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("{ty} & ExclusiveMaximum<{emax}>"),
                value: Value::Number(n.clone()),
                description: None,
            });
        }
    }
    if let Some(mult) = schema.get("multipleOf").and_then(Value::as_f64) {
        if mult != 0.0 && (val % mult).abs() > f64::EPSILON {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("{ty} & MultipleOf<{mult}>"),
                value: Value::Number(n.clone()),
                description: None,
            });
        }
    }
}

fn validate_string(s: &str, schema: &Value, path: &str, errors: &mut Vec<ValidationError>) {
    if !is_type(schema, "string") {
        errors.push(ValidationError {
            path: path.to_owned(),
            expected: expected_type(schema),
            value: Value::String(s.to_owned()),
            description: None,
        });
        return;
    }
    if let Some(min_len) = schema.get("minLength").and_then(Value::as_u64) {
        if (s.len() as u64) < min_len {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("string & MinLength<{min_len}>"),
                value: Value::String(s.to_owned()),
                description: None,
            });
        }
    }
    if let Some(max_len) = schema.get("maxLength").and_then(Value::as_u64) {
        if s.len() as u64 > max_len {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("string & MaxLength<{max_len}>"),
                value: Value::String(s.to_owned()),
                description: None,
            });
        }
    }
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
        if let Ok(re) = regex_lite::Regex::new(pattern) {
            if !re.is_match(s) {
                errors.push(ValidationError {
                    path: path.to_owned(),
                    expected: format!("string & Pattern<\"{pattern}\">"),
                    value: Value::String(s.to_owned()),
                    description: None,
                });
            }
        }
    }
    if let Some(fmt) = schema.get("format").and_then(Value::as_str) {
        if !crate::format::validate(s, fmt) {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("string & Format<\"{fmt}\">"),
                value: Value::String(s.to_owned()),
                description: None,
            });
        }
    }
}

fn validate_array(
    arr: &[Value],
    schema: &Value,
    defs: Option<&Value>,
    path: &str,
    equals: bool,
    errors: &mut Vec<ValidationError>,
) {
    if !is_type(schema, "array") {
        errors.push(ValidationError {
            path: path.to_owned(),
            expected: expected_type(schema),
            value: Value::Array(arr.to_vec()),
            description: None,
        });
        return;
    }
    if let Some(min) = schema.get("minItems").and_then(Value::as_u64) {
        if (arr.len() as u64) < min {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("array & MinItems<{min}>"),
                value: Value::Array(arr.to_vec()),
                description: None,
            });
        }
    }
    if let Some(max) = schema.get("maxItems").and_then(Value::as_u64) {
        if arr.len() as u64 > max {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("array & MaxItems<{max}>"),
                value: Value::Array(arr.to_vec()),
                description: None,
            });
        }
    }
    if let Some(items_schema) = schema.get("items") {
        for (i, item) in arr.iter().enumerate() {
            validate_value(
                item,
                items_schema,
                defs,
                &format!("{path}[{i}]"),
                true,
                equals,
                errors,
            );
        }
    }
}

fn validate_object(
    obj: &serde_json::Map<String, Value>,
    schema: &Value,
    defs: Option<&Value>,
    path: &str,
    equals: bool,
    errors: &mut Vec<ValidationError>,
) {
    if !is_type(schema, "object") {
        errors.push(ValidationError {
            path: path.to_owned(),
            expected: expected_type(schema),
            value: Value::Object(obj.clone()),
            description: None,
        });
        return;
    }
    let properties = schema.get("properties").and_then(Value::as_object);
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for req in required {
            if let Some(key) = req.as_str() {
                if !obj.contains_key(key) {
                    let prop_schema = properties.and_then(|p| p.get(key));
                    let expected =
                        prop_schema.map_or_else(|| "required property".to_owned(), expected_type);
                    errors.push(ValidationError {
                        path: format!("{path}.{key}"),
                        expected,
                        value: Value::Null,
                        description: Some(format!(
                            "The value at this path is `undefined`.\n\nPlease fill the `{key}` typed value next time."
                        )),
                    });
                }
            }
        }
    }
    if let Some(props) = properties {
        for (key, prop_schema) in props {
            if let Some(val) = obj.get(key) {
                validate_value(
                    val,
                    prop_schema,
                    defs,
                    &format!("{path}.{key}"),
                    true,
                    equals,
                    errors,
                );
            }
        }
    }
    if equals {
        if let Some(props) = properties {
            for key in obj.keys() {
                if !props.contains_key(key) {
                    errors.push(ValidationError {
                        path: format!("{path}.{key}"),
                        expected: "no extraneous properties".into(),
                        value: obj.get(key).cloned().unwrap_or(Value::Null),
                        description: Some(format!("unexpected property '{key}'")),
                    });
                }
            }
        }
    }
}

fn is_type(schema: &Value, ty: &str) -> bool {
    schema.get("type").and_then(Value::as_str) == Some(ty)
}

fn expected_type(schema: &Value) -> String {
    if let Some(ty) = schema.get("type").and_then(Value::as_str) {
        return ty.to_owned();
    }
    if schema.get("$ref").is_some() {
        return "referenced type".into();
    }
    if schema.get("anyOf").is_some() || schema.get("oneOf").is_some() {
        return "union type".into();
    }
    "unknown".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_object() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "number"}
            },
            "required": ["name"]
        });
        let value = json!({"name": "test", "age": 25});
        assert!(validate(&value, &schema).is_success());
    }

    #[test]
    fn missing_required() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            },
            "required": ["name"]
        });
        let value = json!({});
        let result = validate(&value, &schema);
        assert!(!result.is_success());
    }

    #[test]
    fn wrong_type() {
        let schema = json!({"type": "number"});
        let value = json!("not a number");
        let result = validate(&value, &schema);
        assert!(!result.is_success());
        if let Validation::Failure(f) = result {
            assert_eq!(f.errors[0].path, "$input");
            assert_eq!(f.errors[0].expected, "number");
        }
    }

    #[test]
    fn number_range() {
        let schema = json!({"type": "number", "minimum": 0, "maximum": 150});
        assert!(validate(&json!(25), &schema).is_success());
        assert!(!validate(&json!(-5), &schema).is_success());
        assert!(!validate(&json!(200), &schema).is_success());
    }

    #[test]
    fn string_format_email() {
        let schema = json!({"type": "string", "format": "email"});
        assert!(validate(&json!("test@example.com"), &schema).is_success());
        assert!(!validate(&json!("not-an-email"), &schema).is_success());
    }

    #[test]
    fn nested_validation() {
        let schema = json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "email": {"type": "string", "format": "email"}
                    },
                    "required": ["email"]
                }
            },
            "required": ["user"]
        });
        let value = json!({"user": {"email": "bad"}});
        let result = validate(&value, &schema);
        assert!(!result.is_success());
        if let Validation::Failure(f) = result {
            assert!(f.errors[0].path.contains("email"));
        }
    }

    #[test]
    fn array_items_validation() {
        let schema = json!({
            "type": "array",
            "items": {"type": "number", "minimum": 0}
        });
        assert!(validate(&json!([1, 2, 3]), &schema).is_success());
        assert!(!validate(&json!([1, -2, 3]), &schema).is_success());
    }

    #[test]
    fn any_of() {
        let schema = json!({
            "anyOf": [
                {"type": "string"},
                {"type": "number"}
            ]
        });
        assert!(validate(&json!("hello"), &schema).is_success());
        assert!(validate(&json!(42), &schema).is_success());
        assert!(!validate(&json!(true), &schema).is_success());
    }

    #[test]
    fn strict_rejects_extra() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}}
        });
        let value = json!({"name": "test", "extra": true});
        assert!(validate(&value, &schema).is_success());
        assert!(!validate_strict(&value, &schema).is_success());
    }
}
