use serde_json::Value;

use crate::lenient;
use crate::types::ParseResult;

pub fn coerce_with_schema(value: Value, schema: &Value) -> Value {
    let defs = schema.get("$defs").or_else(|| schema.get("definitions"));
    coerce_value(value, schema, defs)
}

fn coerce_value(value: Value, schema: &Value, defs: Option<&Value>) -> Value {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        if let Some(resolved) = resolve_ref(r, defs) {
            return coerce_value(value, resolved, defs);
        }
        return value;
    }
    if let Some(any_of) = schema.get("anyOf").and_then(Value::as_array) {
        return coerce_any_of(value, any_of, defs);
    }
    if let Some(one_of) = schema.get("oneOf").and_then(Value::as_array) {
        return coerce_any_of(value, one_of, defs);
    }
    if is_string_schema(schema) {
        return value;
    }
    if let Value::String(s) = &value {
        if let ParseResult::Success(parsed) = lenient::parse(s) {
            return coerce_value(parsed, schema, defs);
        }
        if s.len() == 1 && s.eq_ignore_ascii_case("n") {
            if is_null_schema(schema) {
                return Value::Null;
            }
            if is_boolean_schema(schema) {
                return Value::Bool(false);
            }
        }
        return value;
    }
    if let Value::Array(arr) = value {
        if is_array_schema(schema) {
            let items_schema = schema
                .get("items")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::default()));
            let coerced: Vec<Value> = arr
                .into_iter()
                .map(|item| coerce_value(item, &items_schema, defs))
                .collect();
            return Value::Array(coerced);
        }
        return Value::Array(arr);
    }
    if let Value::Object(obj) = value {
        if is_object_schema(schema) {
            return coerce_object(obj, schema, defs);
        }
        return Value::Object(obj);
    }
    value
}

fn coerce_any_of(value: Value, variants: &[Value], defs: Option<&Value>) -> Value {
    if let Value::String(ref s) = value {
        let has_string = variants.iter().any(|v| is_string_schema(resolve(v, defs)));
        if has_string {
            return value;
        }
        if let ParseResult::Success(parsed) = lenient::parse(s) {
            if let Some(matched) = find_matching_schema(&parsed, variants, defs) {
                return coerce_value(parsed, matched, defs);
            }
            return parsed;
        }
        if s.len() == 1 && s.eq_ignore_ascii_case("n") {
            let has_bool = variants.iter().any(|v| is_boolean_schema(resolve(v, defs)));
            let has_null = variants.iter().any(|v| is_null_schema(resolve(v, defs)));
            if has_bool && !has_null {
                return Value::Bool(false);
            }
            if has_null && !has_bool {
                return Value::Null;
            }
        }
        return value;
    }
    if let Value::Object(ref _obj) = value {
        if let Some(matched) = find_matching_schema(&value, variants, defs) {
            return coerce_value(value, matched, defs);
        }
        return value;
    }
    if let Value::Array(_) = value {
        let array_schemas: Vec<&Value> = variants
            .iter()
            .filter(|v| is_array_schema(resolve(v, defs)))
            .collect();
        if array_schemas.len() == 1 {
            return coerce_value(value, array_schemas[0], defs);
        }
        return value;
    }
    value
}

fn coerce_object(
    obj: serde_json::Map<String, Value>,
    schema: &Value,
    defs: Option<&Value>,
) -> Value {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Value::Object(obj);
    };
    let mut result = serde_json::Map::new();
    for (key, prop_schema) in properties {
        if let Some(val) = obj.get(key) {
            result.insert(key.clone(), coerce_value(val.clone(), prop_schema, defs));
        }
    }
    let additional =
        schema
            .get("additionalProperties")
            .and_then(|v| if v.is_object() { Some(v) } else { None });
    for (key, val) in &obj {
        if !properties.contains_key(key) {
            let coerced = match additional {
                Some(s) => coerce_value(val.clone(), s, defs),
                None => val.clone(),
            };
            result.insert(key.clone(), coerced);
        }
    }
    Value::Object(result)
}

