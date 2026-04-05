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
        return coerce_to_string(value, schema);
    }
    if is_array_schema(schema) {
        return coerce_to_array(value, schema, defs);
    }
    if let Value::String(s) = &value {
        if let Some(coerced) = try_coerce_string(s, schema, defs) {
            return coerced;
        }
        return value;
    }
    if let Value::Array(arr) = value {
        if is_array_schema(schema) {
            return coerce_array_items(arr, schema, defs);
        }
        return Value::Array(arr);
    }
    if let Value::Object(obj) = value {
        if is_object_schema(schema) {
            return coerce_object(obj, schema, defs);
        }
        return Value::Object(obj);
    }
    if let Value::Null = &value {
        if let Some(default) = schema.get("default") {
            return default.clone();
        }
    }
    value
}

fn try_coerce_string(s: &str, schema: &Value, defs: Option<&Value>) -> Option<Value> {
    if let Some(enum_vals) = schema.get("enum").and_then(Value::as_array) {
        return coerce_enum_value(&Value::String(s.to_owned()), enum_vals);
    }
    if is_number_schema(schema) || is_integer_schema(schema) {
        if let Some(n) = parse_number_string(s) {
            return Some(n);
        }
    }
    if let ParseResult::Success(parsed) = lenient::parse(s) {
        return Some(coerce_value(parsed, schema, defs));
    }
    if s.len() == 1 && s.eq_ignore_ascii_case("n") {
        if is_null_schema(schema) {
            return Some(Value::Null);
        }
        if is_boolean_schema(schema) {
            return Some(Value::Bool(false));
        }
    }
    None
}

fn parse_number_string(s: &str) -> Option<Value> {
    let cleaned = s.replace(',', "").trim().to_owned();
    if let Some(rest) = cleaned
        .strip_suffix('k')
        .or_else(|| cleaned.strip_suffix('K'))
    {
        if let Ok(n) = rest.parse::<f64>() {
            return serde_json::Number::from_f64(n * 1000.0).map(Value::Number);
        }
    }
    if let Some(rest) = cleaned
        .strip_suffix('m')
        .or_else(|| cleaned.strip_suffix('M'))
    {
        if let Ok(n) = rest.parse::<f64>() {
            return serde_json::Number::from_f64(n * 1_000_000.0).map(Value::Number);
        }
    }
    if let Ok(n) = cleaned.parse::<i64>() {
        return Some(Value::Number(n.into()));
    }
    if let Ok(n) = cleaned.parse::<f64>() {
        return serde_json::Number::from_f64(n).map(Value::Number);
    }
    None
}

fn coerce_to_string(value: Value, schema: &Value) -> Value {
    let s = match &value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        // Null and complex types can't be meaningfully stringified
        _ => return value,
    };
    let trimmed = s.trim().to_owned();
    if let Some(enum_vals) = schema.get("enum").and_then(Value::as_array) {
        if let Some(matched) = coerce_enum_value(&Value::String(trimmed.clone()), enum_vals) {
            return matched;
        }
    }
    if trimmed != s {
        return Value::String(trimmed);
    }
    Value::String(s)
}

fn coerce_enum_value(value: &Value, enum_vals: &[Value]) -> Option<Value> {
    if enum_vals.contains(value) {
        return Some(value.clone());
    }
    if let Value::String(s) = value {
        let lower = s.to_lowercase();
        for e in enum_vals {
            if let Value::String(es) = e {
                if es.to_lowercase() == lower {
                    return Some(e.clone());
                }
            }
        }
        if let Ok(n) = s.parse::<i64>() {
            let num_val = Value::Number(n.into());
            if enum_vals.contains(&num_val) {
                return Some(num_val);
            }
        }
        let str_of_num = Value::String(s.clone());
        for e in enum_vals {
            if let Value::Number(n) = e {
                if n.to_string() == *s {
                    return Some(e.clone());
                }
            }
        }
        drop(str_of_num);
    }
    if let Value::Number(n) = value {
        let s = n.to_string();
        let str_val = Value::String(s);
        if enum_vals.contains(&str_val) {
            return Some(str_val);
        }
    }
    None
}

fn coerce_to_array(value: Value, schema: &Value, defs: Option<&Value>) -> Value {
    match value {
        Value::Array(arr) => coerce_array_items(arr, schema, defs),
        Value::Object(ref obj) => {
            if let Some(arr) = try_indexed_object_to_array(obj) {
                return coerce_array_items(arr, schema, defs);
            }
            Value::Array(vec![coerce_value(
                value,
                &schema
                    .get("items")
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::default())),
                defs,
            )])
        }
        _ => {
            let items_schema = schema
                .get("items")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::default()));
            Value::Array(vec![coerce_value(value, &items_schema, defs)])
        }
    }
}

fn try_indexed_object_to_array(obj: &serde_json::Map<String, Value>) -> Option<Vec<Value>> {
    if obj.is_empty() {
        return Some(Vec::new());
    }
    let mut indices: Vec<(usize, &Value)> = Vec::new();
    for (key, val) in obj {
        let idx: usize = key.parse().ok()?;
        indices.push((idx, val));
    }
    indices.sort_by_key(|(i, _)| *i);
    if indices.first()?.0 != 0 {
        return None;
    }
    for (i, (idx, _)) in indices.iter().enumerate() {
        if *idx != i {
            return None;
        }
    }
    Some(indices.into_iter().map(|(_, v)| v.clone()).collect())
}

