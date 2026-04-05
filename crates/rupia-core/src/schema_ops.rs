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
}
