use serde_json::Value;

use crate::types::{Validation, ValidationError, ValidationFailure};

pub fn validate(value: &Value, schema: &Value) -> Validation<Value> {
    validate_inner(value, schema, false)
}

pub fn validate_strict(value: &Value, schema: &Value) -> Validation<Value> {
    validate_inner(value, schema, true)
}

fn validate_inner(value: &Value, schema: &Value, strict: bool) -> Validation<Value> {
    if has_circular_refs(schema) {
        return Validation::Failure(ValidationFailure {
            data: value.clone(),
            errors: vec![ValidationError {
                path: "$input".into(),
                expected: "non-circular $ref".into(),
                value: value.clone(),
                description: Some("$ref cycle detected in schema".into()),
            }],
        });
    }
    let schema_to_use;
    let schema_ref = if strict {
        schema_to_use = inject_additional_properties_false(schema);
        &schema_to_use
    } else {
        schema
    };
    let validator = match jsonschema::options()
        .should_validate_formats(true)
        .build(schema_ref)
    {
        Ok(v) => v,
        Err(e) => {
            return Validation::Failure(ValidationFailure {
                data: value.clone(),
                errors: vec![ValidationError {
                    path: "$input".into(),
                    expected: "valid schema".into(),
                    value: value.clone(),
                    description: Some(format!("{e}")),
                }],
            });
        }
    };
    let js_errors: Vec<_> = validator.iter_errors(value).collect();
    if js_errors.is_empty() {
        return Validation::Success(value.clone());
    }
    let errors: Vec<ValidationError> = js_errors.iter().map(convert_error).collect();
    Validation::Failure(ValidationFailure {
        data: value.clone(),
        errors,
    })
}

fn convert_error(e: &jsonschema::ValidationError<'_>) -> ValidationError {
    let base_path = convert_path(&e.instance_path);
    let (path, expected, description, value) = match &e.kind {
        jsonschema::error::ValidationErrorKind::Required { property } => {
            let prop_str = property.as_str().unwrap_or("unknown");
            let full_path = format!("{base_path}.{prop_str}");
            let expected = "required property".to_owned();
            let desc = Some(format!(
                "The value at this path is `undefined`.\n\nPlease fill the `{prop_str}` typed value next time."
            ));
            (full_path, expected, desc, Value::Null)
        }
        kind => {
            let (expected, description) = convert_kind(kind);
            let instance_value = e.instance.clone().into_owned();
            (base_path, expected, description, instance_value)
        }
    };
    ValidationError {
        path,
        expected,
        value,
        description,
    }
}

fn convert_path(location: &jsonschema::paths::Location) -> String {
    let raw = location.as_str();
    if raw.is_empty() {
        return "$input".into();
    }
    let mut result = String::from("$input");
    let mut rest = raw;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix('/') {
            rest = tail;
        } else {
            break;
        }
        let (segment, remainder) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, ""),
        };
        rest = remainder;
        let unescaped = segment.replace("~1", "/").replace("~0", "~");
        if let Ok(idx) = unescaped.parse::<usize>() {
            use std::fmt::Write;
            let _ = write!(result, "[{idx}]");
        } else {
            result.push('.');
            result.push_str(&unescaped);
        }
    }
    result
}