fn coerce_array_items(arr: Vec<Value>, schema: &Value, defs: Option<&Value>) -> Value {
    let items_schema = schema
        .get("items")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::default()));
    Value::Array(
        arr.into_iter()
            .map(|item| coerce_value(item, &items_schema, defs))
            .collect(),
    )
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
            if val.is_null() {
                if let Some(default) = prop_schema.get("default") {
                    result.insert(key.clone(), default.clone());
                    continue;
                }
            }
            result.insert(key.clone(), coerce_value(val.clone(), prop_schema, defs));
        } else if let Some(default) = prop_schema.get("default") {
            result.insert(key.clone(), default.clone());
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

fn is_number_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("number")
}

fn is_integer_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("integer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coerce_string_to_number() {
        let schema = json!({"type": "number"});
        assert_eq!(coerce_with_schema(json!("42"), &schema), json!(42));
    }

    #[test]
    fn coerce_string_to_boolean() {
        let schema = json!({"type": "boolean"});
        assert_eq!(coerce_with_schema(json!("true"), &schema), json!(true));
    }

    #[test]
    fn coerce_string_to_null() {
        let schema = json!({"type": "null"});
        assert_eq!(coerce_with_schema(json!("null"), &schema), json!(null));
    }

    #[test]
    fn coerce_string_to_object() {
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        let result = coerce_with_schema(json!(r#"{"name": "test"}"#), &schema);
        assert_eq!(result["name"], "test");
    }

    #[test]
    fn coerce_nested_object() {
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}, "age": {"type": "number"}}});
        let result = coerce_with_schema(json!({"name": "test", "age": "25"}), &schema);
        assert_eq!(result["age"], json!(25));
    }

    #[test]
    fn coerce_array_items() {
        let schema = json!({"type": "array", "items": {"type": "number"}});
        assert_eq!(
            coerce_with_schema(json!(["1", "2", "3"]), &schema),
            json!([1, 2, 3])
        );
    }

    #[test]
    fn no_coerce_string_when_string_schema() {
        let schema = json!({"type": "string"});
        assert_eq!(coerce_with_schema(json!("42"), &schema), json!("42"));
    }

    #[test]
    fn coerce_with_ref() {
        let schema = json!({"$ref": "#/$defs/MyType", "$defs": {"MyType": {"type": "number"}}});
        assert_eq!(coerce_with_schema(json!("42"), &schema), json!(42));
    }

    // --- NEW: 10가지 기계적 교정 ---

    #[test]
    fn enum_case_insensitive() {
        let schema = json!({"type": "string", "enum": ["admin", "user", "guest"]});
        assert_eq!(coerce_with_schema(json!("Admin"), &schema), json!("admin"));
        assert_eq!(coerce_with_schema(json!("GUEST"), &schema), json!("guest"));
    }

    #[test]
    fn single_value_to_array() {
        let schema = json!({"type": "array", "items": {"type": "string"}});
        assert_eq!(coerce_with_schema(json!("tag"), &schema), json!(["tag"]));
    }

    #[test]
    fn default_fill_missing() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "role": {"type": "string", "default": "user"}
            }
        });
        let result = coerce_with_schema(json!({"name": "test"}), &schema);
        assert_eq!(result["role"], "user");
    }

    #[test]
    fn null_replaced_by_default() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": {"type": "integer", "default": 0}
            }
        });
        let result = coerce_with_schema(json!({"count": null}), &schema);
        assert_eq!(result["count"], 0);
    }

    #[test]
    fn string_trim() {
        let schema = json!({"type": "string"});
        assert_eq!(
            coerce_with_schema(json!("  hello  "), &schema),
            json!("hello")
        );
    }

    #[test]
    fn number_with_comma() {
        let schema = json!({"type": "number"});
        assert_eq!(coerce_with_schema(json!("1,000"), &schema), json!(1000));
    }

    #[test]
    fn number_with_k_suffix() {
        let schema = json!({"type": "number"});
        assert_eq!(coerce_with_schema(json!("1.5k"), &schema), json!(1500.0));
    }

    #[test]
    fn indexed_object_to_array() {
        let schema = json!({"type": "array", "items": {"type": "string"}});
        let input = json!({"0": "a", "1": "b", "2": "c"});
        assert_eq!(coerce_with_schema(input, &schema), json!(["a", "b", "c"]));
    }

    #[test]
    fn enum_number_to_string() {
        let schema = json!({"type": "string", "enum": ["1", "2", "3"]});
        assert_eq!(coerce_with_schema(json!(1), &schema), json!("1"));
    }

    #[test]
    fn enum_string_to_number() {
        let schema = json!({"type": "integer", "enum": [1, 2, 3]});
        assert_eq!(coerce_with_schema(json!("2"), &schema), json!(2));
    }

    #[test]
    fn number_to_string_for_string_schema() {
        let schema = json!({"type": "string"});
        assert_eq!(coerce_with_schema(json!(42), &schema), json!("42"));
    }

    #[test]
    fn bool_to_string_for_string_schema() {
        let schema = json!({"type": "string"});
        assert_eq!(coerce_with_schema(json!(true), &schema), json!("true"));
    }
}
