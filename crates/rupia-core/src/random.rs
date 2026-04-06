use serde_json::Value;

use crate::schema_util;

#[expect(clippy::cast_possible_truncation, reason = "schema values always fit")]
fn usize_from_schema(schema: &Value, key: &str, default: usize) -> usize {
    schema
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(default as u64) as usize
}

pub fn generate(schema: &Value) -> Value {
    generate_value(schema, schema, 0)
}

const MAX_DEPTH: usize = 32;

fn generate_value(schema: &Value, root: &Value, depth: usize) -> Value {
    if depth > MAX_DEPTH {
        return Value::Null;
    }
    let schema = schema_util::resolve_schema(schema, root);
    if schema.get("$ref").is_some() {
        return Value::Null;
    }
    if let Some(enum_vals) = schema.get("enum").and_then(Value::as_array) {
        if !enum_vals.is_empty() {
            return enum_vals[rand_usize() % enum_vals.len()].clone();
        }
    }
    if let Some(const_val) = schema.get("const") {
        return const_val.clone();
    }
    if let Some(variants) = schema.get("anyOf").or_else(|| schema.get("oneOf")) {
        if let Some(arr) = variants.as_array() {
            if !arr.is_empty() {
                return generate_value(&arr[rand_usize() % arr.len()], root, depth + 1);
            }
        }
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("boolean") => Value::Bool(rand_usize() % 2 == 0),
        Some("integer") => gen_integer(schema),
        Some("number") => gen_number(schema),
        Some("string") => gen_string(schema),
        Some("array") => gen_array(schema, root, depth),
        Some("object") => gen_object(schema, root, depth),
        // "null" and unknown types
        _ => Value::Null,
    }
}

fn gen_integer(schema: &Value) -> Value {
    let min = schema.get("minimum").and_then(Value::as_i64).unwrap_or(0);
    let max = schema.get("maximum").and_then(Value::as_i64).unwrap_or(100);
    let range = (max - min + 1).max(1);
    #[expect(clippy::cast_possible_wrap, reason = "rand output fits")]
    let val = min + (rand_usize() as i64).abs() % range;
    Value::Number(val.into())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "acceptable for random generation"
)]
fn gen_number(schema: &Value) -> Value {
    let min = schema.get("minimum").and_then(Value::as_f64).unwrap_or(0.0);
    let max = schema
        .get("maximum")
        .and_then(Value::as_f64)
        .unwrap_or(100.0);
    let frac = (rand_usize() as f64) / f64::from(u32::MAX);
    let val = min + frac * (max - min);
    let rounded = (val * 100.0).round() / 100.0;
    serde_json::Number::from_f64(rounded).map_or(Value::Number(0.into()), Value::Number)
}

fn gen_string(schema: &Value) -> Value {
    if let Some(fmt) = schema.get("format").and_then(Value::as_str) {
        return Value::String(gen_format_string(fmt));
    }
    let min_len = usize_from_schema(schema, "minLength", 1);
    let max_len = usize_from_schema(schema, "maxLength", 20);
    let len = min_len + rand_usize() % (max_len - min_len + 1).max(1);
    let chars: String = (0..len)
        .map(|_| {
            let offset: u8 = (rand_usize() % 26).try_into().unwrap_or(0);
            char::from(b'a' + offset)
        })
        .collect();
    Value::String(chars)
}

fn gen_format_string(fmt: &str) -> String {
    let n = rand_usize() % 10000;
    match fmt {
        "email" | "idn-email" => format!("user{n}@example.com"),
        "uuid" => {
            let hi = rand_usize();
            let lo = rand_usize();
            format!(
                "{:08x}-{:04x}-4{:03x}-{:04x}-{:08x}{:04x}",
                hi & 0xFFFF_FFFF,
                (hi >> 32) & 0xFFFF,
                lo & 0xFFF,
                0x8000 | ((lo >> 12) & 0x3FFF),
                rand_usize() & 0xFFFF_FFFF,
                rand_usize() & 0xFFFF,
            )
        }
        "date-time" | "datetime" => {
            let yr = 2020 + rand_usize() % 5;
            let mo = 1 + rand_usize() % 12;
            let dy = 1 + rand_usize() % 28;
            let hr = rand_usize() % 24;
            let mi = rand_usize() % 60;
            let se = rand_usize() % 60;
            format!("{yr:04}-{mo:02}-{dy:02}T{hr:02}:{mi:02}:{se:02}Z")
        }
        "date" => {
            let yr = 2020 + rand_usize() % 5;
            let mo = 1 + rand_usize() % 12;
            let dy = 1 + rand_usize() % 28;
            format!("{yr:04}-{mo:02}-{dy:02}")
        }
        "time" => {
            let hr = rand_usize() % 24;
            let mi = rand_usize() % 60;
            let se = rand_usize() % 60;
            format!("{hr:02}:{mi:02}:{se:02}Z")
        }
        "duration" => {
            let days = 1 + rand_usize() % 30;
            let hrs = rand_usize() % 24;
            format!("P{days}DT{hrs}H")
        }
        "ipv4" => format!(
            "{}.{}.{}.{}",
            1 + rand_usize() % 254,
            rand_usize() % 256,
            rand_usize() % 256,
            1 + rand_usize() % 254,
        ),
        "ipv6" => {
            let segs: Vec<String> = (0..8)
                .map(|_| format!("{:04x}", rand_usize() % 0xFFFF))
                .collect();
            segs.join(":")
        }
        "hostname" | "idn-hostname" => format!("host{n}.example.com"),
        "uri" | "iri" | "iri-reference" => format!("https://example.com/resource/{n}"),
        "url" => format!("https://example.com/page/{n}"),
        "uri-reference" | "json-pointer" => format!("/path/{n}"),
        "uri-template" => format!("https://example.com/items/{n}"),
        "relative-json-pointer" => format!("0/path/{n}"),
        "byte" => base64_encode(&format!("data-{n}")),
        "regex" => r"^[a-z]+$".into(),
        _ => format!("P@ss{n}word!"),
    }
}

