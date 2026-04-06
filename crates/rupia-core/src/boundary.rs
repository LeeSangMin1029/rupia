use serde_json::{json, Value};

pub struct BoundaryCase {
    pub field: String,
    pub value: Value,
    pub description: String,
    pub expected_valid: Option<bool>,
}

pub fn generate_boundary_cases(schema: &Value) -> Vec<BoundaryCase> {
    let mut cases = vec![];
    let properties = schema.get("properties").and_then(Value::as_object);
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some(props) = properties else {
        return cases;
    };
    for (field, prop_schema) in props {
        let typ = prop_schema
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("");
        match typ {
            "integer" | "number" => {
                gen_numeric_bounds(&mut cases, field, prop_schema, typ);
            }
            "string" => {
                gen_string_bounds(&mut cases, field, prop_schema);
            }
            "boolean" => {
                cases.push(BoundaryCase {
                    field: field.clone(),
                    value: json!(true),
                    description: format!("{field}=true"),
                    expected_valid: Some(true),
                });
                cases.push(BoundaryCase {
                    field: field.clone(),
                    value: json!(false),
                    description: format!("{field}=false"),
                    expected_valid: Some(true),
                });
            }
            "array" => {
                gen_array_bounds(&mut cases, field, prop_schema);
            }
            _ => {}
        }
        if required.contains(&field.as_str()) {
            cases.push(BoundaryCase {
                field: field.clone(),
                value: json!("present_value"),
                description: format!("{field} present (required)"),
                expected_valid: Some(true),
            });
            cases.push(BoundaryCase {
                field: field.clone(),
                value: Value::Null,
                description: format!("{field} absent/null (required)"),
                expected_valid: Some(false),
            });
        }
    }
    cases
}

fn gen_numeric_bounds(cases: &mut Vec<BoundaryCase>, field: &str, schema: &Value, typ: &str) {
    if let Some(min) = schema.get("minimum").and_then(Value::as_i64) {
        #[expect(clippy::cast_precision_loss, reason = "schema boundary values fit")]
        let at = if typ == "integer" {
            json!(min)
        } else {
            json!(min as f64)
        };
        cases.push(BoundaryCase {
            field: field.into(),
            value: at,
            description: format!("{field}=minimum({min})"),
            expected_valid: Some(true),
        });
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!(min - 1),
            description: format!("{field}=minimum-1({val})", val = min - 1),
            expected_valid: Some(false),
        });
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!(min + 1),
            description: format!("{field}=minimum+1({val})", val = min + 1),
            expected_valid: Some(true),
        });
    }
    if let Some(max) = schema.get("maximum").and_then(Value::as_i64) {
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!(max),
            description: format!("{field}=maximum({max})"),
            expected_valid: Some(true),
        });
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!(max + 1),
            description: format!("{field}=maximum+1({val})", val = max + 1),
            expected_valid: Some(false),
        });
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!(max - 1),
            description: format!("{field}=maximum-1({val})", val = max - 1),
            expected_valid: Some(true),
        });
    }
}

#[expect(clippy::cast_possible_truncation, reason = "schema values always fit")]
fn gen_string_bounds(cases: &mut Vec<BoundaryCase>, field: &str, schema: &Value) {
    if let Some(enum_vals) = schema.get("enum").and_then(Value::as_array) {
        for ev in enum_vals {
            cases.push(BoundaryCase {
                field: field.into(),
                value: ev.clone(),
                description: format!("{field}=enum({ev})"),
                expected_valid: Some(true),
            });
        }
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!("__invalid_enum_value__"),
            description: format!("{field}=not-in-enum"),
            expected_valid: Some(false),
        });
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!(""),
            description: format!("{field}=empty-string (enum)"),
            expected_valid: Some(false),
        });
        return;
    }
    if let Some(fmt) = schema.get("format").and_then(Value::as_str) {
        if fmt == "email" {
            cases.push(BoundaryCase {
                field: field.into(),
                value: json!("user@example.com"),
                description: format!("{field}=valid-email"),
                expected_valid: Some(true),
            });
            cases.push(BoundaryCase {
                field: field.into(),
                value: json!("not-an-email"),
                description: format!("{field}=invalid-email"),
                expected_valid: Some(false),
            });
            cases.push(BoundaryCase {
                field: field.into(),
                value: json!(""),
                description: format!("{field}=empty-string (email)"),
                expected_valid: Some(false),
            });
        }
        return;
    }
    if let Some(min_len) = schema.get("minLength").and_then(Value::as_u64) {
        let n = min_len as usize;
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!("a".repeat(n)),
            description: format!("{field}=minLength({n})"),
            expected_valid: Some(true),
        });
        if n > 0 {
            cases.push(BoundaryCase {
                field: field.into(),
                value: json!("a".repeat(n - 1)),
                description: format!("{field}=minLength-1({val})", val = n - 1),
                expected_valid: Some(false),
            });
        }
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!("a".repeat(n + 1)),
            description: format!("{field}=minLength+1({val})", val = n + 1),
            expected_valid: Some(true),
        });
    }
    if let Some(max_len) = schema.get("maxLength").and_then(Value::as_u64) {
        let n = max_len as usize;
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!("a".repeat(n)),
            description: format!("{field}=maxLength({n})"),
            expected_valid: Some(true),
        });
        cases.push(BoundaryCase {
            field: field.into(),
            value: json!("a".repeat(n + 1)),
            description: format!("{field}=maxLength+1({val})", val = n + 1),
            expected_valid: Some(false),
        });
        if n > 0 {
            cases.push(BoundaryCase {
                field: field.into(),
                value: json!("a".repeat(n - 1)),
                description: format!("{field}=maxLength-1({val})", val = n - 1),
                expected_valid: Some(true),
            });
        }
    }
}

