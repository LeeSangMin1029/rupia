use serde_json::Value;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AveConfig {
    pub domain: String,
    pub model_tier: ModelTier,
    pub max_retries: u32,
    pub timeout: Duration,
    pub evolution_threshold: u32,
    pub workspace: PathBuf,
}

impl Default for AveConfig {
    fn default() -> Self {
        Self {
            domain: String::new(),
            model_tier: ModelTier::Sonnet,
            max_retries: 3,
            timeout: Duration::from_secs(30),
            evolution_threshold: 10,
            workspace: PathBuf::from("_workspace"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Haiku,
    Sonnet,
    Opus,
}

#[derive(Debug, Clone)]
pub struct SchemaPackage {
    pub schema: Value,
    pub relations: Vec<FieldRelation>,
    pub counterexamples: Vec<Value>,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct FieldRelation {
    pub field_a: String,
    pub operator: String,
    pub field_b: String,
}

#[derive(Debug, Clone)]
pub struct FieldValidation {
    pub field: String,
    pub status: FieldStatus,
    pub confidence: f64,
    pub coercion: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldStatus {
    Valid,
    Invalid(String),
    Coerced,
}

#[derive(Debug, Clone)]
pub struct MergeResult {
    pub data: Value,
    pub merged_fields: Vec<String>,
    pub new_errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EvolutionProposal {
    pub field: String,
    pub change_type: ChangeType,
    pub approval: ApprovalLevel,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeType {
    DescriptionEnrich,
    DefaultAdd,
    EnumAdd,
    RangeExpand,
    TypeChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalLevel {
    Auto,
    Async,
    Sync,
}

#[derive(Debug, Clone)]
pub struct PhaseTrace {
    pub phase: u8,
    pub agent: String,
    pub llm_calls: u32,
    pub duration_ms: u64,
    pub result: TraceResult,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TraceResult {
    Ok,
    Retry,
    Failed(String),
}

impl TraceResult {
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

#[derive(Debug)]
pub struct TraceBuffer {
    traces: VecDeque<PhaseTrace>,
    max_size: usize,
    failed_traces: Vec<PhaseTrace>,
}

impl TraceBuffer {
    pub fn new(max_size: usize) -> Self {
        Self {
            traces: VecDeque::new(),
            max_size,
            failed_traces: Vec::new(),
        }
    }

    pub fn push(&mut self, trace: PhaseTrace) {
        if trace.result.is_failure() {
            self.failed_traces.push(trace.clone());
        }
        self.traces.push_back(trace);
        while self.traces.len() > self.max_size {
            self.traces.pop_front();
        }
    }

    pub fn traces(&self) -> &VecDeque<PhaseTrace> {
        &self.traces
    }

    pub fn failed_traces(&self) -> &[PhaseTrace] {
        &self.failed_traces
    }
}

impl Default for TraceBuffer {
    fn default() -> Self {
        Self::new(100)
    }
}

#[derive(Debug, Clone)]
pub struct RelationViolation {
    pub field_a: String,
    pub field_b: String,
    pub operator: String,
    pub description: String,
}

pub fn compute_field_confidence(
    _field: &str,
    original: &Value,
    coerced: &Value,
    validation_passed: bool,
) -> f64 {
    if !validation_passed {
        return 0.0;
    }
    if original == coerced {
        return 1.0;
    }
    match (original, coerced) {
        (Value::String(s), Value::Number(_)) if s.parse::<f64>().is_ok() => 1.0,
        (Value::String(s), Value::String(c)) if s.trim() == c.as_str() => 1.0,
        (Value::String(s), Value::String(c)) if s.to_lowercase() == c.to_lowercase() => 0.9,
        (val, Value::Array(arr)) if arr.len() == 1 && &arr[0] == val => 0.9,
        (Value::Null, _) => 0.6,
        _ => 0.7,
    }
}

pub fn merge_fields(
    original: &Value,
    corrections: &Value,
    failed_fields: &[String],
) -> MergeResult {
    let mut merged = original.clone();
    let mut merged_fields = vec![];
    if let (Some(orig_obj), Some(corr_obj)) = (merged.as_object_mut(), corrections.as_object()) {
        for field in failed_fields {
            if let Some(new_val) = corr_obj.get(field) {
                orig_obj.insert(field.clone(), new_val.clone());
                merged_fields.push(field.clone());
            }
        }
    }
    MergeResult {
        data: merged,
        merged_fields,
        new_errors: vec![],
    }
}

fn get_field_as_f64(data: &Value, field: &str) -> Option<f64> {
    let val = data.get(field)?;
    match val {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

pub fn validate_relations(data: &Value, relations: &[FieldRelation]) -> Vec<RelationViolation> {
    let mut violations = vec![];
    for rel in relations {
        let a = get_field_as_f64(data, &rel.field_a);
        let b = get_field_as_f64(data, &rel.field_b);
        let violated = match (a, b) {
            (Some(va), Some(vb)) => match rel.operator.as_str() {
                "gte" | ">=" => va < vb,
                "lte" | "<=" => va > vb,
                "eq" | "==" => (va - vb).abs() > f64::EPSILON,
                "gt" | ">" => va <= vb,
                "lt" | "<" => va >= vb,
                _ => false,
            },
            _ => false,
        };
        if violated {
            violations.push(RelationViolation {
                field_a: rel.field_a.clone(),
                field_b: rel.field_b.clone(),
                operator: rel.operator.clone(),
                description: format!("{} must be {} {}", rel.field_a, rel.operator, rel.field_b),
            });
        }
    }
    violations
}

pub fn generate_schema_prompt(domain: &str) -> String {
    format!(
        r#"You are a schema architect. Given a domain description, generate a COMPLETE validation package in a single JSON response.

Domain: "{domain}"

Return a single JSON object with exactly these 3 keys:
1. "schema": Full JSON Schema (type, properties, required, enums, formats, min/max, patterns)
2. "relations": Array of cross-field rules [{{"field_a": "...", "operator": "...", "field_b": "..."}}]
3. "counterexamples": Array of 3 invalid objects, each with a "violation" field

Return ONLY the JSON. No markdown, no explanation."#
    )
}

pub fn summarize_schema(schema: &Value, tier: ModelTier) -> Result<String, String> {
    let props = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "schema has no properties".to_string())?;
    let required: Vec<&str> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    match tier {
        ModelTier::Haiku => {
            let parts: Vec<String> = props
                .iter()
                .map(|(name, prop)| {
                    let typ = prop.get("type").and_then(|v| v.as_str()).unwrap_or("any");
                    let fmt = prop.get("format").and_then(|v| v.as_str());
                    let req = if required.contains(&name.as_str()) {
                        ", required"
                    } else {
                        ""
                    };
                    let range = format_range(prop);
                    let display_type = fmt.unwrap_or(typ);
                    format!("{name}({display_type}{range}{req})")
                })
                .collect();
            Ok(parts.join(", "))
        }
        ModelTier::Sonnet => {
            let parts: Vec<String> = props
                .iter()
                .map(|(name, prop)| {
                    let typ = prop.get("type").and_then(|v| v.as_str()).unwrap_or("any");
                    let mut attrs = vec![typ.to_string()];
                    if let Some(fmt) = prop.get("format").and_then(|v| v.as_str()) {
                        attrs.push(format!("format={fmt}"));
                    }
                    let range = format_range(prop);
                    if !range.is_empty() {
                        attrs.push(range.trim_start_matches(", ").to_string());
                    }
                    if required.contains(&name.as_str()) {
                        attrs.push("required".to_string());
                    }
                    format!("{name}: {}", attrs.join(", "))
                })
                .collect();
            Ok(parts.join(" | "))
        }
        ModelTier::Opus => {
            let mut compact = schema.clone();
            strip_verbose_fields(&mut compact);
            serde_json::to_string_pretty(&compact).map_err(|e| e.to_string())
        }
    }
}

fn format_range(prop: &Value) -> String {
    let min = prop.get("minimum").and_then(Value::as_f64);
    let max = prop.get("maximum").and_then(Value::as_f64);
    match (min, max) {
        (Some(lo), Some(hi)) => format!(", {lo}-{hi}"),
        (Some(lo), None) => format!(", min={lo}"),
        (None, Some(hi)) => format!(", max={hi}"),
        (None, None) => String::new(),
    }
}

fn strip_verbose_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("description");
            map.remove("examples");
            for v in map.values_mut() {
                strip_verbose_fields(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_verbose_fields(v);
            }
        }
        _ => {}
    }
}

pub fn parse_schema_package(raw: &str) -> Result<SchemaPackage, String> {
    let parsed: Value = serde_json::from_str(raw).map_err(|e| format!("invalid JSON: {e}"))?;
    let schema = parsed
        .get("schema")
        .cloned()
        .ok_or("missing 'schema' key")?;
    let relations = parsed
        .get("relations")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    Some(FieldRelation {
                        field_a: r.get("field_a")?.as_str()?.to_string(),
                        operator: r.get("operator")?.as_str()?.to_string(),
                        field_b: r.get("field_b")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let counterexamples = parsed
        .get("counterexamples")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(SchemaPackage {
        schema,
        relations,
        counterexamples,
        summary: String::new(),
    })
}

pub fn lint_schema_value(schema: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    if schema.get("type").is_none()
        && schema.get("anyOf").is_none()
        && schema.get("oneOf").is_none()
        && schema.get("$ref").is_none()
    {
        errors.push("schema has no top-level type constraint".into());
    }
    if let Some(obj) = schema.as_object() {
        if obj.get("properties").is_some() && obj.get("required").is_none() {
            errors.push("schema has properties but no 'required' array".into());
        }
        if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
            for (name, prop) in props {
                if prop.get("type").is_none()
                    && prop.get("anyOf").is_none()
                    && prop.get("oneOf").is_none()
                    && prop.get("$ref").is_none()
                    && prop.get("enum").is_none()
                    && prop.get("const").is_none()
                {
                    errors.push(format!("property '{name}' has no type constraint"));
                }
            }
        }
    }
    errors
}

pub fn validate_with_confidence(
    data: &Value,
    schema: &Value,
    relations: &[FieldRelation],
) -> Vec<FieldValidation> {
    let coerced = crate::coerce::coerce_with_schema(data.clone(), schema);
    let validation = crate::validator::validate(&coerced, schema);
    let failed_fields: Vec<String> = match &validation {
        crate::types::Validation::Success(_) => vec![],
        crate::types::Validation::Failure(f) => f
            .errors
            .iter()
            .map(|e| {
                e.path
                    .strip_prefix("$input.")
                    .unwrap_or(&e.path)
                    .to_string()
            })
            .collect(),
    };
    let props = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut results: Vec<FieldValidation> = props
        .keys()
        .map(|field| {
            let orig_val = data.get(field).unwrap_or(&Value::Null);
            let coerced_val = coerced.get(field).unwrap_or(&Value::Null);
            let is_failed = failed_fields
                .iter()
                .any(|f| f == field || f.starts_with(&format!("{field}.")));
            let passed = !is_failed;
            let confidence = compute_field_confidence(field, orig_val, coerced_val, passed);
            let status = if is_failed {
                FieldStatus::Invalid(
                    failed_fields
                        .iter()
                        .find(|f| f.as_str() == field || f.starts_with(&format!("{field}.")))
                        .cloned()
                        .unwrap_or_default(),
                )
            } else if orig_val != coerced_val {
                FieldStatus::Coerced
            } else {
                FieldStatus::Valid
            };
            let coercion = if orig_val != coerced_val && passed {
                Some(coerced_val.to_string())
            } else {
                None
            };
            FieldValidation {
                field: field.clone(),
                status,
                confidence,
                coercion,
            }
        })
        .collect();
    let relation_violations = validate_relations(&coerced, relations);
    for v in &relation_violations {
        if let Some(fv) = results.iter_mut().find(|r| r.field == v.field_a) {
            if fv.status == FieldStatus::Valid || matches!(fv.status, FieldStatus::Coerced) {
                fv.status = FieldStatus::Invalid(v.description.clone());
                fv.confidence = 0.0;
            }
        }
    }
    results
}

pub fn detect_field_groups(
    validations: &[FieldValidation],
    relations: &[FieldRelation],
) -> Vec<Vec<String>> {
    let failed: Vec<&str> = validations
        .iter()
        .filter(|v| matches!(v.status, FieldStatus::Invalid(_)))
        .map(|v| v.field.as_str())
        .collect();
    if failed.is_empty() {
        return vec![];
    }
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut assigned: Vec<bool> = vec![false; failed.len()];
    for (i, f) in failed.iter().enumerate() {
        if assigned[i] {
            continue;
        }
        let mut group = vec![f.to_string()];
        assigned[i] = true;
        for (j, g) in failed.iter().enumerate() {
            if assigned[j] {
                continue;
            }
            let related = relations.iter().any(|r| {
                (r.field_a == *f && r.field_b == *g) || (r.field_a == *g && r.field_b == *f)
            });
            if related {
                group.push(g.to_string());
                assigned[j] = true;
            }
        }
        groups.push(group);
    }
    groups
}

pub fn build_selective_prompt(
    schema_summary: &str,
    failed_fields: &[String],
    original_data: &Value,
    errors: &[FieldValidation],
) -> String {
    let field_errors: Vec<String> = errors
        .iter()
        .filter(|e| failed_fields.contains(&e.field) && matches!(e.status, FieldStatus::Invalid(_)))
        .map(|e| {
            let reason = match &e.status {
                FieldStatus::Invalid(msg) => msg.as_str(),
                _ => "unknown",
            };
            format!("  - {}: {}", e.field, reason)
        })
        .collect();
    let current_values: Vec<String> = failed_fields
        .iter()
        .filter_map(|f| original_data.get(f).map(|v| format!("  - {f}: {v}")))
        .collect();
    format!(
        "Fix ONLY these fields. Return a JSON object with corrected values.\n\n\
         Schema: {schema_summary}\n\n\
         Failed fields:\n{}\n\n\
         Current values:\n{}\n\n\
         Return ONLY a JSON object with the corrected fields. No explanation.",
        field_errors.join("\n"),
        current_values.join("\n"),
    )
}

pub fn selective_retry(
    original: &Value,
    corrections_raw: &str,
    failed_fields: &[String],
    schema: &Value,
    relations: &[FieldRelation],
) -> Result<MergeResult, String> {
    let corrections: Value =
        serde_json::from_str(corrections_raw).map_err(|e| format!("invalid JSON: {e}"))?;
    let merged = merge_fields(original, &corrections, failed_fields);
    let post_validations = validate_with_confidence(&merged.data, schema, relations);
    let new_errors: Vec<String> = post_validations
        .iter()
        .filter(|v| matches!(v.status, FieldStatus::Invalid(_)))
        .map(|v| v.field.clone())
        .collect();
    if !new_errors.is_empty() {
        let only_new = new_errors
            .iter()
            .filter(|e| !failed_fields.contains(e))
            .cloned()
            .collect::<Vec<_>>();
        if !only_new.is_empty() {
            return Err(format!(
                "merge introduced new errors: {}",
                only_new.join(", ")
            ));
        }
    }
    Ok(MergeResult {
        data: merged.data,
        merged_fields: merged.merged_fields,
        new_errors,
    })
}

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TraceAnalysis {
    pub total_runs: usize,
    pub failure_rate: f64,
    pub field_failure_counts: HashMap<String, usize>,
    pub most_failed_field: Option<String>,
    pub stalled: bool,
}

pub fn analyze_traces(buffer: &TraceBuffer) -> TraceAnalysis {
    let total = buffer.traces().len();
    if total == 0 {
        return TraceAnalysis {
            total_runs: 0,
            failure_rate: 0.0,
            field_failure_counts: HashMap::new(),
            most_failed_field: None,
            stalled: false,
        };
    }
    let failures = buffer
        .traces()
        .iter()
        .filter(|t| t.result.is_failure())
        .count();
    #[allow(clippy::cast_precision_loss)]
    let failure_rate = failures as f64 / total as f64;
    let mut field_counts: HashMap<String, usize> = HashMap::new();
    for t in buffer.failed_traces() {
        if let TraceResult::Failed(msg) = &t.result {
            *field_counts.entry(msg.clone()).or_default() += 1;
        }
    }
    let most_failed = field_counts
        .iter()
        .max_by_key(|(_, c)| *c)
        .map(|(f, _)| f.clone());
    let stalled = if buffer.traces().len() >= 3 {
        let last_3: Vec<_> = buffer.traces().iter().rev().take(3).collect();
        last_3.iter().all(|t| t.result.is_failure())
            && last_3.windows(2).all(|w| w[0].result == w[1].result)
    } else {
        false
    };
    TraceAnalysis {
        total_runs: total,
        failure_rate,
        field_failure_counts: field_counts,
        most_failed_field: most_failed,
        stalled,
    }
}

pub fn propose_evolution(
    schema: &Value,
    analysis: &TraceAnalysis,
    threshold: u32,
) -> Vec<EvolutionProposal> {
    if analysis.total_runs < threshold as usize {
        return vec![];
    }
    let mut proposals = Vec::new();
    let props = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (field, count) in &analysis.field_failure_counts {
        #[allow(clippy::cast_precision_loss)]
        let rate = *count as f64 / analysis.total_runs as f64;
        let pct = rate * 100.0;
        if rate < 0.1 {
            continue;
        }
        let has_description = props
            .get(field)
            .and_then(|p| p.get("description"))
            .is_some();
        if !has_description && rate >= 0.1 {
            proposals.push(EvolutionProposal {
                field: field.clone(),
                change_type: ChangeType::DescriptionEnrich,
                approval: ApprovalLevel::Auto,
                description: format!("add description to '{field}' (failure rate: {pct:.0}%)"),
            });
        }
        let has_default = props.get(field).and_then(|p| p.get("default")).is_some();
        if !has_default && rate >= 0.2 {
            proposals.push(EvolutionProposal {
                field: field.clone(),
                change_type: ChangeType::DefaultAdd,
                approval: ApprovalLevel::Auto,
                description: format!("add default for '{field}' (failure rate: {pct:.0}%)"),
            });
        }
        if rate >= 0.3 {
            let has_enum = props.get(field).and_then(|p| p.get("enum")).is_some();
            if has_enum {
                proposals.push(EvolutionProposal {
                    field: field.clone(),
                    change_type: ChangeType::EnumAdd,
                    approval: ApprovalLevel::Async,
                    description: format!(
                        "consider expanding enum for '{field}' (failure rate: {pct:.0}%)"
                    ),
                });
            }
        }
        if rate >= 0.4 {
            let has_range = props
                .get(field)
                .is_some_and(|p| p.get("minimum").is_some() || p.get("maximum").is_some());
            if has_range {
                proposals.push(EvolutionProposal {
                    field: field.clone(),
                    change_type: ChangeType::RangeExpand,
                    approval: ApprovalLevel::Async,
                    description: format!(
                        "consider expanding range for '{field}' (failure rate: {pct:.0}%)"
                    ),
                });
            }
        }
        if rate >= 0.5 {
            proposals.push(EvolutionProposal {
                field: field.clone(),
                change_type: ChangeType::TypeChange,
                approval: ApprovalLevel::Sync,
                description: format!(
                    "type change needed for '{field}' (failure rate: {pct:.0}%) — requires manual approval"
                ),
            });
        }
    }
    proposals
}

pub fn schema_resolve(raw_llm_output: &str, config: &AveConfig) -> Result<SchemaPackage, String> {
    let mut pkg = parse_schema_package(raw_llm_output)?;
    let lint_errors = lint_schema_value(&pkg.schema);
    if !lint_errors.is_empty() {
        return Err(format!("schema lint failed: {}", lint_errors.join("; ")));
    }
    let summary = summarize_schema(&pkg.schema, config.model_tier)?;
    pkg.summary = summary;
    Ok(pkg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assert_f64_eq(a: f64, b: f64) {
        assert!((a - b).abs() < f64::EPSILON, "{a} != {b}");
    }

    #[test]
    fn confidence_exact_match() {
        assert_f64_eq(
            compute_field_confidence("f", &json!(42), &json!(42), true),
            1.0,
        );
    }

    #[test]
    fn confidence_failed_validation() {
        assert_f64_eq(
            compute_field_confidence("f", &json!(42), &json!(42), false),
            0.0,
        );
    }

    #[test]
    fn confidence_string_to_number() {
        assert_f64_eq(
            compute_field_confidence("f", &json!("42"), &json!(42), true),
            1.0,
        );
    }

    #[test]
    fn confidence_trim_coerce() {
        assert_f64_eq(
            compute_field_confidence("f", &json!("  hello  "), &json!("hello"), true),
            1.0,
        );
    }

    #[test]
    fn confidence_case_coerce() {
        let c = compute_field_confidence("f", &json!("Hello"), &json!("hello"), true);
        assert!((c - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_wrap_in_array() {
        let c = compute_field_confidence("f", &json!("x"), &json!(["x"]), true);
        assert!((c - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_null_default() {
        let c = compute_field_confidence("f", &Value::Null, &json!("default"), true);
        assert!((c - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_unknown_coerce() {
        let c = compute_field_confidence("f", &json!("abc"), &json!(123), true);
        assert!((c - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn merge_replaces_failed_fields() {
        let original = json!({"name": "bad", "age": 25});
        let corrections = json!({"name": "good", "extra": "ignored"});
        let result = merge_fields(&original, &corrections, &["name".into()]);
        assert_eq!(result.data["name"], "good");
        assert_eq!(result.data["age"], 25);
        assert_eq!(result.merged_fields, vec!["name"]);
    }

    #[test]
    fn merge_missing_correction_skipped() {
        let original = json!({"a": 1});
        let corrections = json!({"b": 2});
        let result = merge_fields(&original, &corrections, &["a".into()]);
        assert_eq!(result.data["a"], 1);
        assert!(result.merged_fields.is_empty());
    }

    #[test]
    fn validate_relations_gte_pass() {
        let data = json!({"end": 10, "start": 5});
        let rels = vec![FieldRelation {
            field_a: "end".into(),
            operator: "gte".into(),
            field_b: "start".into(),
        }];
        assert!(validate_relations(&data, &rels).is_empty());
    }

    #[test]
    fn validate_relations_gte_fail() {
        let data = json!({"end": 3, "start": 5});
        let rels = vec![FieldRelation {
            field_a: "end".into(),
            operator: "gte".into(),
            field_b: "start".into(),
        }];
        let v = validate_relations(&data, &rels);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].field_a, "end");
    }

    #[test]
    fn validate_relations_eq() {
        let data = json!({"a": 5, "b": 5});
        let rels = vec![FieldRelation {
            field_a: "a".into(),
            operator: "eq".into(),
            field_b: "b".into(),
        }];
        assert!(validate_relations(&data, &rels).is_empty());
    }

    #[test]
    fn validate_relations_missing_field() {
        let data = json!({"a": 5});
        let rels = vec![FieldRelation {
            field_a: "a".into(),
            operator: "gte".into(),
            field_b: "missing".into(),
        }];
        assert!(validate_relations(&data, &rels).is_empty());
    }

    #[test]
    fn trace_buffer_ring() {
        let mut buf = TraceBuffer::new(3);
        for i in 0_u8..5 {
            buf.push(PhaseTrace {
                phase: i,
                agent: "test".into(),
                llm_calls: 1,
                duration_ms: 100,
                result: TraceResult::Ok,
                timestamp: String::new(),
            });
        }
        assert_eq!(buf.traces().len(), 3);
        assert_eq!(buf.traces()[0].phase, 2);
    }

    #[test]
    fn trace_buffer_preserves_failures() {
        let mut buf = TraceBuffer::new(2);
        buf.push(PhaseTrace {
            phase: 0,
            agent: "test".into(),
            llm_calls: 1,
            duration_ms: 100,
            result: TraceResult::Failed("err".into()),
            timestamp: String::new(),
        });
        buf.push(PhaseTrace {
            phase: 1,
            agent: "test".into(),
            llm_calls: 1,
            duration_ms: 100,
            result: TraceResult::Ok,
            timestamp: String::new(),
        });
        buf.push(PhaseTrace {
            phase: 2,
            agent: "test".into(),
            llm_calls: 1,
            duration_ms: 100,
            result: TraceResult::Ok,
            timestamp: String::new(),
        });
        assert_eq!(buf.traces().len(), 2);
        assert_eq!(buf.failed_traces().len(), 1);
        assert!(buf.failed_traces()[0].result.is_failure());
    }

    #[test]
    fn summarize_haiku() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "email": {"type": "string", "format": "email"},
                "age": {"type": "integer", "minimum": 0, "maximum": 150}
            },
            "required": ["name", "email"]
        });
        let summary = summarize_schema(&schema, ModelTier::Haiku).unwrap();
        assert!(summary.contains("name(string, required)"));
        assert!(summary.contains("email(email, required)"));
        assert!(summary.contains("age(integer, 0-150)"));
    }

    #[test]
    fn summarize_sonnet() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer", "minimum": 0}
            },
            "required": ["name"]
        });
        let summary = summarize_schema(&schema, ModelTier::Sonnet).unwrap();
        assert!(summary.contains("name: string, required"));
        assert!(summary.contains("age: integer, min=0"));
    }

    #[test]
    fn summarize_opus_strips_descriptions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "The user name", "examples": ["Alice"]}
            },
            "required": ["name"]
        });
        let summary = summarize_schema(&schema, ModelTier::Opus).unwrap();
        assert!(!summary.contains("The user name"));
        assert!(!summary.contains("Alice"));
        assert!(summary.contains("\"type\": \"string\""));
    }

    #[test]
    fn parse_schema_package_valid() {
        let raw = r#"{
            "schema": {"type": "object", "properties": {"name": {"type": "string"}}},
            "relations": [{"field_a": "end", "operator": "gte", "field_b": "start"}],
            "counterexamples": [{"name": 123, "violation": "name must be string"}]
        }"#;
        let pkg = parse_schema_package(raw).unwrap();
        assert!(pkg.schema.get("properties").is_some());
        assert_eq!(pkg.relations.len(), 1);
        assert_eq!(pkg.relations[0].operator, "gte");
        assert_eq!(pkg.counterexamples.len(), 1);
    }

    #[test]
    fn parse_schema_package_missing_schema() {
        let raw = r#"{"relations": []}"#;
        assert!(parse_schema_package(raw).is_err());
    }

    #[test]
    fn parse_schema_package_invalid_json() {
        assert!(parse_schema_package("not json").is_err());
    }

    #[test]
    fn generate_prompt_contains_domain() {
        let prompt = generate_schema_prompt("online shop");
        assert!(prompt.contains("online shop"));
        assert!(prompt.contains("schema architect"));
    }

    #[test]
    fn lint_schema_valid() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        });
        assert!(lint_schema_value(&schema).is_empty());
    }

    #[test]
    fn lint_schema_no_type() {
        let schema = json!({"properties": {"name": {"type": "string"}}});
        let errs = lint_schema_value(&schema);
        assert!(errs.iter().any(|e| e.contains("top-level type")));
    }

    #[test]
    fn lint_schema_no_required() {
        let schema = json!({"type": "object", "properties": {"x": {"type": "string"}}});
        let errs = lint_schema_value(&schema);
        assert!(errs.iter().any(|e| e.contains("required")));
    }

    #[test]
    fn lint_schema_property_no_type() {
        let schema = json!({
            "type": "object",
            "properties": {"bad": {}},
            "required": ["bad"]
        });
        let errs = lint_schema_value(&schema);
        assert!(errs.iter().any(|e| e.contains("bad")));
    }

    #[test]
    fn schema_resolve_valid() {
        let raw = r#"{
            "schema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            },
            "relations": [],
            "counterexamples": []
        }"#;
        let config = AveConfig {
            model_tier: ModelTier::Haiku,
            ..Default::default()
        };
        let pkg = schema_resolve(raw, &config).unwrap();
        assert!(!pkg.summary.is_empty());
        assert!(pkg.summary.contains("name"));
    }

    #[test]
    fn schema_resolve_lint_fail() {
        let raw = r#"{
            "schema": {"properties": {"x": {}}},
            "relations": [],
            "counterexamples": []
        }"#;
        let config = AveConfig::default();
        assert!(schema_resolve(raw, &config).is_err());
    }

    #[test]
    fn validate_with_confidence_all_valid() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer", "minimum": 0}
            },
            "required": ["name", "age"]
        });
        let data = json!({"name": "Alice", "age": 30});
        let results = validate_with_confidence(&data, &schema, &[]);
        assert!(results.iter().all(|r| r.status == FieldStatus::Valid));
        assert!(results
            .iter()
            .all(|r| (r.confidence - 1.0).abs() < f64::EPSILON));
    }

    #[test]
    fn validate_with_confidence_coerced() {
        let schema = json!({
            "type": "object",
            "properties": {
                "age": {"type": "number"}
            },
            "required": ["age"]
        });
        let data = json!({"age": "25"});
        let results = validate_with_confidence(&data, &schema, &[]);
        let age = results.iter().find(|r| r.field == "age").unwrap();
        assert_eq!(age.status, FieldStatus::Coerced);
        assert!(age.coercion.is_some());
        assert_f64_eq(age.confidence, 1.0);
    }

    #[test]
    fn validate_with_confidence_invalid() {
        let schema = json!({
            "type": "object",
            "properties": {
                "age": {"type": "integer", "minimum": 0}
            },
            "required": ["age"]
        });
        let data = json!({"age": -5});
        let results = validate_with_confidence(&data, &schema, &[]);
        let age = results.iter().find(|r| r.field == "age").unwrap();
        assert!(matches!(age.status, FieldStatus::Invalid(_)));
        assert_f64_eq(age.confidence, 0.0);
    }

    #[test]
    fn validate_with_confidence_relation_violation() {
        let schema = json!({
            "type": "object",
            "properties": {
                "start": {"type": "integer"},
                "end": {"type": "integer"}
            },
            "required": ["start", "end"]
        });
        let data = json!({"start": 10, "end": 5});
        let rels = vec![FieldRelation {
            field_a: "end".into(),
            operator: "gte".into(),
            field_b: "start".into(),
        }];
        let results = validate_with_confidence(&data, &schema, &rels);
        let end_field = results.iter().find(|r| r.field == "end").unwrap();
        assert!(matches!(end_field.status, FieldStatus::Invalid(_)));
        assert_f64_eq(end_field.confidence, 0.0);
    }

    #[test]
    fn detect_field_groups_independent() {
        let validations = vec![
            FieldValidation {
                field: "a".into(),
                status: FieldStatus::Invalid("err".into()),
                confidence: 0.0,
                coercion: None,
            },
            FieldValidation {
                field: "b".into(),
                status: FieldStatus::Invalid("err".into()),
                confidence: 0.0,
                coercion: None,
            },
        ];
        let groups = detect_field_groups(&validations, &[]);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn detect_field_groups_related() {
        let validations = vec![
            FieldValidation {
                field: "start".into(),
                status: FieldStatus::Invalid("err".into()),
                confidence: 0.0,
                coercion: None,
            },
            FieldValidation {
                field: "end".into(),
                status: FieldStatus::Invalid("err".into()),
                confidence: 0.0,
                coercion: None,
            },
        ];
        let rels = vec![FieldRelation {
            field_a: "end".into(),
            operator: "gte".into(),
            field_b: "start".into(),
        }];
        let groups = detect_field_groups(&validations, &rels);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
    }

    #[test]
    fn detect_field_groups_no_failures() {
        let validations = vec![FieldValidation {
            field: "a".into(),
            status: FieldStatus::Valid,
            confidence: 1.0,
            coercion: None,
        }];
        assert!(detect_field_groups(&validations, &[]).is_empty());
    }

    #[test]
    fn build_selective_prompt_format() {
        let errors = vec![FieldValidation {
            field: "age".into(),
            status: FieldStatus::Invalid("minimum 0".into()),
            confidence: 0.0,
            coercion: None,
        }];
        let data = json!({"age": -5, "name": "ok"});
        let prompt = build_selective_prompt("age: integer, min=0", &["age".into()], &data, &errors);
        assert!(prompt.contains("age"));
        assert!(prompt.contains("minimum 0"));
        assert!(prompt.contains("-5"));
        assert!(!prompt.contains("name"));
    }

    #[test]
    fn selective_retry_success() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer", "minimum": 0}
            },
            "required": ["name", "age"]
        });
        let original = json!({"name": "Alice", "age": -5});
        let corrections = r#"{"age": 25}"#;
        let result =
            selective_retry(&original, corrections, &["age".into()], &schema, &[]).unwrap();
        assert_eq!(result.data["age"], 25);
        assert_eq!(result.data["name"], "Alice");
        assert_eq!(result.merged_fields, vec!["age"]);
    }

    #[test]
    fn selective_retry_new_error_rejected() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer", "minimum": 0}
            },
            "required": ["name", "age"]
        });
        let original = json!({"name": "Alice", "age": -5});
        let corrections = r#"{"age": 25, "name": 123}"#;
        let result = selective_retry(
            &original,
            corrections,
            &["age".into(), "name".into()],
            &schema,
            &[],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn selective_retry_invalid_json() {
        let schema = json!({"type": "object", "properties": {}, "required": []});
        let result = selective_retry(&json!({}), "not json", &[], &schema, &[]);
        assert!(result.is_err());
    }

    fn make_trace(phase: u8, result: TraceResult) -> PhaseTrace {
        PhaseTrace {
            phase,
            agent: "test".into(),
            llm_calls: 1,
            duration_ms: 100,
            result,
            timestamp: String::new(),
        }
    }

    #[test]
    fn analyze_traces_empty() {
        let buf = TraceBuffer::new(10);
        let a = analyze_traces(&buf);
        assert_eq!(a.total_runs, 0);
        assert_f64_eq(a.failure_rate, 0.0);
    }

    #[test]
    fn analyze_traces_mixed() {
        let mut buf = TraceBuffer::new(10);
        buf.push(make_trace(0, TraceResult::Ok));
        buf.push(make_trace(1, TraceResult::Failed("age".into())));
        buf.push(make_trace(2, TraceResult::Ok));
        buf.push(make_trace(3, TraceResult::Failed("age".into())));
        let a = analyze_traces(&buf);
        assert_eq!(a.total_runs, 4);
        assert_f64_eq(a.failure_rate, 0.5);
        assert_eq!(a.most_failed_field, Some("age".into()));
        assert_eq!(*a.field_failure_counts.get("age").unwrap(), 2);
        assert!(!a.stalled);
    }

    #[test]
    fn analyze_traces_stalled() {
        let mut buf = TraceBuffer::new(10);
        buf.push(make_trace(0, TraceResult::Failed("x".into())));
        buf.push(make_trace(1, TraceResult::Failed("x".into())));
        buf.push(make_trace(2, TraceResult::Failed("x".into())));
        let a = analyze_traces(&buf);
        assert!(a.stalled);
    }

    #[test]
    fn propose_evolution_below_threshold() {
        let schema =
            json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]});
        let analysis = TraceAnalysis {
            total_runs: 5,
            failure_rate: 0.5,
            field_failure_counts: HashMap::from([("a".into(), 3)]),
            most_failed_field: Some("a".into()),
            stalled: false,
        };
        let proposals = propose_evolution(&schema, &analysis, 10);
        assert!(proposals.is_empty());
    }

    #[test]
    fn propose_evolution_description_enrich() {
        let schema = json!({
            "type": "object",
            "properties": {"age": {"type": "integer"}},
            "required": ["age"]
        });
        let analysis = TraceAnalysis {
            total_runs: 10,
            failure_rate: 0.2,
            field_failure_counts: HashMap::from([("age".into(), 2)]),
            most_failed_field: Some("age".into()),
            stalled: false,
        };
        let proposals = propose_evolution(&schema, &analysis, 10);
        assert!(proposals
            .iter()
            .any(|p| p.change_type == ChangeType::DescriptionEnrich
                && p.approval == ApprovalLevel::Auto));
    }

    #[test]
    fn propose_evolution_type_change_sync() {
        let schema = json!({
            "type": "object",
            "properties": {"val": {"type": "string"}},
            "required": ["val"]
        });
        let analysis = TraceAnalysis {
            total_runs: 10,
            failure_rate: 0.6,
            field_failure_counts: HashMap::from([("val".into(), 6)]),
            most_failed_field: Some("val".into()),
            stalled: false,
        };
        let proposals = propose_evolution(&schema, &analysis, 10);
        assert!(proposals
            .iter()
            .any(|p| p.change_type == ChangeType::TypeChange && p.approval == ApprovalLevel::Sync));
    }

    #[test]
    fn propose_evolution_range_expand_async() {
        let schema = json!({
            "type": "object",
            "properties": {"score": {"type": "integer", "minimum": 0, "maximum": 100}},
            "required": ["score"]
        });
        let analysis = TraceAnalysis {
            total_runs: 10,
            failure_rate: 0.4,
            field_failure_counts: HashMap::from([("score".into(), 4)]),
            most_failed_field: Some("score".into()),
            stalled: false,
        };
        let proposals = propose_evolution(&schema, &analysis, 10);
        assert!(proposals.iter().any(
            |p| p.change_type == ChangeType::RangeExpand && p.approval == ApprovalLevel::Async
        ));
    }
}