fn base64_encode(input: &str) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[((b0 & 3) << 4 | b1 >> 4) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((b1 & 0xF) << 2 | b2 >> 6) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(b2 & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn gen_array(schema: &Value, root: &Value, depth: usize) -> Value {
    let min = usize_from_schema(schema, "minItems", 1);
    let max = usize_from_schema(schema, "maxItems", 3);
    let count = min + rand_usize() % (max - min + 1).max(1);
    let items_schema = schema
        .get("items")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::default()));
    Value::Array(
        (0..count)
            .map(|_| generate_value(&items_schema, root, depth + 1))
            .collect(),
    )
}

fn gen_object(schema: &Value, root: &Value, depth: usize) -> Value {
    let mut map = serde_json::Map::new();
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (key, prop_schema) in properties {
            map.insert(key.clone(), generate_value(prop_schema, root, depth + 1));
        }
    }
    Value::Object(map)
}

fn rand_usize() -> usize {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    #[expect(clippy::cast_possible_truncation, reason = "hash to usize")]
    {
        RandomState::new().build_hasher().finish() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_object() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer", "minimum": 0, "maximum": 100}
            }
        });
        let result = generate(&schema);
        assert!(result.is_object());
        assert!(result["name"].is_string());
        let age = result["age"].as_i64().unwrap();
        assert!((0..=100).contains(&age));
    }

    #[test]
    fn with_enum() {
        let schema = json!({"type": "string", "enum": ["admin", "user", "guest"]});
        let s = generate(&schema).as_str().unwrap().to_owned();
        assert!(["admin", "user", "guest"].contains(&s.as_str()));
    }

    #[test]
    fn format_email() {
        let schema = json!({"type": "string", "format": "email"});
        let s = generate(&schema).as_str().unwrap().to_owned();
        assert!(crate::format::validate(&s, "email"));
    }

    #[test]
    fn format_uuid() {
        let schema = json!({"type": "string", "format": "uuid"});
        let s = generate(&schema).as_str().unwrap().to_owned();
        assert!(crate::format::validate(&s, "uuid"), "invalid uuid: {s}");
    }

    #[test]
    fn format_datetime() {
        let schema = json!({"type": "string", "format": "date-time"});
        let s = generate(&schema).as_str().unwrap().to_owned();
        assert!(crate::format::validate(&s, "date-time"), "invalid: {s}");
    }

    #[test]
    fn format_ipv4() {
        let schema = json!({"type": "string", "format": "ipv4"});
        let s = generate(&schema).as_str().unwrap().to_owned();
        assert!(crate::format::validate(&s, "ipv4"), "invalid: {s}");
    }

    #[test]
    fn array_with_bounds() {
        let schema = json!({"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 10}, "minItems": 2, "maxItems": 5});
        let arr = generate(&schema).as_array().unwrap().clone();
        assert!(arr.len() >= 2 && arr.len() <= 5);
    }

    #[test]
    fn nested_ref() {
        let schema = json!({
            "type": "object",
            "properties": {"child": {"$ref": "#/$defs/Child"}},
            "$defs": {"Child": {"type": "object", "properties": {"name": {"type": "string"}}}}
        });
        assert!(generate(&schema)["child"]["name"].is_string());
    }

    #[test]
    fn boolean_and_null() {
        assert!(generate(&json!({"type": "boolean"})).is_boolean());
        assert!(generate(&json!({"type": "null"})).is_null());
    }
}