#[expect(clippy::cast_possible_truncation, reason = "schema values always fit")]
fn gen_array_bounds(cases: &mut Vec<BoundaryCase>, field: &str, schema: &Value) {
    if let Some(min_items) = schema.get("minItems").and_then(Value::as_u64) {
        let n = min_items as usize;
        let items: Vec<Value> = (0..n).map(|i| json!(format!("item{i}"))).collect();
        cases.push(BoundaryCase {
            field: field.into(),
            value: Value::Array(items),
            description: format!("{field}=minItems({n})"),
            expected_valid: Some(true),
        });
        if n > 0 {
            let items_minus: Vec<Value> = (0..n - 1).map(|i| json!(format!("item{i}"))).collect();
            cases.push(BoundaryCase {
                field: field.into(),
                value: Value::Array(items_minus),
                description: format!("{field}=minItems-1({val})", val = n - 1),
                expected_valid: Some(false),
            });
        }
    }
    if let Some(max_items) = schema.get("maxItems").and_then(Value::as_u64) {
        let n = max_items as usize;
        let items: Vec<Value> = (0..n).map(|i| json!(format!("item{i}"))).collect();
        cases.push(BoundaryCase {
            field: field.into(),
            value: Value::Array(items),
            description: format!("{field}=maxItems({n})"),
            expected_valid: Some(true),
        });
        let items_plus: Vec<Value> = (0..=n).map(|i| json!(format!("item{i}"))).collect();
        cases.push(BoundaryCase {
            field: field.into(),
            value: Value::Array(items_plus),
            description: format!("{field}=maxItems+1({val})", val = n + 1),
            expected_valid: Some(false),
        });
    }
}

