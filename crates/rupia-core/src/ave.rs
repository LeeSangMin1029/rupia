use serde_json::Value;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
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
    pub rules: Vec<JsonLogicRule>,
    pub counterexamples: Vec<Value>,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct JsonLogicRule {
    pub description: String,
    pub logic: Value,
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

const RULE_EVAL_TIMEOUT_MS: u128 = 50;
const RULE_MAX_JSON_SIZE: usize = 1_048_576;

#[derive(Debug, Clone)]
pub struct RuleViolation {
    pub description: String,
    pub rule: Value,
}

pub fn validate_rules(data: &Value, rules: &[JsonLogicRule]) -> Vec<RuleViolation> {
    if rules.is_empty() {
        return vec![];
    }
    let data_size = serde_json::to_string(data).map(|s| s.len()).unwrap_or(0);
    if data_size > RULE_MAX_JSON_SIZE {
        return vec![RuleViolation {
            description: format!(
                "data exceeds max size for rule evaluation ({data_size} > {RULE_MAX_JSON_SIZE})"
            ),
            rule: Value::Null,
        }];
    }
    let engine = datalogic_rs::DataLogic::new();
    let mut violations = vec![];
    for rule in rules {
        let Ok(compiled) = engine.compile(&rule.logic) else {
            violations.push(RuleViolation {
                description: format!("rule compile error: {}", rule.description),
                rule: rule.logic.clone(),
            });
            continue;
        };
        let start = std::time::Instant::now();
        let result = engine.evaluate_owned(&compiled, data.clone());
        if start.elapsed().as_millis() > RULE_EVAL_TIMEOUT_MS {
            violations.push(RuleViolation {
                description: format!("rule evaluation timed out: {}", rule.description),
                rule: rule.logic.clone(),
            });
            continue;
        }
        match result {
            Ok(val) => {
                let passed = match &val {
                    Value::Bool(b) => *b,
                    Value::Null => false,
                    Value::Number(n) => n.as_f64().is_some_and(|v| v != 0.0),
                    Value::String(s) => !s.is_empty(),
                    Value::Array(a) => !a.is_empty(),
                    Value::Object(_) => true,
                };
                if !passed {
                    violations.push(RuleViolation {
                        description: rule.description.clone(),
                        rule: rule.logic.clone(),
                    });
                }
            }
            Err(_) => {
                violations.push(RuleViolation {
                    description: format!("rule evaluation error: {}", rule.description),
                    rule: rule.logic.clone(),
                });
            }
        }
    }
    violations
}

pub fn generate_schema_prompt(domain: &str) -> String {
    format!(
        r#"You are a schema architect. Given a domain description, generate a COMPLETE validation package in a single JSON response.

Domain: "{domain}"

Return a single JSON object with exactly these 4 keys:
1. "schema": Full JSON Schema (type, properties, required, enums, formats, min/max, patterns)
2. "relations": Array of simple cross-field rules [{{"field_a": "...", "operator": ">=", "field_b": "..."}}]
3. "rules": Array of JSONLogic rules for complex constraints [{{"description": "...", "logic": {{JSONLogic expression}}}}]
   Examples: {{"description": "shipped requires tracking", "logic": {{"if": [{{"==": [{{"var": "status"}}, "shipped"]}}, {{"!!": {{"var": "tracking_number"}}}}, true]}}}}
   {{"description": "total equals subtotal + tax", "logic": {{"==": [{{"var": "total"}}, {{"+": [{{"var": "subtotal"}}, {{"var": "tax"}}]}}]}}}}
4. "counterexamples": Array of 3 invalid objects, each with a "violation" field

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
    let rules = parsed
        .get("rules")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    let logic = r.get("logic")?.clone();
                    let description = r
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("unnamed rule")
                        .to_string();
                    Some(JsonLogicRule { description, logic })
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
        rules,
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
    let strictness = check_strictness(&pkg.schema);
    if !strictness.passed {
        let msgs: Vec<String> = strictness
            .violations
            .iter()
            .filter(|v| v.severity == ViolationSeverity::Block)
            .map(|v| format!("[{}] {}: {}", v.code, v.path, v.fix))
            .collect();
        return Err(format!("schema strictness failed: {}", msgs.join("; ")));
    }
    let summary = summarize_schema(&pkg.schema, config.model_tier)?;
    pkg.summary = summary;
    Ok(pkg)
}

#[derive(Debug, Clone)]
pub struct SchemaVersion {
    pub version: u32,
    pub timestamp: String,
    pub source: SchemaSource,
    pub schema: Value,
    pub changes: Vec<VersionChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaSource {
    Generated,
    AutoEvolution,
    Manual,
}

#[derive(Debug, Clone)]
pub struct VersionChange {
    pub field: String,
    pub change_type: String,
    pub description: String,
}

#[derive(Debug)]
pub struct SchemaVersionStore {
    versions: Vec<SchemaVersion>,
    current: u32,
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "schema version counts won't exceed u32::MAX"
)]
impl SchemaVersionStore {
    pub fn new(initial_schema: Value) -> Self {
        let v = SchemaVersion {
            version: 1,
            timestamp: current_timestamp(),
            source: SchemaSource::Generated,
            schema: initial_schema,
            changes: vec![],
        };
        Self {
            versions: vec![v],
            current: 1,
        }
    }

    pub fn current_schema(&self) -> &Value {
        &self.versions[self.current as usize - 1].schema
    }

    pub fn current_version(&self) -> u32 {
        self.current
    }

    pub fn push(
        &mut self,
        schema: Value,
        source: SchemaSource,
        changes: Vec<VersionChange>,
    ) -> u32 {
        let next = self.versions.len() as u32 + 1;
        self.versions.push(SchemaVersion {
            version: next,
            timestamp: current_timestamp(),
            source,
            schema,
            changes,
        });
        self.current = next;
        next
    }

    pub fn rollback(&mut self, target_version: u32) -> Result<(), String> {
        if target_version == 0 || target_version > self.versions.len() as u32 {
            return Err(format!(
                "version {target_version} out of range (1..={})",
                self.versions.len()
            ));
        }
        self.current = target_version;
        Ok(())
    }

    pub fn diff(&self, from: u32, to: u32) -> Result<crate::schema_ops::SchemaDiff, String> {
        let max = self.versions.len() as u32;
        if from == 0 || from > max {
            return Err(format!("from version {from} out of range (1..={max})"));
        }
        if to == 0 || to > max {
            return Err(format!("to version {to} out of range (1..={max})"));
        }
        let old = &self.versions[from as usize - 1].schema;
        let new = &self.versions[to as usize - 1].schema;
        Ok(crate::schema_ops::diff_schemas(old, new))
    }

    pub fn changelog(&self) -> &[SchemaVersion] {
        &self.versions
    }

    pub fn save_to_dir(&self, dir: &Path) -> Result<(), String> {
        let schemas_dir = dir.join("schemas");
        std::fs::create_dir_all(&schemas_dir).map_err(|e| e.to_string())?;
        for v in &self.versions {
            let filename = format!("v{}_{}.json", v.version, v.timestamp);
            let path = schemas_dir.join(filename);
            let content = serde_json::to_string_pretty(&v.schema).map_err(|e| e.to_string())?;
            std::fs::write(&path, content).map_err(|e| e.to_string())?;
        }
        let current_schema = self.current_schema();
        let current_path = schemas_dir.join("current.json");
        let current_content =
            serde_json::to_string_pretty(current_schema).map_err(|e| e.to_string())?;
        std::fs::write(&current_path, current_content).map_err(|e| e.to_string())?;
        let changelog: Vec<serde_json::Value> = self
            .versions
            .iter()
            .map(|v| {
                serde_json::json!({
                    "version": v.version,
                    "timestamp": v.timestamp,
                    "source": format!("{:?}", v.source),
                    "changes": v.changes.iter().map(|c| serde_json::json!({
                        "field": c.field,
                        "change_type": c.change_type,
                        "description": c.description,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let changelog_json = serde_json::json!({"current": self.current, "versions": changelog});
        let changelog_content =
            serde_json::to_string_pretty(&changelog_json).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("schema-changelog.json"), changelog_content)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_from_dir(dir: &Path) -> Result<Self, String> {
        let changelog_path = dir.join("schema-changelog.json");
        let raw = std::fs::read_to_string(&changelog_path).map_err(|e| e.to_string())?;
        let changelog: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let current = changelog
            .get("current")
            .and_then(Value::as_u64)
            .ok_or_else(|| "missing 'current' in changelog".to_string())?
            as u32;
        let entries = changelog
            .get("versions")
            .and_then(Value::as_array)
            .ok_or_else(|| "missing 'versions' in changelog".to_string())?;
        let schemas_dir = dir.join("schemas");
        let mut versions = Vec::with_capacity(entries.len());
        for entry in entries {
            let version = entry
                .get("version")
                .and_then(Value::as_u64)
                .ok_or_else(|| "missing version number".to_string())?
                as u32;
            let timestamp = entry
                .get("timestamp")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing timestamp".to_string())?
                .to_owned();
            let source_str = entry
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("Generated");
            let source = match source_str {
                "AutoEvolution" => SchemaSource::AutoEvolution,
                "Manual" => SchemaSource::Manual,
                _ => SchemaSource::Generated,
            };
            let changes: Vec<VersionChange> = entry
                .get("changes")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            Some(VersionChange {
                                field: c.get("field")?.as_str()?.to_owned(),
                                change_type: c.get("change_type")?.as_str()?.to_owned(),
                                description: c.get("description")?.as_str()?.to_owned(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let filename = format!("v{version}_{timestamp}.json");
            let schema_path = schemas_dir.join(filename);
            let schema_raw = std::fs::read_to_string(&schema_path).map_err(|e| e.to_string())?;
            let schema: Value = serde_json::from_str(&schema_raw).map_err(|e| e.to_string())?;
            versions.push(SchemaVersion {
                version,
                timestamp,
                source,
                schema,
                changes,
            });
        }
        if versions.is_empty() {
            return Err("no versions found".to_string());
        }
        Ok(Self { versions, current })
    }
}

pub fn apply_auto_evolutions(
    store: &mut SchemaVersionStore,
    proposals: &[EvolutionProposal],
) -> Vec<u32> {
    let auto_proposals: Vec<&EvolutionProposal> = proposals
        .iter()
        .filter(|p| p.approval == ApprovalLevel::Auto)
        .collect();
    if auto_proposals.is_empty() {
        return vec![];
    }
    let mut schema = store.current_schema().clone();
    let mut changes = Vec::new();
    for p in &auto_proposals {
        match p.change_type {
            ChangeType::DescriptionEnrich => {
                if let Some(prop) = schema
                    .get_mut("properties")
                    .and_then(|ps| ps.get_mut(&p.field))
                {
                    if prop.get("description").is_none() {
                        prop.as_object_mut().map(|obj| {
                            obj.insert(
                                "description".into(),
                                Value::String(format!("The {} field", p.field)),
                            )
                        });
                    }
                }
            }
            ChangeType::DefaultAdd => {
                if let Some(prop) = schema
                    .get_mut("properties")
                    .and_then(|ps| ps.get_mut(&p.field))
                {
                    if prop.get("default").is_none() {
                        let default_val = match prop.get("type").and_then(Value::as_str) {
                            Some("string") => Value::String(String::new()),
                            Some("integer" | "number") => serde_json::json!(0),
                            Some("boolean") => serde_json::json!(false),
                            _ => Value::Null,
                        };
                        prop.as_object_mut()
                            .map(|obj| obj.insert("default".into(), default_val));
                    }
                }
            }
            _ => continue,
        }
        changes.push(VersionChange {
            field: p.field.clone(),
            change_type: format!("{:?}", p.change_type),
            description: p.description.clone(),
        });
    }
    if changes.is_empty() {
        return vec![];
    }
    let ver = store.push(schema, SchemaSource::AutoEvolution, changes);
    vec![ver]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViolationSeverity {
    Block,
    Warn,
}

#[derive(Debug, Clone)]
pub struct StrictnessViolation {
    pub code: &'static str,
    pub severity: ViolationSeverity,
    pub path: String,
    pub message: String,
    pub fix: String,
}

#[derive(Debug, Clone)]
pub struct StrictnessReport {
    pub passed: bool,
    pub violations: Vec<StrictnessViolation>,
}

pub fn check_strictness(schema: &Value) -> StrictnessReport {
    let mut violations = Vec::new();
    check_s007(schema, &mut violations);
    check_strictness_recursive(schema, "$", &mut violations);
    let passed = violations
        .iter()
        .all(|v| v.severity != ViolationSeverity::Block);
    StrictnessReport { passed, violations }
}

#[expect(
    clippy::too_many_lines,
    reason = "8 anti-pattern checks, splitting reduces readability"
)]
fn check_strictness_recursive(
    schema: &Value,
    path: &str,
    violations: &mut Vec<StrictnessViolation>,
) {
    let Some(obj) = schema.as_object() else {
        return;
    };
    if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        let prop_count = props.len();
        if prop_count >= 3 {
            let required = obj
                .get("required")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if required == 0 {
                violations.push(StrictnessViolation {
                    code: "AVE-S001",
                    severity: ViolationSeverity::Block,
                    path: path.to_owned(),
                    message: format!("Object with {prop_count} properties but 0 required fields"),
                    fix: "Add required array. Without it, empty {{}} passes validation.".into(),
                });
            }
        }
        if prop_count >= 3 {
            let string_props: Vec<&String> = props
                .iter()
                .filter(|(_, v)| v.get("type").and_then(Value::as_str) == Some("string"))
                .map(|(k, _)| k)
                .collect();
            let unconstrained_strings: Vec<&&String> = string_props
                .iter()
                .filter(|k| {
                    let p = &props[**k];
                    p.get("format").is_none()
                        && p.get("enum").is_none()
                        && p.get("pattern").is_none()
                })
                .collect();
            if unconstrained_strings.len() >= 3 && unconstrained_strings.len() == string_props.len()
            {
                violations.push(StrictnessViolation {
                    code: "AVE-S002",
                    severity: ViolationSeverity::Block,
                    path: path.to_owned(),
                    message: format!(
                        "All {} string properties lack format/enum/pattern",
                        string_props.len()
                    ),
                    fix: "Add format, enum, or pattern to string fields. 'asdf' passes as email."
                        .into(),
                });
            }
            let all_string = props
                .values()
                .all(|v| v.get("type").and_then(Value::as_str) == Some("string"));
            if all_string {
                violations.push(StrictnessViolation {
                    code: "AVE-S005",
                    severity: ViolationSeverity::Block,
                    path: path.to_owned(),
                    message: format!("Every property ({prop_count}) is type \"string\""),
                    fix: "Not all fields are strings. Use integer, number, boolean, array, object."
                        .into(),
                });
            }
        }
        for (name, prop) in props {
            let child_path = format!("{path}.{name}");
            let typ = prop.get("type").and_then(Value::as_str);
            if (typ == Some("number") || typ == Some("integer"))
                && prop.get("minimum").is_none()
                && prop.get("maximum").is_none()
            {
                violations.push(StrictnessViolation {
                    code: "AVE-S003",
                    severity: ViolationSeverity::Warn,
                    path: child_path.clone(),
                    message: format!("Number field '{name}' without min or max"),
                    fix: "Add minimum/maximum. -999999 or 999999 would pass.".into(),
                });
            }
            if typ == Some("array") && prop.get("items").is_none() {
                violations.push(StrictnessViolation {
                    code: "AVE-S004",
                    severity: ViolationSeverity::Warn,
                    path: child_path.clone(),
                    message: format!("Array field '{name}' without items schema"),
                    fix: "Add items schema. [null, 123, 'garbage'] would pass.".into(),
                });
            }
            if let Some(enum_arr) = prop.get("enum").and_then(Value::as_array) {
                if enum_arr.len() >= 50 {
                    violations.push(StrictnessViolation {
                        code: "AVE-S006",
                        severity: ViolationSeverity::Warn,
                        path: child_path.clone(),
                        message: format!("Enum has {} values", enum_arr.len()),
                        fix: format!(
                            "Enum has {} values. Consider if this is effectively unconstrained.",
                            enum_arr.len()
                        ),
                    });
                }
            }
            if typ == Some("object") {
                if prop.get("properties").is_none() {
                    violations.push(StrictnessViolation {
                        code: "AVE-S008",
                        severity: ViolationSeverity::Warn,
                        path: child_path.clone(),
                        message: format!(
                            "Nested object at {child_path} has no property definitions"
                        ),
                        fix: format!("Nested object at {child_path} has no property definitions."),
                    });
                }
                check_strictness_recursive(prop, &child_path, violations);
            }
            if let Some(items) = prop.get("items") {
                if items.get("type").and_then(Value::as_str) == Some("object") {
                    let items_path = format!("{child_path}.items");
                    check_strictness_recursive(items, &items_path, violations);
                }
            }
        }
    }
}

fn check_s007(schema: &Value, violations: &mut Vec<StrictnessViolation>) {
    let has_type = schema.get("type").is_some();
    let has_any_of = schema.get("anyOf").is_some();
    let has_one_of = schema.get("oneOf").is_some();
    let has_ref = schema.get("$ref").is_some();
    if !has_type && !has_any_of && !has_one_of && !has_ref {
        violations.push(StrictnessViolation {
            code: "AVE-S007",
            severity: ViolationSeverity::Block,
            path: "$".into(),
            message: "Schema has no root type".into(),
            fix: "Schema has no root type. Any JSON value passes.".into(),
        });
    }
}

#[derive(Debug, Clone)]
pub struct LooseningChange {
    pub code: &'static str,
    pub severity: ViolationSeverity,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct LooseningReport {
    pub allowed: bool,
    pub changes: Vec<LooseningChange>,
}

#[expect(
    clippy::too_many_lines,
    reason = "7 loosening checks, splitting reduces readability"
)]
pub fn check_loosening(old: &Value, new: &Value) -> LooseningReport {
    let mut changes = Vec::new();
    let diff = crate::schema_ops::diff_schemas(old, new);
    let old_required = old
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let new_required = new
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for r in &old_required {
        if !new_required.contains(r) {
            let name = r.as_str().unwrap_or("?");
            changes.push(LooseningChange {
                code: "AVE-L001",
                severity: ViolationSeverity::Block,
                description: format!("Required field '{name}' removed"),
            });
        }
    }
    for ch in &diff.changed {
        if ch.field == "format" && !ch.old.is_empty() && ch.new.is_empty() {
            changes.push(LooseningChange {
                code: "AVE-L003",
                severity: ViolationSeverity::Warn,
                description: format!(
                    "Format constraint removed from {}: {} -> none",
                    ch.path, ch.old
                ),
            });
        }
        if ch.field == "minimum" || ch.field == "maximum" {
            let old_val = ch.old.parse::<f64>();
            let new_val = ch.new.parse::<f64>();
            if let (Ok(ov), Ok(nv)) = (old_val, new_val) {
                let loosened = if ch.field == "minimum" {
                    nv < ov
                } else {
                    nv > ov
                };
                if loosened {
                    changes.push(LooseningChange {
                        code: "AVE-L004",
                        severity: ViolationSeverity::Warn,
                        description: format!(
                            "{} {} changed: {} -> {}",
                            ch.path, ch.field, ch.old, ch.new
                        ),
                    });
                }
            }
        }
        if ch.field == "type" {
            let specific = ["integer", "number", "boolean", "array", "object"];
            let old_is_specific = specific.iter().any(|s| ch.old.contains(s));
            let new_is_string = ch.new.contains("string");
            if old_is_specific && new_is_string {
                changes.push(LooseningChange {
                    code: "AVE-L005",
                    severity: ViolationSeverity::Block,
                    description: format!(
                        "Type changed from {} to {} at {}",
                        ch.old, ch.new, ch.path
                    ),
                });
            }
        }
    }
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
    for name in &diff.added {
        if let Some(prop) = new_props.get(name) {
            let has_constraint = prop.get("format").is_some()
                || prop.get("enum").is_some()
                || prop.get("pattern").is_some()
                || prop.get("minimum").is_some()
                || prop.get("maximum").is_some()
                || prop.get("minLength").is_some()
                || prop.get("maxLength").is_some();
            if !has_constraint {
                changes.push(LooseningChange {
                    code: "AVE-L006",
                    severity: ViolationSeverity::Warn,
                    description: format!("New field '{name}' without any constraints"),
                });
            }
        }
    }
    if !old_required.is_empty() {
        let old_count = old_required.len();
        let new_count = new_required.len();
        #[expect(
            clippy::cast_precision_loss,
            reason = "required field counts won't exceed f64 mantissa"
        )]
        let decreased_by_half = new_count as f64 <= old_count as f64 * 0.5;
        if decreased_by_half && new_count < old_count {
            changes.push(LooseningChange {
                code: "AVE-L007",
                severity: ViolationSeverity::Block,
                description: format!(
                    "Required count decreased by 50%+: {old_count} -> {new_count}"
                ),
            });
        }
    }
    let _ = (old_props, new_props);
    let allowed = changes
        .iter()
        .all(|c| c.severity != ViolationSeverity::Block);
    LooseningReport { allowed, changes }
}

fn current_timestamp() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
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

    #[test]
    fn version_store_push_and_current() {
        let s1 = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        let mut store = SchemaVersionStore::new(s1.clone());
        assert_eq!(store.current_version(), 1);
        assert_eq!(store.current_schema(), &s1);
        let s2 = json!({"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "integer"}}});
        let v2 = store.push(
            s2.clone(),
            SchemaSource::Manual,
            vec![VersionChange {
                field: "b".into(),
                change_type: "added".into(),
                description: "added field b".into(),
            }],
        );
        assert_eq!(v2, 2);
        let s3 = json!({"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "integer"}, "c": {"type": "boolean"}}});
        let v3 = store.push(s3.clone(), SchemaSource::AutoEvolution, vec![]);
        assert_eq!(v3, 3);
        assert_eq!(store.current_version(), 3);
        assert_eq!(store.current_schema(), &s3);
    }

    #[test]
    fn version_store_rollback() {
        let s1 = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        let mut store = SchemaVersionStore::new(s1.clone());
        let s2 = json!({"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "integer"}}});
        store.push(s2, SchemaSource::Manual, vec![]);
        let s3 = json!({"type": "object", "properties": {"c": {"type": "boolean"}}});
        store.push(s3, SchemaSource::Manual, vec![]);
        assert_eq!(store.current_version(), 3);
        store.rollback(1).expect("rollback to v1");
        assert_eq!(store.current_version(), 1);
        assert_eq!(store.current_schema(), &s1);
        assert!(store.rollback(0).is_err());
        assert!(store.rollback(99).is_err());
    }

    #[test]
    fn version_store_diff() {
        let s1 = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        let mut store = SchemaVersionStore::new(s1);
        let s2 = json!({"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "integer"}}});
        store.push(s2, SchemaSource::Manual, vec![]);
        let s3 = json!({"type": "object", "properties": {"b": {"type": "integer"}, "c": {"type": "boolean"}}});
        store.push(s3, SchemaSource::Manual, vec![]);
        let diff = store.diff(1, 3).expect("diff v1 to v3");
        assert!(diff.added.contains(&"c".to_owned()));
        assert!(diff.removed.contains(&"a".to_owned()));
        assert!(store.diff(0, 1).is_err());
        assert!(store.diff(1, 99).is_err());
    }

    #[test]
    fn version_store_save_load_roundtrip() {
        let s1 = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        let mut store = SchemaVersionStore::new(s1.clone());
        let s2 = json!({"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "integer"}}});
        store.push(
            s2.clone(),
            SchemaSource::AutoEvolution,
            vec![VersionChange {
                field: "b".into(),
                change_type: "added".into(),
                description: "added field b".into(),
            }],
        );
        let dir = std::env::temp_dir().join(format!("rupia_test_{}", std::process::id()));
        store.save_to_dir(&dir).expect("save");
        let loaded = SchemaVersionStore::load_from_dir(&dir).expect("load");
        assert_eq!(loaded.current_version(), store.current_version());
        assert_eq!(loaded.current_schema(), store.current_schema());
        assert_eq!(loaded.changelog().len(), 2);
        assert_eq!(loaded.changelog()[0].schema, s1);
        assert_eq!(loaded.changelog()[1].schema, s2);
        assert_eq!(loaded.changelog()[1].source, SchemaSource::AutoEvolution);
        assert_eq!(loaded.changelog()[1].changes.len(), 1);
        assert_eq!(loaded.changelog()[1].changes[0].field, "b");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_auto_evolutions_pushes_version() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        });
        let mut store = SchemaVersionStore::new(schema);
        let proposals = vec![
            EvolutionProposal {
                field: "name".into(),
                change_type: ChangeType::DescriptionEnrich,
                approval: ApprovalLevel::Auto,
                description: "add description".into(),
            },
            EvolutionProposal {
                field: "name".into(),
                change_type: ChangeType::TypeChange,
                approval: ApprovalLevel::Sync,
                description: "type change".into(),
            },
        ];
        let new_vers = apply_auto_evolutions(&mut store, &proposals);
        assert_eq!(new_vers.len(), 1);
        assert_eq!(store.current_version(), 2);
        let desc = store.current_schema()["properties"]["name"]["description"]
            .as_str()
            .expect("description should exist");
        assert!(desc.contains("name"));
    }

    #[test]
    fn jsonlogic_simple_equality() {
        let data = json!({"status": "active"});
        let rules = vec![JsonLogicRule {
            description: "status must be active".into(),
            logic: json!({"==": [{"var": "status"}, "active"]}),
        }];
        let violations = validate_rules(&data, &rules);
        assert!(violations.is_empty());
    }

    #[test]
    fn jsonlogic_simple_equality_fail() {
        let data = json!({"status": "inactive"});
        let rules = vec![JsonLogicRule {
            description: "status must be active".into(),
            logic: json!({"==": [{"var": "status"}, "active"]}),
        }];
        let violations = validate_rules(&data, &rules);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].description, "status must be active");
    }

    #[test]
    fn jsonlogic_arithmetic_relation() {
        let data = json!({"subtotal": 100, "tax": 10, "total": 110});
        let rules = vec![JsonLogicRule {
            description: "total must equal subtotal + tax".into(),
            logic: json!({"==": [{"var": "total"}, {"+": [{"var": "subtotal"}, {"var": "tax"}]}]}),
        }];
        assert!(validate_rules(&data, &rules).is_empty());
    }

    #[test]
    fn jsonlogic_arithmetic_fail() {
        let data = json!({"subtotal": 100, "tax": 10, "total": 999});
        let rules = vec![JsonLogicRule {
            description: "total must equal subtotal + tax".into(),
            logic: json!({"==": [{"var": "total"}, {"+": [{"var": "subtotal"}, {"var": "tax"}]}]}),
        }];
        let violations = validate_rules(&data, &rules);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn jsonlogic_conditional_required() {
        let data = json!({"status": "shipped", "tracking_number": "TRK123"});
        let rules = vec![JsonLogicRule {
            description: "shipped requires tracking_number".into(),
            logic: json!({"if": [{"==": [{"var": "status"}, "shipped"]}, {"!!": {"var": "tracking_number"}}, true]}),
        }];
        assert!(validate_rules(&data, &rules).is_empty());
    }

    #[test]
    fn jsonlogic_conditional_required_fail() {
        let data = json!({"status": "shipped"});
        let rules = vec![JsonLogicRule {
            description: "shipped requires tracking_number".into(),
            logic: json!({"if": [{"==": [{"var": "status"}, "shipped"]}, {"!!": {"var": "tracking_number"}}, true]}),
        }];
        let violations = validate_rules(&data, &rules);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn jsonlogic_conditional_not_triggered() {
        let data = json!({"status": "pending"});
        let rules = vec![JsonLogicRule {
            description: "shipped requires tracking_number".into(),
            logic: json!({"if": [{"==": [{"var": "status"}, "shipped"]}, {"!!": {"var": "tracking_number"}}, true]}),
        }];
        assert!(validate_rules(&data, &rules).is_empty());
    }

    #[test]
    fn jsonlogic_empty_rules() {
        let data = json!({"a": 1});
        assert!(validate_rules(&data, &[]).is_empty());
    }

    #[test]
    fn jsonlogic_oversized_data() {
        let big = "x".repeat(RULE_MAX_JSON_SIZE + 1);
        let data = json!({"payload": big});
        let rules = vec![JsonLogicRule {
            description: "any".into(),
            logic: json!({"==": [1, 1]}),
        }];
        let violations = validate_rules(&data, &rules);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].description.contains("exceeds max size"));
    }

    #[test]
    fn jsonlogic_invalid_rule() {
        let data = json!({"a": 1});
        let rules = vec![JsonLogicRule {
            description: "bad rule".into(),
            logic: json!({"nonexistent_op": [1, 2]}),
        }];
        let violations = validate_rules(&data, &rules);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn jsonlogic_multiple_rules_partial_fail() {
        let data = json!({"age": 25, "status": "inactive"});
        let rules = vec![
            JsonLogicRule {
                description: "age >= 18".into(),
                logic: json!({">=": [{"var": "age"}, 18]}),
            },
            JsonLogicRule {
                description: "status must be active".into(),
                logic: json!({"==": [{"var": "status"}, "active"]}),
            },
        ];
        let violations = validate_rules(&data, &rules);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].description, "status must be active");
    }

    #[test]
    fn parse_package_with_rules() {
        let raw = r#"{
            "schema": {"type": "object", "properties": {"total": {"type": "number"}}},
            "relations": [],
            "rules": [{"description": "total > 0", "logic": {">": [{"var": "total"}, 0]}}],
            "counterexamples": []
        }"#;
        let pkg = parse_schema_package(raw).unwrap();
        assert_eq!(pkg.rules.len(), 1);
        assert_eq!(pkg.rules[0].description, "total > 0");
    }
}
