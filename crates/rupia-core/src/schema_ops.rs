use std::collections::HashMap;

use serde_json::Value;

pub fn inject_constraints_to_description(schema: &Value) -> Value {
    let mut result = schema.clone();
    inject_recursive(&mut result);
    result
}

fn inject_recursive(schema: &mut Value) {
    if let Some(obj) = schema.as_object_mut() {
        let mut parts: Vec<String> = Vec::new();
        let existing_desc = obj
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if let Some(min) = obj.get("minimum").and_then(Value::as_f64) {
            parts.push(format!("minimum: {min}"));
        }
        if let Some(max) = obj.get("maximum").and_then(Value::as_f64) {
            parts.push(format!("maximum: {max}"));
        }
        if let Some(min) = obj.get("minLength").and_then(Value::as_u64) {
            parts.push(format!("minLength: {min}"));
        }
        if let Some(max) = obj.get("maxLength").and_then(Value::as_u64) {
            parts.push(format!("maxLength: {max}"));
        }
        if let Some(pat) = obj.get("pattern").and_then(Value::as_str) {
            parts.push(format!("pattern: {pat}"));
        }
        if let Some(fmt) = obj.get("format").and_then(Value::as_str) {
            parts.push(format!("format: {fmt}"));
        }
        if let Some(enums) = obj.get("enum").and_then(Value::as_array) {
            let vals: Vec<String> = enums.iter().map(ToString::to_string).collect();
            parts.push(format!("allowed: [{}]", vals.join(", ")));
        }
        if let Some(def) = obj.get("default") {
            parts.push(format!("default: {def}"));
        }
        if !parts.is_empty() {
            let constraint_str = parts.join(", ");
            let new_desc = if existing_desc.is_empty() {
                format!("@constraints {constraint_str}")
            } else if existing_desc.contains("@constraints") {
                existing_desc
            } else {
                format!("{existing_desc} @constraints {constraint_str}")
            };
            obj.insert("description".into(), Value::String(new_desc));
        }
        if let Some(props) = obj.get_mut("properties") {
            if let Some(props_obj) = props.as_object_mut() {
                for val in props_obj.values_mut() {
                    inject_recursive(val);
                }
            }
        }
        if let Some(items) = obj.get_mut("items") {
            inject_recursive(items);
        }
        let defs_key = if obj.contains_key("$defs") {
            Some("$defs")
        } else if obj.contains_key("definitions") {
            Some("definitions")
        } else {
            None
        };
        if let Some(key) = defs_key {
            if let Some(defs) = obj.get_mut(key) {
                if let Some(defs_obj) = defs.as_object_mut() {
                    for val in defs_obj.values_mut() {
                        inject_recursive(val);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchemaDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<SchemaChange>,
}

#[derive(Debug, Clone)]
pub struct SchemaChange {
    pub path: String,
    pub field: String,
    pub old: String,
    pub new: String,
}

impl SchemaDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    pub fn is_compatible(&self) -> bool {
        self.removed.is_empty()
            && self
                .changed
                .iter()
                .all(|c| c.field == "description" || c.field == "default" || c.field == "example")
    }
}

pub fn diff_schemas(old: &Value, new: &Value) -> SchemaDiff {
    let old_props = old
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let new_props = new
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let added: Vec<String> = new_props
        .keys()
        .filter(|k| !old_props.contains_key(*k))
        .cloned()
        .collect();
    let removed: Vec<String> = old_props
        .keys()
        .filter(|k| !new_props.contains_key(*k))
        .cloned()
        .collect();
    let mut changed = Vec::new();
    for (key, old_val) in &old_props {
        if let Some(new_val) = new_props.get(key) {
            for field in &[
                "type",
                "format",
                "minimum",
                "maximum",
                "enum",
                "description",
                "default",
            ] {
                let ov = old_val
                    .get(*field)
                    .map(ToString::to_string)
                    .unwrap_or_default();
                let nv = new_val
                    .get(*field)
                    .map(ToString::to_string)
                    .unwrap_or_default();
                if ov != nv {
                    changed.push(SchemaChange {
                        path: format!("$.{key}"),
                        field: (*field).to_owned(),
                        old: ov,
                        new: nv,
                    });
                }
            }
        }
    }
    SchemaDiff {
        added,
        removed,
        changed,
    }
}

pub fn make_partial(schema: &Value) -> Value {
    let mut result = schema.clone();
    if let Some(obj) = result.as_object_mut() {
        obj.remove("required");
    }
    result
}

#[derive(Debug, Default)]
pub struct ValidationStats {
    pub total_validations: u64,
    pub successes: u64,
    pub failures: u64,
    pub field_errors: HashMap<String, u64>,
    pub error_code_counts: HashMap<String, u64>,
}

impl ValidationStats {
    pub fn record_success(&mut self) {
        self.total_validations += 1;
        self.successes += 1;
    }

    pub fn record_failure(&mut self, errors: &[crate::types::ValidationError]) {
        self.total_validations += 1;
        self.failures += 1;
        for e in errors {
            *self.field_errors.entry(e.path.clone()).or_default() += 1;
            let code = if e.expected.contains("Format<") {
                "format"
            } else if e.expected.contains("Minimum<") || e.expected.contains("Maximum<") {
                "range"
            } else if e.expected.contains("one of") {
                "enum"
            } else if e
                .description
                .as_ref()
                .is_some_and(|d| d.contains("undefined"))
            {
                "required"
            } else {
                "type"
            };
            *self.error_code_counts.entry(code.to_owned()).or_default() += 1;
        }
    }

    pub fn top_failing_fields(&self, n: usize) -> Vec<(&str, u64)> {
        let mut fields: Vec<(&str, u64)> = self
            .field_errors
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();
        fields.sort_by(|a, b| b.1.cmp(&a.1));
        fields.truncate(n);
        fields
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "stats counters won't exceed f64 mantissa"
    )]
    pub fn success_rate(&self) -> f64 {
        if self.total_validations == 0 {
            return 0.0;
        }
        self.successes as f64 / self.total_validations as f64
    }

    pub fn prompt_hints(&self) -> Vec<String> {
        let mut hints = Vec::new();
        for (field, count) in self.top_failing_fields(5) {
            hints.push(format!(
                "Field '{field}' fails {count} times — add explicit constraint to prompt"
            ));
        }
        for (code, count) in &self.error_code_counts {
            if *count > 3 {
                let hint = match code.as_str() {
                    "format" => {
                        "Add format examples to prompt (e.g., 'email must be user@domain.com')"
                    }
                    "range" => "State numeric ranges explicitly in prompt",
                    "enum" => "List all allowed values in prompt",
                    "required" => "Emphasize required fields with 'MUST include'",
                    _ => "Check type constraints in schema",
                };
                hints.push(format!("{code}: {count} errors — {hint}"));
            }
        }
        hints
    }
}

pub fn infer_schema(samples: &[Value]) -> Value {
    if samples.is_empty() {
        return serde_json::json!({"type": "object"});
    }
    infer_from_values(samples)
}

fn infer_from_values(values: &[Value]) -> Value {
    if values.is_empty() {
        return serde_json::json!({});
    }
    let first = &values[0];
    match first {
        Value::Object(_) => infer_object(values),
        Value::Array(_) => infer_array(values),
        Value::String(_) => serde_json::json!({"type": "string"}),
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                serde_json::json!({"type": "integer"})
            } else {
                serde_json::json!({"type": "number"})
            }
        }
        Value::Bool(_) => serde_json::json!({"type": "boolean"}),
        Value::Null => serde_json::json!({"type": "null"}),
    }
}

fn infer_object(values: &[Value]) -> Value {
    let mut all_keys: HashMap<String, Vec<Value>> = HashMap::new();
    let mut required_candidates: HashMap<String, usize> = HashMap::new();
    let total = values.len();
    for val in values {
        if let Some(obj) = val.as_object() {
            for (key, v) in obj {
                all_keys.entry(key.clone()).or_default().push(v.clone());
                *required_candidates.entry(key.clone()).or_default() += 1;
            }
        }
    }
    let mut properties = serde_json::Map::new();
    for (key, vals) in &all_keys {
        properties.insert(key.clone(), infer_from_values(vals));
    }
    let required: Vec<Value> = required_candidates
        .iter()
        .filter(|(_, count)| **count == total)
        .map(|(key, _)| Value::String(key.clone()))
        .collect();
    let mut schema = serde_json::json!({
        "type": "object",
        "properties": properties,
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

fn infer_array(values: &[Value]) -> Value {
    let mut all_items: Vec<&Value> = Vec::new();
    for val in values {
        if let Some(arr) = val.as_array() {
            for item in arr {
                all_items.push(item);
            }
        }
    }
    let items_schema = if all_items.is_empty() {
        serde_json::json!({})
    } else {
        infer_from_values(&all_items.into_iter().cloned().collect::<Vec<_>>())
    };
    serde_json::json!({"type": "array", "items": items_schema})
}

pub fn compress_feedback(feedback: &str) -> String {
    let lines: Vec<&str> = feedback.lines().collect();
    if lines.len() <= 20 {
        return feedback.to_owned();
    }
    let mut seen_errors: HashMap<String, usize> = HashMap::new();
    let mut result_lines: Vec<String> = Vec::new();
    for line in &lines {
        if let Some(err_start) = line.find("// ❌") {
            let error_part = &line[err_start..];
            let key = extract_error_key(error_part);
            let count = seen_errors.entry(key.clone()).or_default();
            *count += 1;
            if *count <= 2 {
                result_lines.push((*line).to_owned());
            } else if *count == 3 {
                result_lines.push(format!(
                    "  // ... ({} more '{key}' errors omitted)",
                    *count - 2
                ));
            } else if let Some(summary) = result_lines
                .iter_mut()
                .rev()
                .find(|l| l.contains(&format!("more '{key}'")))
            {
                *summary = format!("  // ... ({} more '{key}' errors omitted)", *count - 2);
            }
        } else {
            result_lines.push((*line).to_owned());
        }
    }
    result_lines.join("\n")
}

fn extract_error_key(error_comment: &str) -> String {
    if let Some(start) = error_comment.find("\"expected\":\"") {
        let rest = &error_comment[start + 12..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_owned();
        }
    }
    "unknown".to_owned()
}

pub fn openapi_to_llm_tools(openapi: &Value) -> Vec<Value> {
    let mut tools = Vec::new();
    let Some(paths) = openapi.get("paths").and_then(Value::as_object) else {
        return tools;
    };
    for (path, methods) in paths {
        let Some(methods_obj) = methods.as_object() else {
            continue;
        };
        for (method, operation) in methods_obj {
            if !["get", "post", "put", "patch", "delete"].contains(&method.as_str()) {
                continue;
            }
            let op_id = operation
                .get("operationId")
                .and_then(Value::as_str)
                .unwrap_or(path.as_str());
            let description = operation
                .get("summary")
                .or_else(|| operation.get("description"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let parameters = build_parameters_schema(operation, openapi);
            tools.push(serde_json::json!({
                "name": sanitize_name(op_id),
                "description": format!("{method} {path} - {description}"),
                "parameters": parameters,
                "method": method,
                "path": path,
            }));
        }
    }
    tools
}

fn build_parameters_schema(operation: &Value, openapi: &Value) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    if let Some(params) = operation.get("parameters").and_then(Value::as_array) {
        for param in params {
            let name = param.get("name").and_then(Value::as_str).unwrap_or("_");
            let schema = param
                .get("schema")
                .cloned()
                .unwrap_or(serde_json::json!({"type": "string"}));
            let resolved = resolve_schema_ref(&schema, openapi);
            properties.insert(name.to_owned(), resolved);
            if param.get("required").and_then(Value::as_bool) == Some(true) {
                required.push(Value::String(name.to_owned()));
            }
        }
    }
    if let Some(body) = operation.get("requestBody") {
        if let Some(content) = body.get("content") {
            let json_schema = content
                .get("application/json")
                .and_then(|c| c.get("schema"));
            if let Some(schema) = json_schema {
                let resolved = resolve_schema_ref(schema, openapi);
                if let Some(props) = resolved.get("properties").and_then(Value::as_object) {
                    for (k, v) in props {
                        properties.insert(k.clone(), v.clone());
                    }
                }
                if let Some(req) = resolved.get("required").and_then(Value::as_array) {
                    required.extend(req.iter().cloned());
                }
            }
        }
    }
    let mut result = serde_json::json!({"type": "object", "properties": properties});
    if !required.is_empty() {
        result["required"] = Value::Array(required);
    }
    result
}

fn resolve_schema_ref(schema: &Value, openapi: &Value) -> Value {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let path: Vec<&str> = r.trim_start_matches("#/").split('/').collect();
        let mut current = openapi;
        for segment in &path {
            current = match current.get(*segment) {
                Some(v) => v,
                None => return schema.clone(),
            };
        }
        return current.clone();
    }
    schema.clone()
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .chars()
        .take(64)
        .collect()
}

#[derive(Debug, Clone)]
pub struct CrossRefResult {
    pub universal_enums: Vec<UniversalEnum>,
    pub universal_constraints: Vec<UniversalConstraint>,
    pub divergences: Vec<Divergence>,
}

#[derive(Debug, Clone)]
pub struct UniversalEnum {
    pub field_pattern: String,
    pub common_values: Vec<String>,
    pub all_values: Vec<String>,
    pub source_count: usize,
}

#[derive(Debug, Clone)]
pub struct UniversalConstraint {
    pub field_pattern: String,
    pub constraint_type: String,
    pub value: String,
    pub agreement: usize,
    pub total: usize,
}

#[derive(Debug, Clone)]
pub struct Divergence {
    pub field_pattern: String,
    pub description: String,
}

pub fn cross_reference_schemas(schemas: &[Value]) -> CrossRefResult {
    if schemas.is_empty() {
        return CrossRefResult {
            universal_enums: vec![],
            universal_constraints: vec![],
            divergences: vec![],
        };
    }
    let all_flat: Vec<HashMap<String, Value>> =
        schemas.iter().map(|s| flatten_properties(s, "")).collect();
    let mut field_sources: HashMap<String, Vec<&Value>> = HashMap::new();
    for flat in &all_flat {
        for (key, val) in flat {
            field_sources.entry(key.clone()).or_default().push(val);
        }
    }
    let mut universal_enums = Vec::new();
    let mut universal_constraints = Vec::new();
    let mut divergences = Vec::new();
    for (field, sources) in &field_sources {
        let total = sources.len();
        collect_enum_info(
            field,
            sources,
            total,
            &mut universal_enums,
            &mut divergences,
        );
        collect_numeric_constraints(
            field,
            sources,
            total,
            &mut universal_constraints,
            &mut divergences,
        );
        collect_format_constraints(
            field,
            sources,
            total,
            &mut universal_constraints,
            &mut divergences,
        );
        collect_required_constraints(field, sources, total, schemas, &mut universal_constraints);
        collect_type_constraints(
            field,
            sources,
            total,
            &mut universal_constraints,
            &mut divergences,
        );
    }
    CrossRefResult {
        universal_enums,
        universal_constraints,
        divergences,
    }
}

fn flatten_properties(schema: &Value, prefix: &str) -> HashMap<String, Value> {
    let mut result = HashMap::new();
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return result;
    };
    for (key, val) in props {
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        result.insert(full_key.clone(), val.clone());
        if val.get("properties").is_some() {
            result.extend(flatten_properties(val, &full_key));
        }
    }
    result
}

fn collect_enum_info(
    field: &str,
    sources: &[&Value],
    total: usize,
    enums: &mut Vec<UniversalEnum>,
    divergences: &mut Vec<Divergence>,
) {
    let enum_sets: Vec<Vec<String>> = sources
        .iter()
        .filter_map(|v| {
            v.get("enum").and_then(Value::as_array).map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
        })
        .collect();
    if enum_sets.is_empty() {
        return;
    }
    let mut all_values: Vec<String> = enum_sets.iter().flatten().cloned().collect();
    all_values.sort();
    all_values.dedup();
    let common_values: Vec<String> = all_values
        .iter()
        .filter(|v| enum_sets.iter().all(|s| s.contains(v)))
        .cloned()
        .collect();
    if common_values.len() < all_values.len() {
        let only_some: Vec<&String> = all_values
            .iter()
            .filter(|v| !common_values.contains(v))
            .collect();
        divergences.push(Divergence {
            field_pattern: field.to_owned(),
            description: format!(
                "enum values not universal: [{}]",
                only_some
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    enums.push(UniversalEnum {
        field_pattern: field.to_owned(),
        common_values,
        all_values,
        source_count: total,
    });
}

fn collect_numeric_constraints(
    field: &str,
    sources: &[&Value],
    total: usize,
    constraints: &mut Vec<UniversalConstraint>,
    divergences: &mut Vec<Divergence>,
) {
    let mins: Vec<f64> = sources
        .iter()
        .filter_map(|v| v.get("minimum").and_then(Value::as_f64))
        .collect();
    let maxs: Vec<f64> = sources
        .iter()
        .filter_map(|v| v.get("maximum").and_then(Value::as_f64))
        .collect();
    if !mins.is_empty() {
        let all_same = mins.windows(2).all(|w| (w[0] - w[1]).abs() < f64::EPSILON);
        if all_same {
            constraints.push(UniversalConstraint {
                field_pattern: field.to_owned(),
                constraint_type: "minimum".to_owned(),
                value: format!("{}", mins[0]),
                agreement: mins.len(),
                total,
            });
        } else {
            divergences.push(Divergence {
                field_pattern: field.to_owned(),
                description: format!("minimum differs: {mins:?}"),
            });
        }
    }
    if !maxs.is_empty() {
        let all_same = maxs.windows(2).all(|w| (w[0] - w[1]).abs() < f64::EPSILON);
        if all_same {
            constraints.push(UniversalConstraint {
                field_pattern: field.to_owned(),
                constraint_type: "maximum".to_owned(),
                value: format!("{}", maxs[0]),
                agreement: maxs.len(),
                total,
            });
        } else {
            divergences.push(Divergence {
                field_pattern: field.to_owned(),
                description: format!("maximum differs: {maxs:?}"),
            });
        }
    }
}

fn collect_format_constraints(
    field: &str,
    sources: &[&Value],
    total: usize,
    constraints: &mut Vec<UniversalConstraint>,
    divergences: &mut Vec<Divergence>,
) {
    let formats: Vec<&str> = sources
        .iter()
        .filter_map(|v| v.get("format").and_then(Value::as_str))
        .collect();
    if formats.is_empty() {
        return;
    }
    let all_same = formats.windows(2).all(|w| w[0] == w[1]);
    if all_same {
        constraints.push(UniversalConstraint {
            field_pattern: field.to_owned(),
            constraint_type: "format".to_owned(),
            value: formats[0].to_owned(),
            agreement: formats.len(),
            total,
        });
    } else {
        divergences.push(Divergence {
            field_pattern: field.to_owned(),
            description: format!("format differs: {formats:?}"),
        });
    }
}

fn collect_required_constraints(
    field: &str,
    _sources: &[&Value],
    total: usize,
    schemas: &[Value],
    constraints: &mut Vec<UniversalConstraint>,
) {
    let leaf = field.rsplit('.').next().unwrap_or(field);
    let parent_prefix = if field.contains('.') {
        &field[..field.len() - leaf.len() - 1]
    } else {
        ""
    };
    let required_count = schemas
        .iter()
        .filter(|s| {
            let target = if parent_prefix.is_empty() {
                (*s).clone()
            } else {
                navigate_to_nested(s, parent_prefix)
            };
            target
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(leaf)))
        })
        .count();
    if required_count > 0 {
        constraints.push(UniversalConstraint {
            field_pattern: field.to_owned(),
            constraint_type: "required".to_owned(),
            value: "true".to_owned(),
            agreement: required_count,
            total,
        });
    }
}

fn navigate_to_nested(schema: &Value, dotpath: &str) -> Value {
    let mut current = schema.clone();
    for seg in dotpath.split('.') {
        current = current
            .get("properties")
            .and_then(|p| p.get(seg))
            .cloned()
            .unwrap_or(Value::Null);
    }
    current
}

fn collect_type_constraints(
    field: &str,
    sources: &[&Value],
    total: usize,
    constraints: &mut Vec<UniversalConstraint>,
    divergences: &mut Vec<Divergence>,
) {
    let types: Vec<&str> = sources
        .iter()
        .filter_map(|v| v.get("type").and_then(Value::as_str))
        .collect();
    if types.is_empty() {
        return;
    }
    let all_same = types.windows(2).all(|w| w[0] == w[1]);
    if all_same {
        constraints.push(UniversalConstraint {
            field_pattern: field.to_owned(),
            constraint_type: "type".to_owned(),
            value: types[0].to_owned(),
            agreement: types.len(),
            total,
        });
    } else {
        divergences.push(Divergence {
            field_pattern: field.to_owned(),
            description: format!("type differs: {types:?}"),
        });
    }
}

#[derive(Debug, Clone)]
pub struct ConsistencyResult {
    pub consistent: bool,
    pub conflicts: Vec<RuleConflict>,
    pub satisfying_example: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct RuleConflict {
    pub rule_a: String,
    pub rule_b: String,
    pub reason: String,
}

use crate::ave::FieldRelation;

pub fn check_rule_consistency(rules: &[FieldRelation], schema: &Value) -> ConsistencyResult {
    let mut bounds = extract_bounds_from_schema(schema);
    let mut conflicts = Vec::new();
    propagate_ordering_rules(rules, &mut bounds, &mut conflicts);
    detect_ordering_cycles(rules, &mut conflicts);
    check_arithmetic_rules(rules, &bounds, &mut conflicts);
    let consistent = conflicts.is_empty();
    let satisfying_example = if consistent {
        Some(generate_satisfying_example(&bounds, schema))
    } else {
        None
    };
    ConsistencyResult {
        consistent,
        conflicts,
        satisfying_example,
    }
}

#[derive(Debug, Clone)]
struct FieldBounds {
    min: Option<f64>,
    max: Option<f64>,
}

fn extract_bounds_from_schema(schema: &Value) -> HashMap<String, FieldBounds> {
    let mut bounds = HashMap::new();
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return bounds;
    };
    for (key, val) in props {
        let min = val.get("minimum").and_then(Value::as_f64);
        let max = val.get("maximum").and_then(Value::as_f64);
        if min.is_some() || max.is_some() {
            bounds.insert(key.clone(), FieldBounds { min, max });
        } else {
            bounds.insert(
                key.clone(),
                FieldBounds {
                    min: None,
                    max: None,
                },
            );
        }
    }
    bounds
}

fn propagate_ordering_rules(
    rules: &[FieldRelation],
    bounds: &mut HashMap<String, FieldBounds>,
    conflicts: &mut Vec<RuleConflict>,
) {
    let max_iterations = rules.len() * 3 + 1;
    for _ in 0..max_iterations {
        let mut changed = false;
        for rule in rules {
            let op = rule.operator.as_str();
            match op {
                "lte" | "lt" => {
                    let b_max = bounds.get(&rule.field_b).and_then(|b| b.max);
                    if let Some(bmax) = b_max {
                        let entry = bounds.entry(rule.field_a.clone()).or_insert(FieldBounds {
                            min: None,
                            max: None,
                        });
                        let effective = if op == "lt" { bmax - 1.0 } else { bmax };
                        if entry.max.is_none() || entry.max.is_some_and(|m| m > effective) {
                            entry.max = Some(effective);
                            changed = true;
                        }
                    }
                    let a_min = bounds.get(&rule.field_a).and_then(|b| b.min);
                    if let Some(amin) = a_min {
                        let entry = bounds.entry(rule.field_b.clone()).or_insert(FieldBounds {
                            min: None,
                            max: None,
                        });
                        let effective = if op == "lt" { amin + 1.0 } else { amin };
                        if entry.min.is_none() || entry.min.is_some_and(|m| m < effective) {
                            entry.min = Some(effective);
                            changed = true;
                        }
                    }
                }
                "gte" | "gt" => {
                    let b_min = bounds.get(&rule.field_b).and_then(|b| b.min);
                    if let Some(bmin) = b_min {
                        let entry = bounds.entry(rule.field_a.clone()).or_insert(FieldBounds {
                            min: None,
                            max: None,
                        });
                        let effective = if op == "gt" { bmin + 1.0 } else { bmin };
                        if entry.min.is_none() || entry.min.is_some_and(|m| m < effective) {
                            entry.min = Some(effective);
                            changed = true;
                        }
                    }
                    let a_max = bounds.get(&rule.field_a).and_then(|b| b.max);
                    if let Some(amax) = a_max {
                        let entry = bounds.entry(rule.field_b.clone()).or_insert(FieldBounds {
                            min: None,
                            max: None,
                        });
                        let effective = if op == "gt" { amax - 1.0 } else { amax };
                        if entry.max.is_none() || entry.max.is_some_and(|m| m > effective) {
                            entry.max = Some(effective);
                            changed = true;
                        }
                    }
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }
    for (field, fb) in bounds.iter() {
        if let (Some(lo), Some(hi)) = (fb.min, fb.max) {
            if lo > hi {
                conflicts.push(RuleConflict {
                    rule_a: format!("{field}.min = {lo}"),
                    rule_b: format!("{field}.max = {hi}"),
                    reason: format!("range conflict: {field} requires min({lo}) <= max({hi})"),
                });
            }
        }
    }
}

fn detect_ordering_cycles(rules: &[FieldRelation], conflicts: &mut Vec<RuleConflict>) {
    let ordering_ops = ["lt", "lte", "gt", "gte"];
    let mut edges: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for rule in rules {
        if !ordering_ops.contains(&rule.operator.as_str()) {
            continue;
        }
        let (from, to, op) = match rule.operator.as_str() {
            "lt" | "lte" => (
                rule.field_a.clone(),
                rule.field_b.clone(),
                rule.operator.clone(),
            ),
            "gt" | "gte" => (rule.field_b.clone(), rule.field_a.clone(), {
                match rule.operator.as_str() {
                    "gt" => "lt".to_owned(),
                    _ => "lte".to_owned(),
                }
            }),
            _ => continue,
        };
        edges.entry(from).or_default().push((to, op));
    }
    let nodes: Vec<String> = edges.keys().cloned().collect();
    for start in &nodes {
        let mut visited = HashMap::new();
        let mut stack = vec![(start.clone(), false)];
        while let Some((node, is_strict)) = stack.pop() {
            if &node == start && visited.contains_key(start) {
                if is_strict {
                    conflicts.push(RuleConflict {
                        rule_a: format!("{start} < ... < {start}"),
                        rule_b: String::new(),
                        reason: format!("cycle conflict: strict ordering cycle through {start}"),
                    });
                }
                continue;
            }
            if visited.contains_key(&node) {
                continue;
            }
            visited.insert(node.clone(), is_strict);
            if let Some(neighbors) = edges.get(&node) {
                for (next, op) in neighbors {
                    let strict = is_strict || op == "lt";
                    stack.push((next.clone(), strict));
                }
            }
        }
    }
}

fn check_arithmetic_rules(
    rules: &[FieldRelation],
    bounds: &HashMap<String, FieldBounds>,
    conflicts: &mut Vec<RuleConflict>,
) {
    for rule in rules {
        if rule.operator != "eq" {
            continue;
        }
        if !rule.field_b.contains('+') && !rule.field_b.contains('-') {
            continue;
        }
        let parts: Vec<(f64, &str)> = parse_arithmetic_expr(&rule.field_b);
        if parts.is_empty() {
            continue;
        }
        let target_bounds = bounds.get(&rule.field_a);
        let mut sum_min: f64 = 0.0;
        let mut sum_max: f64 = 0.0;
        let mut all_bounded = true;
        for (sign, field) in &parts {
            if let Some(fb) = bounds.get(*field) {
                if *sign > 0.0 {
                    sum_min += fb.min.unwrap_or(f64::NEG_INFINITY);
                    sum_max += fb.max.unwrap_or(f64::INFINITY);
                } else {
                    sum_min += fb.max.map_or(f64::NEG_INFINITY, |m| -m);
                    sum_max += fb.min.map_or(f64::INFINITY, |m| -m);
                }
            } else {
                all_bounded = false;
            }
        }
        if !all_bounded {
            continue;
        }
        if let Some(tb) = target_bounds {
            if let Some(tmax) = tb.max {
                if sum_min > tmax {
                    conflicts.push(RuleConflict {
                        rule_a: format!("{} = {}", rule.field_a, rule.field_b),
                        rule_b: format!("{}.max = {tmax}", rule.field_a),
                        reason: format!(
                            "arithmetic infeasibility: {} minimum possible value ({sum_min}) > {}.max ({tmax})",
                            rule.field_b, rule.field_a
                        ),
                    });
                }
            }
            if let Some(tmin) = tb.min {
                if sum_max < tmin {
                    conflicts.push(RuleConflict {
                        rule_a: format!("{} = {}", rule.field_a, rule.field_b),
                        rule_b: format!("{}.min = {tmin}", rule.field_a),
                        reason: format!(
                            "arithmetic infeasibility: {} maximum possible value ({sum_max}) < {}.min ({tmin})",
                            rule.field_b, rule.field_a
                        ),
                    });
                }
            }
        }
    }
}

fn parse_arithmetic_expr(expr: &str) -> Vec<(f64, &str)> {
    let mut result = Vec::new();
    let mut sign = 1.0_f64;
    for token in expr.split_whitespace() {
        match token {
            "+" => sign = 1.0,
            "-" => sign = -1.0,
            field => {
                result.push((sign, field));
                sign = 1.0;
            }
        }
    }
    result
}

fn generate_satisfying_example(bounds: &HashMap<String, FieldBounds>, schema: &Value) -> Value {
    let mut obj = serde_json::Map::new();
    let props = schema.get("properties").and_then(Value::as_object);
    let fields: Vec<&String> = if let Some(p) = props {
        p.keys().collect()
    } else {
        bounds.keys().collect()
    };
    for field in fields {
        let val = if let Some(fb) = bounds.get(field) {
            match (fb.min, fb.max) {
                (Some(lo), Some(hi)) => f64::midpoint(lo, hi),
                (Some(lo), None) => lo + 1.0,
                (None, Some(hi)) => hi - 1.0,
                (None, None) => 0.0,
            }
        } else {
            let field_type = props
                .and_then(|p| p.get(field))
                .and_then(|v| v.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("number");
            match field_type {
                "string" => {
                    obj.insert(field.clone(), Value::String("example".to_owned()));
                    continue;
                }
                "boolean" => {
                    obj.insert(field.clone(), Value::Bool(true));
                    continue;
                }
                _ => 0.0,
            }
        };
        let prop_type = props
            .and_then(|p| p.get(field))
            .and_then(|v| v.get("type"))
            .and_then(Value::as_str);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "satisfying example: mid-range values won't overflow i64"
        )]
        if prop_type == Some("integer") {
            obj.insert(field.clone(), serde_json::json!(val as i64));
        } else {
            obj.insert(field.clone(), serde_json::json!(val));
        }
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inject_constraints() {
        let schema = json!({
            "type": "object",
            "properties": {
                "age": {"type": "integer", "minimum": 0, "maximum": 150},
                "email": {"type": "string", "format": "email"},
                "role": {"type": "string", "enum": ["admin", "user"]}
            }
        });
        let result = inject_constraints_to_description(&schema);
        let age_desc = result["properties"]["age"]["description"].as_str().unwrap();
        assert!(age_desc.contains("minimum: 0"));
        assert!(age_desc.contains("maximum: 150"));
        let email_desc = result["properties"]["email"]["description"]
            .as_str()
            .unwrap();
        assert!(email_desc.contains("format: email"));
    }

    #[test]
    fn schema_diff_detects_changes() {
        let old = json!({"properties": {"name": {"type": "string"}, "age": {"type": "integer"}}});
        let new = json!({"properties": {"name": {"type": "string"}, "email": {"type": "string"}}});
        let diff = diff_schemas(&old, &new);
        assert!(diff.added.contains(&"email".to_owned()));
        assert!(diff.removed.contains(&"age".to_owned()));
        assert!(!diff.is_compatible());
    }

    #[test]
    fn schema_diff_compatible() {
        let old = json!({"properties": {"name": {"type": "string"}}});
        let new = json!({"properties": {"name": {"type": "string"}, "bio": {"type": "string"}}});
        let diff = diff_schemas(&old, &new);
        assert!(diff.is_compatible());
    }

    #[test]
    fn make_partial_removes_required() {
        let schema = json!({"type": "object", "required": ["name", "email"], "properties": {"name": {"type": "string"}}});
        let partial = make_partial(&schema);
        assert!(partial.get("required").is_none());
    }

    #[test]
    fn infer_from_samples() {
        let samples = vec![
            json!({"name": "Alice", "age": 30}),
            json!({"name": "Bob", "age": 25}),
        ];
        let schema = infer_schema(&samples);
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["name"]["type"] == "string");
        assert!(schema["properties"]["age"]["type"] == "integer");
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("name")));
    }

    #[test]
    fn infer_nested() {
        let samples = vec![json!({"items": [1, 2, 3]})];
        let schema = infer_schema(&samples);
        assert_eq!(schema["properties"]["items"]["type"], "array");
        assert_eq!(schema["properties"]["items"]["items"]["type"], "integer");
    }

    #[test]
    fn validation_stats_tracking() {
        let mut stats = ValidationStats::default();
        stats.record_success();
        stats.record_failure(&[crate::types::ValidationError {
            path: "$input.email".into(),
            expected: "string & Format<\"email\">".into(),
            value: json!("bad"),
            description: None,
        }]);
        assert_eq!(stats.total_validations, 2);
        assert_eq!(stats.successes, 1);
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.field_errors["$input.email"], 1);
        assert!(!stats.prompt_hints().is_empty());
    }

    #[test]
    fn compress_deduplicates() {
        let feedback = (0..10)
            .map(|i| format!(r#"  "field_{i}": bad // ❌ [{{"expected":"string"}}]"#))
            .collect::<Vec<_>>()
            .join("\n");
        let long = format!("```json\n{{\n{feedback}\n}}\n```");
        let compressed = compress_feedback(&long);
        assert!(compressed.len() <= long.len());
    }

    #[test]
    fn openapi_to_tools() {
        let spec = json!({
            "paths": {
                "/users": {
                    "get": {
                        "operationId": "listUsers",
                        "summary": "List all users",
                        "parameters": [
                            {"name": "limit", "in": "query", "schema": {"type": "integer"}, "required": true}
                        ]
                    },
                    "post": {
                        "operationId": "createUser",
                        "summary": "Create a user",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {"name": {"type": "string"}, "email": {"type": "string"}},
                                        "required": ["name"]
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        let tools = openapi_to_llm_tools(&spec);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "listUsers");
        assert_eq!(tools[1]["name"], "createUser");
        assert!(tools[0]["parameters"]["properties"]["limit"]["type"] == "integer");
        assert!(tools[1]["parameters"]["properties"]["name"]["type"] == "string");
    }

    #[test]
    fn cross_ref_overlapping_enums() {
        let s1 = json!({
            "properties": {
                "status": {"type": "string", "enum": ["pending", "paid", "shipped", "cancelled"]},
                "payment_status": {"type": "string", "enum": ["pending", "completed", "failed"]}
            },
            "required": ["status"]
        });
        let s2 = json!({
            "properties": {
                "status": {"type": "string", "enum": ["pending", "paid", "delivered"]},
                "payment_status": {"type": "string", "enum": ["pending", "completed", "refunded"]}
            },
            "required": ["status"]
        });
        let s3 = json!({
            "properties": {
                "status": {"type": "string", "enum": ["pending", "paid", "returned"]},
                "payment_status": {"type": "string", "enum": ["pending", "completed"]}
            },
            "required": ["status", "payment_status"]
        });
        let result = cross_reference_schemas(&[s1, s2, s3]);
        let status_enum = result
            .universal_enums
            .iter()
            .find(|e| e.field_pattern == "status");
        assert!(status_enum.is_some());
        let se = status_enum.unwrap();
        assert!(se.common_values.contains(&"pending".to_owned()));
        assert!(se.common_values.contains(&"paid".to_owned()));
        assert!(!se.common_values.contains(&"shipped".to_owned()));
        assert!(se.all_values.len() > se.common_values.len());
    }

    #[test]
    fn cross_ref_price_divergence() {
        let s1 = json!({
            "properties": {
                "price": {"type": "number", "minimum": 0, "maximum": 10000}
            }
        });
        let s2 = json!({
            "properties": {
                "price": {"type": "number", "minimum": 0, "maximum": 99999}
            }
        });
        let result = cross_reference_schemas(&[s1, s2]);
        let min_constraint = result
            .universal_constraints
            .iter()
            .find(|c| c.field_pattern == "price" && c.constraint_type == "minimum");
        assert!(min_constraint.is_some());
        assert_eq!(min_constraint.unwrap().agreement, 2);
        let max_divergence = result
            .divergences
            .iter()
            .find(|d| d.field_pattern == "price" && d.description.contains("maximum"));
        assert!(max_divergence.is_some());
    }

    #[test]
    fn cross_ref_universal_required() {
        let s1 = json!({"properties": {"id": {"type": "string"}, "name": {"type": "string"}}, "required": ["id", "name"]});
        let s2 = json!({"properties": {"id": {"type": "string"}, "name": {"type": "string"}}, "required": ["id"]});
        let s3 = json!({"properties": {"id": {"type": "string"}}, "required": ["id"]});
        let result = cross_reference_schemas(&[s1, s2, s3]);
        let id_req = result
            .universal_constraints
            .iter()
            .find(|c| c.field_pattern == "id" && c.constraint_type == "required");
        assert!(id_req.is_some());
        assert_eq!(id_req.unwrap().agreement, 3);
    }

    #[test]
    fn consistency_compatible_rules() {
        let schema = json!({
            "type": "object",
            "properties": {
                "start": {"type": "integer", "minimum": 0, "maximum": 100},
                "end": {"type": "integer", "minimum": 0, "maximum": 100}
            }
        });
        let rules = vec![FieldRelation {
            field_a: "start".into(),
            operator: "lte".into(),
            field_b: "end".into(),
        }];
        let result = check_rule_consistency(&rules, &schema);
        assert!(result.consistent);
        assert!(result.satisfying_example.is_some());
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn consistency_cycle_conflict() {
        let schema = json!({
            "type": "object",
            "properties": {
                "a": {"type": "integer", "minimum": 0, "maximum": 100},
                "b": {"type": "integer", "minimum": 0, "maximum": 100}
            }
        });
        let rules = vec![
            FieldRelation {
                field_a: "a".into(),
                operator: "lt".into(),
                field_b: "b".into(),
            },
            FieldRelation {
                field_a: "b".into(),
                operator: "lt".into(),
                field_b: "a".into(),
            },
        ];
        let result = check_rule_consistency(&rules, &schema);
        assert!(!result.consistent);
        assert!(!result.conflicts.is_empty());
    }

    #[test]
    fn consistency_range_contradiction() {
        let schema = json!({
            "type": "object",
            "properties": {
                "x": {"type": "integer", "minimum": 100, "maximum": 50}
            }
        });
        let rules: Vec<FieldRelation> = vec![];
        let result = check_rule_consistency(&rules, &schema);
        assert!(!result.consistent);
        let conflict = &result.conflicts[0];
        assert!(conflict.reason.contains("range conflict"));
    }

    #[test]
    fn consistency_arithmetic_infeasibility() {
        let schema = json!({
            "type": "object",
            "properties": {
                "total": {"type": "number", "minimum": 0, "maximum": 10},
                "a": {"type": "number", "minimum": 50, "maximum": 100},
                "b": {"type": "number", "minimum": 50, "maximum": 100}
            }
        });
        let rules = vec![FieldRelation {
            field_a: "total".into(),
            operator: "eq".into(),
            field_b: "a + b".into(),
        }];
        let result = check_rule_consistency(&rules, &schema);
        assert!(!result.consistent);
        let arith = result
            .conflicts
            .iter()
            .find(|c| c.reason.contains("arithmetic"));
        assert!(arith.is_some());
    }
}
