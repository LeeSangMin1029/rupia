use serde_json::Value;

use crate::lenient;
use crate::schema_util;
use crate::types::ParseResult;

#[derive(Debug, Clone, PartialEq)]
pub struct CoercionLog {
    pub field: String,
    pub original: Value,
    pub coerced: Value,
    pub coercion_type: String,
}

pub fn coerce_with_schema(value: Value, schema: &Value) -> Value {
    coerce_value(value, schema, schema, 0)
}

pub fn coerce_with_schema_logged(value: Value, schema: &Value) -> (Value, Vec<CoercionLog>) {
    let original = value.clone();
    let coerced = coerce_with_schema(value, schema);
    let mut logs = Vec::new();
    diff_values(&original, &coerced, "", &mut logs);
    (coerced, logs)
}

fn diff_values(original: &Value, coerced: &Value, path: &str, logs: &mut Vec<CoercionLog>) {
    if original == coerced {
        return;
    }
    match (original, coerced) {
        (Value::Object(orig_obj), Value::Object(coerced_obj)) => {
            for (key, coerced_val) in coerced_obj {
                let field_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match orig_obj.get(key) {
                    Some(orig_val) => diff_values(orig_val, coerced_val, &field_path, logs),
                    None => {
                        logs.push(CoercionLog {
                            field: field_path,
                            original: Value::Null,
                            coerced: coerced_val.clone(),
                            coercion_type: "default_fill".into(),
                        });
                    }
                }
            }
        }
        (Value::Array(orig_arr), Value::Array(coerced_arr)) => {
            for (i, (o, c)) in orig_arr.iter().zip(coerced_arr.iter()).enumerate() {
                let field_path = if path.is_empty() {
                    format!("[{i}]")
                } else {
                    format!("{path}[{i}]")
                };
                diff_values(o, c, &field_path, logs);
            }
        }
        _ => {
            let coercion_type = infer_coercion_type(original, coerced);
            logs.push(CoercionLog {
                field: path.to_owned(),
                original: original.clone(),
                coerced: coerced.clone(),
                coercion_type,
            });
        }
    }
}

fn infer_coercion_type(original: &Value, coerced: &Value) -> String {
    match (original, coerced) {
        (Value::String(_), Value::Number(n)) => {
            if n.is_i64() || n.is_u64() {
                "string_to_int".into()
            } else {
                "string_to_number".into()
            }
        }
        (Value::String(_), Value::Bool(_)) => "string_to_bool".into(),
        (Value::String(_), Value::Null) => "string_to_null".into(),
        (Value::String(_), Value::Object(_)) => "string_to_object".into(),
        (Value::String(_), Value::Array(_)) => "string_to_array".into(),
        (Value::String(a), Value::String(b)) => {
            if a.to_lowercase() == b.to_lowercase() {
                "enum_case".into()
            } else if a.trim() == b.as_str() {
                "string_trim".into()
            } else {
                "string_transform".into()
            }
        }
        (Value::Number(_), Value::String(_)) => "number_to_string".into(),
        (Value::Bool(_), Value::String(_)) => "bool_to_string".into(),
        (Value::Null, _) => "default_fill".into(),
        _ => "unknown".into(),
    }
}

fn coerce_value(value: Value, schema: &Value, root: &Value, depth: u32) -> Value {
    let schema = schema_util::resolve_schema(schema, root);
    if schema.get("$ref").is_some() {
        return value;
    }
    if let Some(any_of) = schema.get("anyOf").and_then(Value::as_array) {
        return coerce_any_of(value, any_of, root, depth);
    }
    if let Some(one_of) = schema.get("oneOf").and_then(Value::as_array) {
        return coerce_any_of(value, one_of, root, depth);
    }
    if schema.get("const").is_some() {
        return value;
    }
    if is_string_schema(schema) {
        return coerce_to_string(value, schema);
    }
    if is_array_schema(schema) {
        return coerce_to_array(value, schema, root, depth);
    }
    if let Value::String(s) = &value {
        if let Some(coerced) = try_coerce_string(s, schema, root, depth) {
            return coerced;
        }
        return value;
    }
    if let Value::Array(arr) = value {
        if is_array_schema(schema) {
            return coerce_array_items(arr, schema, root, depth);
        }
        return Value::Array(arr);
    }
    if let Value::Object(obj) = value {
        if is_object_schema(schema) {
            return coerce_object(obj, schema, root, depth);
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

fn try_coerce_string(s: &str, schema: &Value, root: &Value, depth: u32) -> Option<Value> {
    if let Some(enum_vals) = schema.get("enum").and_then(Value::as_array) {
        return coerce_enum_value(&Value::String(s.to_owned()), enum_vals);
    }
    if is_number_schema(schema) || is_integer_schema(schema) {
        if let Some(n) = parse_number_string(s) {
            return Some(n);
        }
    }
    if let ParseResult::Success(parsed) = lenient::parse(s) {
        return Some(coerce_value(parsed, schema, root, depth));
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

fn coerce_to_array(value: Value, schema: &Value, root: &Value, depth: u32) -> Value {
    match value {
        Value::Array(arr) => coerce_array_items(arr, schema, root, depth),
        Value::Object(ref obj) => {
            if let Some(arr) = try_indexed_object_to_array(obj) {
                return coerce_array_items(arr, schema, root, depth);
            }
            Value::Array(vec![coerce_value(
                value,
                &schema
                    .get("items")
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::default())),
                root,
                depth,
            )])
        }
        _ => {
            let items_schema = schema
                .get("items")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::default()));
            Value::Array(vec![coerce_value(value, &items_schema, root, depth)])
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

fn coerce_array_items(arr: Vec<Value>, schema: &Value, root: &Value, depth: u32) -> Value {
    let items_schema = schema
        .get("items")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::default()));
    Value::Array(
        arr.into_iter()
            .map(|item| coerce_value(item, &items_schema, root, depth))
            .collect(),
    )
}

fn coerce_object(
    obj: serde_json::Map<String, Value>,
    schema: &Value,
    root: &Value,
    depth: u32,
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
            result.insert(
                key.clone(),
                coerce_value(val.clone(), prop_schema, root, depth),
            );
        } else if let Some(default) = prop_schema.get("default") {
            result.insert(key.clone(), default.clone());
        }
    }
    let additional = schema
        .get("additionalProperties")
        .and_then(|v| if v.is_object() { Some(v) } else { None });
    for (key, val) in &obj {
        if !properties.contains_key(key) {
            let coerced = match additional {
                Some(s) => coerce_value(val.clone(), s, root, depth),
                None => val.clone(),
            };
            result.insert(key.clone(), coerced);
        }
    }
    Value::Object(result)
}