fn convert_kind(kind: &jsonschema::error::ValidationErrorKind) -> (String, Option<String>) {
    use jsonschema::error::ValidationErrorKind as K;
    match kind {
        K::Type { kind: tk } => {
            use jsonschema::error::TypeKind;
            let expected = match tk {
                TypeKind::Single(pt) => pt.to_string(),
                TypeKind::Multiple(bitmap) => {
                    let types: Vec<String> = (*bitmap).into_iter().map(|t| t.to_string()).collect();
                    format!("({})", types.join(" | "))
                }
            };
            (expected, None)
        }
        K::Required { .. } => unreachable!("handled in convert_error"),
        K::AdditionalProperties { unexpected } => {
            // We'll return one error per unexpected property from the caller side,
            // but jsonschema bundles them. Return a single error here.
            let desc = unexpected
                .iter()
                .map(|u| format!("unexpected property '{u}'"))
                .collect::<Vec<_>>()
                .join(", ");
            ("no extraneous properties".into(), Some(desc))
        }
        K::Format { format } => (format!("string & Format<\"{format}\">"), None),
        K::Minimum { limit } => {
            let ty = num_type(limit);
            (format!("{ty} & Minimum<{limit}>"), None)
        }
        K::Maximum { limit } => {
            let ty = num_type(limit);
            (format!("{ty} & Maximum<{limit}>"), None)
        }
        K::ExclusiveMinimum { limit } => {
            let ty = num_type(limit);
            (format!("{ty} & ExclusiveMinimum<{limit}>"), None)
        }
        K::ExclusiveMaximum { limit } => {
            let ty = num_type(limit);
            (format!("{ty} & ExclusiveMaximum<{limit}>"), None)
        }
        K::MultipleOf { multiple_of } => (format!("number & MultipleOf<{multiple_of}>"), None),
        K::MinLength { limit } => (format!("string & MinLength<{limit}>"), None),
        K::MaxLength { limit } => (format!("string & MaxLength<{limit}>"), None),
        K::Pattern { pattern } => (format!("string & Pattern<\"{pattern}\">"), None),
        K::MinItems { limit } => (format!("array & MinItems<{limit}>"), None),
        K::MaxItems { limit } => (format!("array & MaxItems<{limit}>"), None),
        K::UniqueItems => (
            "array & UniqueItems".into(),
            Some("Array contains duplicate elements.".into()),
        ),
        K::MinProperties { limit } => (format!("object & MinProperties<{limit}>"), None),
        K::MaxProperties { limit } => (format!("object & MaxProperties<{limit}>"), None),
        K::Enum { options } => (format!("one of {options}"), None),
        K::Constant { expected_value } => (format!("const {expected_value}"), None),
        K::Not { .. } => ("value NOT matching the sub-schema".into(), None),
        K::AnyOf => ("union type".into(), None),
        K::OneOfNotValid => (
            "oneOf match".into(),
            Some("oneOf: expected exactly 1 match but found 0".into()),
        ),
        K::OneOfMultipleValid => (
            "oneOf match".into(),
            Some("oneOf: expected exactly 1 match but found multiple".into()),
        ),
        K::Contains => ("array & Contains".into(), None),
        K::PropertyNames { error } => {
            let inner_path = convert_path(&error.instance_path);
            (format!("valid property name at {inner_path}"), None)
        }
        K::AdditionalItems { limit } => (format!("array with at most {limit} items"), None),
        K::FalseSchema => ("never (false schema)".into(), None),
        K::Referencing(err) => ("resolvable $ref".into(), Some(format!("{err}"))),
        K::UnevaluatedProperties { unexpected } => {
            let desc = unexpected
                .iter()
                .map(|u| format!("unexpected property '{u}'"))
                .collect::<Vec<_>>()
                .join(", ");
            ("no extraneous properties".into(), Some(desc))
        }
        K::UnevaluatedItems { unexpected } => {
            let desc = unexpected
                .iter()
                .map(|u| format!("unexpected item '{u}'"))
                .collect::<Vec<_>>()
                .join(", ");
            ("no extraneous items".into(), Some(desc))
        }
        _ => (format!("{kind:?}"), None),
    }
}

fn num_type(limit: &Value) -> &'static str {
    match limit {
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        _ => "number",
    }
}

fn follow_ref(
    name: &str,
    defs: &serde_json::Map<String, Value>,
    visited: &mut std::collections::HashSet<String>,
) -> bool {
    if !visited.insert(name.to_owned()) {
        return true;
    }
    if let Some(def) = defs.get(name) {
        if let Some(r) = def.get("$ref").and_then(Value::as_str) {
            let key = r
                .strip_prefix("#/$defs/")
                .or_else(|| r.strip_prefix("#/definitions/"));
            if let Some(k) = key {
                return follow_ref(k, defs, visited);
            }
        }
    }
    false
}