pub fn generate_relation_boundaries(
    _schema: &Value,
    relations: &[crate::ave::FieldRelation],
) -> Vec<BoundaryCase> {
    let mut cases = vec![];
    for rel in relations {
        match rel.operator.as_str() {
            "lte" | "gte" => {
                let (lesser, greater) = if rel.operator == "lte" {
                    (&rel.field_a, &rel.field_b)
                } else {
                    (&rel.field_b, &rel.field_a)
                };
                cases.push(BoundaryCase {
                    field: format!("{lesser},{greater}"),
                    value: json!({lesser: 10, greater: 10}),
                    description: format!("{lesser}=={greater} (boundary)"),
                    expected_valid: Some(true),
                });
                cases.push(BoundaryCase {
                    field: format!("{lesser},{greater}"),
                    value: json!({lesser: 11, greater: 10}),
                    description: format!("{lesser}>{greater} (violation)"),
                    expected_valid: Some(false),
                });
                cases.push(BoundaryCase {
                    field: format!("{lesser},{greater}"),
                    value: json!({lesser: 9, greater: 10}),
                    description: format!("{lesser}<{greater} (safe)"),
                    expected_valid: Some(true),
                });
            }
            "eq" => {
                cases.push(BoundaryCase {
                    field: format!("{},{}", rel.field_a, rel.field_b),
                    value: json!({&rel.field_a: 10, &rel.field_b: 10}),
                    description: format!("{}=={} (correct)", rel.field_a, rel.field_b),
                    expected_valid: Some(true),
                });
                cases.push(BoundaryCase {
                    field: format!("{},{}", rel.field_a, rel.field_b),
                    value: json!({&rel.field_a: 11, &rel.field_b: 10}),
                    description: format!("{}!={} (violation +1)", rel.field_a, rel.field_b),
                    expected_valid: Some(false),
                });
                cases.push(BoundaryCase {
                    field: format!("{},{}", rel.field_a, rel.field_b),
                    value: json!({&rel.field_a: 9, &rel.field_b: 10}),
                    description: format!("{}!={} (violation -1)", rel.field_a, rel.field_b),
                    expected_valid: Some(false),
                });
            }
            _ => {}
        }
    }
    cases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_min_max_boundaries() {
        let schema = json!({
            "type": "object",
            "properties": {
                "score": {"type": "integer", "minimum": 0, "maximum": 100}
            }
        });
        let cases = generate_boundary_cases(&schema);
        let values: Vec<i64> = cases
            .iter()
            .filter(|c| c.field == "score")
            .filter_map(|c| c.value.as_i64())
            .collect();
        assert!(values.contains(&0), "should have min boundary 0");
        assert!(values.contains(&-1), "should have below-min -1");
        assert!(values.contains(&1), "should have above-min 1");
        assert!(values.contains(&100), "should have max boundary 100");
        assert!(values.contains(&101), "should have above-max 101");
        assert!(values.contains(&99), "should have below-max 99");
    }

    #[test]
    fn enum_boundaries() {
        let schema = json!({
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": ["active", "inactive"]}
            }
        });
        let cases = generate_boundary_cases(&schema);
        let status_cases: Vec<_> = cases.iter().filter(|c| c.field == "status").collect();
        assert!(status_cases.iter().any(|c| c.value == json!("active")));
        assert!(status_cases.iter().any(|c| c.value == json!("inactive")));
        assert!(status_cases
            .iter()
            .any(|c| c.value == json!("__invalid_enum_value__")));
    }

    #[test]
    fn required_field_boundaries() {
        let schema = json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {"type": "string", "minLength": 1}
            }
        });
        let cases = generate_boundary_cases(&schema);
        let name_cases: Vec<_> = cases.iter().filter(|c| c.field == "name").collect();
        assert!(name_cases
            .iter()
            .any(|c| c.value.is_null() && c.expected_valid == Some(false)));
        assert!(name_cases
            .iter()
            .any(|c| c.description.contains("present") && c.expected_valid == Some(true)));
    }

    #[test]
    fn relation_lte_boundaries() {
        let schema = json!({"type": "object", "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}}});
        let rels = vec![crate::ave::FieldRelation {
            field_a: "a".into(),
            operator: "lte".into(),
            field_b: "b".into(),
        }];
        let cases = generate_relation_boundaries(&schema, &rels);
        assert_eq!(cases.len(), 3);
        assert!(cases
            .iter()
            .any(|c| c.description.contains("boundary") && c.expected_valid == Some(true)));
        assert!(cases
            .iter()
            .any(|c| c.description.contains("violation") && c.expected_valid == Some(false)));
        assert!(cases
            .iter()
            .any(|c| c.description.contains("safe") && c.expected_valid == Some(true)));
    }

    #[test]
    fn medium_schema_generates_many_cases() {
        let schema = json!({
            "type": "object",
            "required": ["id", "status", "started_at"],
            "properties": {
                "id": {"type": "string", "minLength": 1},
                "name": {"type": "string"},
                "status": {"type": "string", "enum": ["queued", "running", "completed", "failed", "skipped", "cancelled"]},
                "conclusion": {"type": "string", "enum": ["success", "failure", "skipped", "cancelled"]},
                "started_at": {"type": "string", "format": "date-time"},
                "completed_at": {"type": "string", "format": "date-time"},
                "changed_files": {"type": "array", "items": {"type": "string"}},
                "output": {"type": "string"},
                "error": {"type": "string"},
                "title": {"type": "string"},
                "changes_count": {"type": "integer", "minimum": 0},
                "test_passed": {"type": "boolean"},
                "duration_ms": {"type": "integer", "minimum": 0}
            }
        });
        let cases = generate_boundary_cases(&schema);
        assert!(cases.len() >= 20, "expected 20+ cases, got {}", cases.len());
    }
}