fn coerce_any_of(value: Value, variants: &[Value], root: &Value, depth: u32) -> Value {
    if let Value::String(ref s) = value {
        let has_string = variants
            .iter()
            .any(|v| is_string_schema(schema_util::resolve_schema(v, root)));
        if has_string {
            return value;
        }
        if let ParseResult::Success(parsed) = lenient::parse(s) {
            if let Some(matched) = find_matching_schema(&parsed, variants, root, depth) {
                return coerce_value(parsed, matched, root, depth);
            }
            return parsed;
        }
        if s.len() == 1 && s.eq_ignore_ascii_case("n") {
            let has_bool = variants
                .iter()
                .any(|v| is_boolean_schema(schema_util::resolve_schema(v, root)));
            let has_null = variants
                .iter()
                .any(|v| is_null_schema(schema_util::resolve_schema(v, root)));
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
        if let Some(matched) = find_matching_schema(&value, variants, root, depth) {
            return coerce_value(value, matched, root, depth);
        }
        return value;
    }
    if let Value::Array(_) = value {
        let array_schemas: Vec<&Value> = variants
            .iter()
            .filter(|v| is_array_schema(schema_util::resolve_schema(v, root)))
            .collect();
        if array_schemas.len() == 1 {
            return coerce_value(value, array_schemas[0], root, depth);
        }
        return value;
    }
    value
}

fn find_matching_schema<'a>(
    value: &Value,
    variants: &'a [Value],
    root: &Value,
    _depth: u32,
) -> Option<&'a Value> {
    let matching: Vec<&Value> = variants
        .iter()
        .filter(|s| matches_type(value, schema_util::resolve_schema(s, root)))
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

fn schema_is(schema: &Value, ty: &str) -> bool {
    schema.get("type").and_then(Value::as_str) == Some(ty)
}
fn is_string_schema(s: &Value) -> bool {
    schema_is(s, "string")
}
fn is_boolean_schema(s: &Value) -> bool {
    schema_is(s, "boolean")
}
fn is_null_schema(s: &Value) -> bool {
    schema_is(s, "null")
}
fn is_array_schema(s: &Value) -> bool {
    schema_is(s, "array")
}
fn is_object_schema(s: &Value) -> bool {
    schema_is(s, "object")
}
fn is_number_schema(s: &Value) -> bool {
    schema_is(s, "number")
}
fn is_integer_schema(s: &Value) -> bool {
    schema_is(s, "integer")
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

    #[test]
    fn circular_ref_returns_original_value() {
        let schema = json!({
            "$ref": "#/$defs/A",
            "$defs": { "A": { "$ref": "#/$defs/A" } }
        });
        assert_eq!(coerce_with_schema(json!("hello"), &schema), json!("hello"));
    }

    #[test]
    fn const_skips_coercion() {
        let schema = json!({"const": 42});
        assert_eq!(coerce_with_schema(json!("42"), &schema), json!("42"));
        assert_eq!(coerce_with_schema(json!(42), &schema), json!(42));
    }

    #[test]
    fn coerce_logged_tracks_changes() {
        let schema = json!({
            "type": "object",
            "properties": {
                "age": {"type": "integer"},
                "role": {"type": "string", "enum": ["admin", "user", "guest"]}
            },
            "required": ["age", "role"]
        });
        let input = json!({"age": "25", "role": "Admin"});
        let (result, logs) = coerce_with_schema_logged(input, &schema);
        assert_eq!(result["age"], json!(25));
        assert_eq!(result["role"], json!("admin"));
        assert_eq!(logs.len(), 2);
        let age_log = logs.iter().find(|l| l.field == "age").expect("age log");
        assert_eq!(age_log.original, json!("25"));
        assert_eq!(age_log.coerced, json!(25));
        assert_eq!(age_log.coercion_type, "string_to_int");
        let role_log = logs.iter().find(|l| l.field == "role").expect("role log");
        assert_eq!(role_log.original, json!("Admin"));
        assert_eq!(role_log.coerced, json!("admin"));
        assert_eq!(role_log.coercion_type, "enum_case");
    }

    #[test]
    fn coerce_logged_no_changes() {
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        let input = json!({"name": "test"});
        let (result, logs) = coerce_with_schema_logged(input.clone(), &schema);
        assert_eq!(result, input);
        assert!(logs.is_empty());
    }

    #[test]
    fn default_violating_schema_is_filled_anyway() {
        let schema = json!({
            "type":"object",
            "properties":{
                "count":{"type":"integer","minimum":10,"default":5}
            }
        });
        let result = coerce_with_schema(json!({}), &schema);
        assert_eq!(result["count"], json!(5));
    }
}
