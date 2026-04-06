use serde_json::Value;

use crate::types::{Validation, ValidationError, ValidationFailure};

pub fn validate(value: &Value, schema: &Value) -> Validation<Value> {
    validate_with_options(value, schema, false)
}

pub fn validate_strict(value: &Value, schema: &Value) -> Validation<Value> {
    validate_with_options(value, schema, true)
}

const MAX_REF_DEPTH: u32 = 64;

fn validate_with_options(value: &Value, schema: &Value, equals: bool) -> Validation<Value> {
    let defs = schema.get("$defs").or_else(|| schema.get("definitions"));
    let mut errors = Vec::new();
    validate_value(value, schema, defs, "$input", true, equals, &mut errors, 0);
    if errors.is_empty() {
        Validation::Success(value.clone())
    } else {
        Validation::Failure(ValidationFailure {
            data: value.clone(),
            errors,
        })
    }
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "depth tracking adds 1 param"
)]
fn validate_value(
    value: &Value,
    schema: &Value,
    defs: Option<&Value>,
    path: &str,
    required: bool,
    equals: bool,
    errors: &mut Vec<ValidationError>,
    depth: u32,
) {
    if schema.get("type").is_none()
        && schema.get("$ref").is_none()
        && schema.get("anyOf").is_none()
        && schema.get("oneOf").is_none()
        && schema.get("allOf").is_none()
        && schema.get("not").is_none()
        && schema.get("const").is_none()
        && schema.get("enum").is_none()
        && schema.get("if").is_none()
        && schema.get("properties").is_none()
        && schema.get("required").is_none()
    {
        return;
    }
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        if depth > MAX_REF_DEPTH {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: "non-circular $ref".into(),
                value: value.clone(),
                description: Some(format!("$ref depth exceeded {MAX_REF_DEPTH}")),
            });
            return;
        }
        if !r.starts_with("#/") {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: "resolvable $ref".into(),
                value: value.clone(),
                description: Some(format!("External $ref not supported: {r}")),
            });
            return;
        }
        let key = r
            .strip_prefix("#/$defs/")
            .or_else(|| r.strip_prefix("#/definitions/"));
        if let Some(resolved) = key.and_then(|k| defs?.get(k)) {
            validate_value(
                value,
                resolved,
                defs,
                path,
                required,
                equals,
                errors,
                depth + 1,
            );
            return;
        }
        return;
    }
    if let Some(variants) = schema.get("oneOf").and_then(Value::as_array) {
        let discriminator = schema.get("x-discriminator");
        validate_one_of(
            value,
            variants,
            discriminator,
            defs,
            path,
            required,
            equals,
            errors,
            depth,
            true,
        );
        return;
    }
    if let Some(variants) = schema.get("anyOf").and_then(Value::as_array) {
        let discriminator = schema.get("x-discriminator");
        validate_one_of(
            value,
            variants,
            discriminator,
            defs,
            path,
            required,
            equals,
            errors,
            depth,
            false,
        );
        return;
    }
    if let Some(all) = schema.get("allOf").and_then(Value::as_array) {
        for sub in all {
            validate_value(value, sub, defs, path, required, equals, errors, depth);
        }
        return;
    }
    if let Some(not_schema) = schema.get("not") {
        let mut sub_errors = Vec::new();
        validate_value(
            value,
            not_schema,
            defs,
            path,
            required,
            equals,
            &mut sub_errors,
            depth,
        );
        if sub_errors.is_empty() {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: "value NOT matching the sub-schema".into(),
                value: value.clone(),
                description: None,
            });
        }
        return;
    }
    if let Some(if_schema) = schema.get("if") {
        let mut if_errors = Vec::new();
        validate_value(
            value,
            if_schema,
            defs,
            path,
            required,
            equals,
            &mut if_errors,
            depth,
        );
        if if_errors.is_empty() {
            if let Some(then_schema) = schema.get("then") {
                validate_value(
                    value,
                    then_schema,
                    defs,
                    path,
                    required,
                    equals,
                    errors,
                    depth,
                );
            }
        } else if let Some(else_schema) = schema.get("else") {
            validate_value(
                value,
                else_schema,
                defs,
                path,
                required,
                equals,
                errors,
                depth,
            );
        }
        if schema.get("type").is_none() {
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
        Value::Array(arr) => validate_array(arr, schema, defs, path, equals, errors, depth),
        Value::Object(obj) => validate_object(obj, schema, defs, path, equals, errors, depth),
    }
}