fn resolve_ref<'a>(ref_path: &str, defs: Option<&'a Value>) -> Option<&'a Value> {
    let key = ref_path
        .strip_prefix("#/$defs/")
        .or_else(|| ref_path.strip_prefix("#/definitions/"))?;
    defs?.get(key)
}

fn resolve<'a>(schema: &'a Value, defs: Option<&'a Value>) -> &'a Value {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        if let Some(resolved) = resolve_ref(r, defs) {
            return resolve(resolved, defs);
        }
    }
    schema
}

fn find_matching_schema<'a>(
    value: &Value,
    variants: &'a [Value],
    defs: Option<&Value>,
) -> Option<&'a Value> {
    let matching: Vec<&Value> = variants
        .iter()
        .filter(|s| matches_type(value, resolve(s, defs)))
        .collect();
    if matching.len() == 1 {
        return Some(matching[0]);
    }
    None
}

fn matches_type(value: &Value, schema: &Value) -> bool {
    let ty = schema.get("type").and_then(Value::as_str);
    match (value, ty) {
        (Value::Number(n), Some("integer")) => n.as_i64().is_some() || n.as_u64().is_some(),
        (Value::Null, Some("null"))
        | (Value::Bool(_), Some("boolean"))
        | (Value::Number(_), Some("number"))
        | (Value::String(_), Some("string"))
        | (Value::Array(_), Some("array"))
        | (Value::Object(_), Some("object")) => true,
        _ => false,
    }
}

fn is_string_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("string")
}

fn is_boolean_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("boolean")
}

fn is_null_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("null")
}

fn is_array_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("array")
}

fn is_object_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("object")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coerce_string_to_number() {
        let schema = json!({"type": "number"});
        let result = coerce_with_schema(json!("42"), &schema);
        assert_eq!(result, json!(42));
    }

    #[test]
    fn coerce_string_to_boolean() {
        let schema = json!({"type": "boolean"});
        let result = coerce_with_schema(json!("true"), &schema);
        assert_eq!(result, json!(true));
    }

    #[test]
    fn coerce_string_to_null() {
        let schema = json!({"type": "null"});
        let result = coerce_with_schema(json!("null"), &schema);
        assert_eq!(result, json!(null));
    }

    #[test]
    fn coerce_string_to_object() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}}
        });
        let result = coerce_with_schema(json!(r#"{"name": "test"}"#), &schema);
        assert_eq!(result["name"], "test");
    }

    #[test]
    fn coerce_nested_object() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "number"}
            }
        });
        let input = json!({"name": "test", "age": "25"});
        let result = coerce_with_schema(input, &schema);
        assert_eq!(result["name"], "test");
        assert_eq!(result["age"], json!(25));
    }

    #[test]
    fn coerce_array_items() {
        let schema = json!({
            "type": "array",
            "items": {"type": "number"}
        });
        let input = json!(["1", "2", "3"]);
        let result = coerce_with_schema(input, &schema);
        assert_eq!(result, json!([1, 2, 3]));
    }

    #[test]
    fn no_coerce_string_when_string_schema() {
        let schema = json!({"type": "string"});
        let result = coerce_with_schema(json!("42"), &schema);
        assert_eq!(result, json!("42"));
    }

    #[test]
    fn coerce_any_of_discriminator() {
        let schema = json!({
            "anyOf": [
                {"type": "string"},
                {"type": "number"}
            ]
        });
        let result = coerce_with_schema(json!("hello"), &schema);
        assert_eq!(result, json!("hello"));
    }

    #[test]
    fn coerce_with_ref() {
        let schema = json!({
            "$ref": "#/$defs/MyType",
            "$defs": {
                "MyType": {"type": "number"}
            }
        });
        let result = coerce_with_schema(json!("42"), &schema);
        assert_eq!(result, json!(42));
    }
}