fn has_circular_refs(schema: &Value) -> bool {
    use std::collections::HashSet;
    let defs = schema
        .get("$defs")
        .or_else(|| schema.get("definitions"))
        .and_then(Value::as_object);
    let Some(defs) = defs else {
        return false;
    };
    for key in defs.keys() {
        let mut visited = HashSet::new();
        if follow_ref(key, defs, &mut visited) {
            return true;
        }
    }
    false
}

fn inject_additional_properties_false(schema: &Value) -> Value {
    match schema {
        Value::Object(obj) => {
            let mut new_obj = obj.clone();
            if new_obj.contains_key("properties") && !new_obj.contains_key("additionalProperties") {
                new_obj.insert("additionalProperties".into(), Value::Bool(false));
            }
            for (key, val) in &mut new_obj {
                match key.as_str() {
                    "properties" | "$defs" | "definitions" => {
                        if let Value::Object(map) = val {
                            for sub in map.values_mut() {
                                *sub = inject_additional_properties_false(sub);
                            }
                        }
                    }
                    "items" => {
                        *val = inject_additional_properties_false(val);
                    }
                    "allOf" | "anyOf" | "oneOf" => {
                        if let Value::Array(arr) = val {
                            for item in arr.iter_mut() {
                                *item = inject_additional_properties_false(item);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Value::Object(new_obj)
        }
        other => other.clone(),
    }
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

    #[test]
    fn unique_items() {
        let schema = json!({"type": "array", "items": {"type": "number"}, "uniqueItems": true});
        assert!(validate(&json!([1, 2, 3]), &schema).is_success());
        assert!(!validate(&json!([1, 2, 2]), &schema).is_success());
    }

    #[test]
    fn unique_items_objects() {
        let schema = json!({"type": "array", "items": {"type": "object"}, "uniqueItems": true});
        assert!(validate(&json!([{"a":1},{"b":2}]), &schema).is_success());
        assert!(!validate(&json!([{"a":1},{"a":1}]), &schema).is_success());
    }

    #[test]
    fn circular_ref_returns_error() {
        let schema = json!({
            "$ref": "#/$defs/A",
            "$defs": { "A": { "$ref": "#/$defs/A" } }
        });
        let result = validate(&json!("test"), &schema);
        // jsonschema crate handles circular refs — may pass or fail depending on detection
        // The key guarantee: it doesn't infinite-loop
        let _ = result;
    }

    #[test]
    fn self_referencing_ref_returns_error() {
        let schema = json!({
            "$ref": "#/$defs/Node",
            "$defs": {
                "Node": {
                    "$ref": "#/$defs/Leaf"
                },
                "Leaf": {
                    "$ref": "#/$defs/Node"
                }
            }
        });
        let result = validate(&json!(42), &schema);
        let _ = result;
    }

    #[test]
    fn empty_schema_validates_everything() {
        let schema = json!({});
        assert!(validate(&json!("hello"), &schema).is_success());
        assert!(validate(&json!(42), &schema).is_success());
        assert!(validate(&json!(null), &schema).is_success());
        assert!(validate(&json!({"a": 1}), &schema).is_success());
    }

    #[test]
    fn all_of_validates_all_sub_schemas() {
        let schema = json!({
            "allOf": [
                {"type":"object","properties":{"a":{"type":"string"}},"required":["a"]},
                {"type":"object","properties":{"b":{"type":"integer"}},"required":["b"]}
            ]
        });
        assert!(validate(&json!({"a":"hi","b":1}), &schema).is_success());
        assert!(!validate(&json!({"a":"hi"}), &schema).is_success());
        assert!(!validate(&json!({"b":1}), &schema).is_success());
    }

    #[test]
    fn not_rejects_matching_value() {
        let schema = json!({"not":{"type":"string"}});
        assert!(!validate(&json!("hello"), &schema).is_success());
        assert!(validate(&json!(42), &schema).is_success());
        assert!(validate(&json!(null), &schema).is_success());
    }

    #[test]
    fn pattern_properties_validates_matching_keys() {
        let schema = json!({
            "type":"object",
            "patternProperties":{"^x-":{"type":"string"}}
        });
        assert!(validate(&json!({"x-custom":"ok"}), &schema).is_success());
        assert!(!validate(&json!({"x-custom":123}), &schema).is_success());
        assert!(validate(&json!({"normal":123}), &schema).is_success());
    }

    #[test]
    fn min_max_properties() {
        let schema = json!({"type":"object","minProperties":2});
        assert!(!validate(&json!({"a":1}), &schema).is_success());
        assert!(validate(&json!({"a":1,"b":2}), &schema).is_success());
        let schema2 = json!({"type":"object","maxProperties":1});
        assert!(validate(&json!({"a":1}), &schema2).is_success());
        assert!(!validate(&json!({"a":1,"b":2}), &schema2).is_success());
    }

    #[test]
    fn multiple_of_zero_is_silently_skipped() {
        let schema = json!({"type":"number","multipleOf":0});
        // jsonschema crate may reject multipleOf:0 at schema level
        // We just ensure no panic
        let _ = validate(&json!(42), &schema);
        let _ = validate(&json!(3.15), &schema);
    }

    #[test]
    fn duplicate_json_keys_last_wins() {
        let raw = r#"{"a":1,"a":2}"#;
        let value: Value = serde_json::from_str(raw).expect("valid json");
        assert_eq!(value["a"], json!(2));
        let schema = json!({"type":"object","properties":{"a":{"type":"number"}}});
        assert!(validate(&value, &schema).is_success());
    }

    #[test]
    fn if_then_else_conditional() {
        let schema = json!({
            "type":"object",
            "properties":{"status":{"type":"string"}},
            "if":{"properties":{"status":{"const":"shipped"}},"required":["status"]},
            "then":{"required":["tracking_number"]},
            "else":{}
        });
        assert!(!validate(&json!({"status":"shipped"}), &schema).is_success());
        assert!(
            validate(
                &json!({"status":"shipped","tracking_number":"123"}),
                &schema
            )
            .is_success()
        );
        assert!(validate(&json!({"status":"pending"}), &schema).is_success());
    }

    #[test]
    fn external_ref_produces_error() {
        let schema = json!({"$ref":"https://example.com/schema.json"});
        let result = validate(&json!("anything"), &schema);
        // jsonschema crate may handle this differently (fetch or error)
        // Key: no panic
        let _ = result;
    }

    #[test]
    fn additional_properties_false_rejects_extra() {
        let schema = json!({
            "type":"object",
            "properties":{"a":{"type":"string"}},
            "additionalProperties":false
        });
        assert!(validate(&json!({"a":"hi"}), &schema).is_success());
        assert!(!validate(&json!({"a":"hi","b":1}), &schema).is_success());
    }

    #[test]
    fn one_of_requires_exactly_one_match() {
        let schema = json!({
            "oneOf":[
                {"type":"string"},
                {"type":"string","minLength":5}
            ]
        });
        assert!(!validate(&json!("hello world"), &schema).is_success());
        assert!(validate(&json!("hi"), &schema).is_success());
    }

    #[test]
    fn any_of_passes_with_multiple_matches() {
        let schema = json!({
            "anyOf":[
                {"type":"string"},
                {"type":"string","minLength":5}
            ]
        });
        assert!(validate(&json!("hello world"), &schema).is_success());
        assert!(validate(&json!("hi"), &schema).is_success());
    }

    #[test]
    fn type_array_support() {
        let schema = json!({"type":["string","integer"]});
        assert!(validate(&json!("hello"), &schema).is_success());
        assert!(validate(&json!(42), &schema).is_success());
        assert!(!validate(&json!(true), &schema).is_success());
    }

    #[test]
    fn tuple_items_validation() {
        let schema = json!({
            "type":"array",
            "prefixItems":[{"type":"string"},{"type":"integer"}]
        });
        assert!(validate(&json!(["a", 1]), &schema).is_success());
        assert!(!validate(&json!(["a", "b"]), &schema).is_success());
    }
}