fn resolve_variant<'a>(schema: &'a Value, defs: Option<&'a Value>, depth: u32) -> &'a Value {
    if depth > MAX_REF_DEPTH {
        return schema;
    }
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let key = r
            .strip_prefix("#/$defs/")
            .or_else(|| r.strip_prefix("#/definitions/"));
        if let Some(resolved) = key.and_then(|k| defs?.get(k)) {
            return resolve_variant(resolved, defs, depth + 1);
        }
    }
    schema
}

#[expect(
    clippy::too_many_arguments,
    reason = "discriminator + strict_one adds params"
)]
fn validate_one_of(
    value: &Value,
    variants: &[Value],
    discriminator: Option<&Value>,
    defs: Option<&Value>,
    path: &str,
    required: bool,
    equals: bool,
    errors: &mut Vec<ValidationError>,
    depth: u32,
    strict_one: bool,
) {
    if let Some(disc) = discriminator {
        if let Some(prop_name) = disc.get("propertyName").and_then(Value::as_str) {
            if let Some(disc_val) = value.as_object().and_then(|o| o.get(prop_name)) {
                if let Some(mapping) = disc.get("mapping").and_then(Value::as_object) {
                    if let Some(disc_str) = disc_val.as_str() {
                        if let Some(ref_val) = mapping.get(disc_str).and_then(Value::as_str) {
                            for variant in variants {
                                if variant.get("$ref").and_then(Value::as_str) == Some(ref_val) {
                                    validate_value(
                                        value, variant, defs, path, required, equals, errors, depth,
                                    );
                                    return;
                                }
                            }
                        }
                    }
                }
                for variant in variants {
                    let resolved = resolve_variant(variant, defs, depth);
                    if let Some(props) = resolved.get("properties").and_then(Value::as_object) {
                        if let Some(prop_schema) = props.get(prop_name) {
                            if let Some(enums) = prop_schema.get("enum").and_then(Value::as_array) {
                                if enums.contains(disc_val) {
                                    validate_value(
                                        value, variant, defs, path, required, equals, errors, depth,
                                    );
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let mut match_count = 0u32;
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
            depth,
        );
        if sub_errors.is_empty() {
            match_count += 1;
            if !strict_one {
                return;
            }
        }
    }
    if strict_one && match_count == 1 {
        return;
    }
    let expected: Vec<String> = variants.iter().map(expected_type).collect();
    let desc = if strict_one && match_count > 1 {
        Some(format!(
            "oneOf: expected exactly 1 match but found {match_count}"
        ))
    } else {
        None
    };
    errors.push(ValidationError {
        path: path.to_owned(),
        expected: format!("({})", expected.join(" | ")),
        value: value.clone(),
        description: desc,
    });
}

fn validate_number(
    n: &serde_json::Number,
    schema: &Value,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    let is_integer = is_type(schema, "integer");
    let is_number = is_type(schema, "number");
    if !is_integer && !is_number {
        errors.push(ValidationError {
            path: path.to_owned(),
            expected: expected_type(schema),
            value: Value::Number(n.clone()),
            description: None,
        });
        return;
    }
    if is_integer && !is_number && n.as_i64().is_none() && n.as_u64().is_none() {
        errors.push(ValidationError {
            path: path.to_owned(),
            expected: "integer".into(),
            value: Value::Number(n.clone()),
            description: None,
        });
        return;
    }
    let ty = if is_integer && (n.as_i64().is_some() || n.as_u64().is_some()) {
        "integer"
    } else {
        "number"
    };
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
    depth: u32,
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
    if schema.get("uniqueItems").and_then(Value::as_bool) == Some(true) && !is_unique(arr) {
        errors.push(ValidationError {
            path: path.to_owned(),
            expected: "array & UniqueItems".into(),
            value: Value::Array(arr.to_vec()),
            description: Some("Array contains duplicate elements.".into()),
        });
    }
    if let Some(items_schema) = schema.get("items") {
        if let Some(tuple_schemas) = items_schema.as_array() {
            for (i, item) in arr.iter().enumerate() {
                if let Some(ith_schema) = tuple_schemas.get(i) {
                    validate_value(
                        item,
                        ith_schema,
                        defs,
                        &format!("{path}[{i}]"),
                        true,
                        equals,
                        errors,
                        depth,
                    );
                }
            }
        } else {
            for (i, item) in arr.iter().enumerate() {
                validate_value(
                    item,
                    items_schema,
                    defs,
                    &format!("{path}[{i}]"),
                    true,
                    equals,
                    errors,
                    depth,
                );
            }
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "patternProperties + additionalProperties checks"
)]
fn validate_object(
    obj: &serde_json::Map<String, Value>,
    schema: &Value,
    defs: Option<&Value>,
    path: &str,
    equals: bool,
    errors: &mut Vec<ValidationError>,
    depth: u32,
) {
    let has_implicit_object = schema.get("properties").is_some()
        || schema.get("required").is_some()
        || schema.get("patternProperties").is_some()
        || schema.get("additionalProperties").is_some();
    if !is_type(schema, "object") && !has_implicit_object {
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
    if let Some(min_props) = schema.get("minProperties").and_then(Value::as_u64) {
        if (obj.len() as u64) < min_props {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("object & MinProperties<{min_props}>"),
                value: Value::Object(obj.clone()),
                description: None,
            });
        }
    }
    if let Some(max_props) = schema.get("maxProperties").and_then(Value::as_u64) {
        if obj.len() as u64 > max_props {
            errors.push(ValidationError {
                path: path.to_owned(),
                expected: format!("object & MaxProperties<{max_props}>"),
                value: Value::Object(obj.clone()),
                description: None,
            });
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
                    depth,
                );
            }
        }
    }
    if let Some(pp) = schema.get("patternProperties").and_then(Value::as_object) {
        for (key, val) in obj {
            for (pattern, pat_schema) in pp {
                if let Ok(re) = regex_lite::Regex::new(pattern) {
                    if re.is_match(key) {
                        validate_value(
                            val,
                            pat_schema,
                            defs,
                            &format!("{path}.{key}"),
                            true,
                            equals,
                            errors,
                            depth,
                        );
                    }
                }
            }
        }
    }
    let additional_properties_false =
        schema.get("additionalProperties").and_then(Value::as_bool) == Some(false);
    if equals || additional_properties_false {
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
    match schema.get("type") {
        Some(Value::String(s)) => s == ty,
        Some(Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some(ty)),
        _ => false,
    }
}

fn expected_type(schema: &Value) -> String {
    match schema.get("type") {
        Some(Value::String(ty)) => return ty.clone(),
        Some(Value::Array(arr)) => {
            let types: Vec<&str> = arr.iter().filter_map(Value::as_str).collect();
            if !types.is_empty() {
                return format!("({})", types.join(" | "));
            }
        }
        _ => {}
    }
    if schema.get("$ref").is_some() {
        return "referenced type".into();
    }
    if schema.get("anyOf").is_some() || schema.get("oneOf").is_some() {
        return "union type".into();
    }
    "unknown".into()
}

fn is_unique(arr: &[Value]) -> bool {
    if arr.len() < 2 {
        return true;
    }
    for i in 0..arr.len() {
        for j in (i + 1)..arr.len() {
            if arr[i] == arr[j] {
                return false;
            }
        }
    }
    true
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
        assert!(!result.is_success());
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
        assert!(!result.is_success());
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
        assert!(validate(&json!(42), &schema).is_success());
        assert!(validate(&json!(3.15), &schema).is_success());
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
            "if":{"properties":{"status":{"const":"shipped"}}},
            "then":{"required":["tracking_number"]}
        });
        assert!(!validate(&json!({"status":"shipped"}), &schema).is_success());
        assert!(validate(
            &json!({"status":"shipped","tracking_number":"123"}),
            &schema
        )
        .is_success());
        assert!(validate(&json!({"status":"pending"}), &schema).is_success());
    }

    #[test]
    fn external_ref_produces_error() {
        let schema = json!({"$ref":"https://example.com/schema.json"});
        let result = validate(&json!("anything"), &schema);
        assert!(!result.is_success());
        if let Validation::Failure(f) = result {
            assert!(f.errors[0]
                .description
                .as_ref()
                .is_some_and(|d| d.contains("External $ref not supported")));
        }
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
            "items":[{"type":"string"},{"type":"integer"}]
        });
        assert!(validate(&json!(["a", 1]), &schema).is_success());
        assert!(!validate(&json!(["a", "b"]), &schema).is_success());
    }
}
