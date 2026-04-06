use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskScale {
    Small,
    Medium,
    Large,
}

pub fn task_schema(scale: TaskScale) -> Value {
    let mut schema = json!({
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
            "error": {"type": "string"}
        }
    });
    if scale == TaskScale::Medium || scale == TaskScale::Large {
        let props = schema["properties"].as_object_mut().expect("properties");
        props.insert("title".into(), json!({"type": "string"}));
        props.insert(
            "changes_count".into(),
            json!({"type": "integer", "minimum": 0}),
        );
        props.insert(
            "dependencies".into(),
            json!({"type": "array", "items": {"type": "string"}}),
        );
        props.insert(
            "labels".into(),
            json!({"type": "array", "items": {"type": "string"}}),
        );
        props.insert("test_passed".into(), json!({"type": "boolean"}));
        props.insert(
            "duration_ms".into(),
            json!({"type": "integer", "minimum": 0}),
        );
    }
    if scale == TaskScale::Large {
        let props = schema["properties"].as_object_mut().expect("properties");
        props.insert(
            "progress".into(),
            json!({"type": "string", "pattern": "^[0-9]+/[0-9]+$"}),
        );
        props.insert("nodes".into(), json!({"type": "object"}));
        props.insert("run_count".into(), json!({"type": "integer", "minimum": 1}));
        props.insert(
            "resources".into(),
            json!({
                "type": "object",
                "properties": {
                    "cpu": {"type": "number", "minimum": 0},
                    "memory": {"type": "number", "minimum": 0},
                    "cost_usd": {"type": "number", "minimum": 0}
                }
            }),
        );
        props.insert(
            "artifacts".into(),
            json!({"type": "array", "items": {"type": "string"}}),
        );
    }
    schema
}

pub fn detect_scale(description: &str, estimated_sloc: Option<u32>) -> TaskScale {
    let desc_lower = description.to_lowercase();
    let small_keywords = ["fix", "bug", "typo", "rename"];
    let medium_keywords = ["feature", "add", "implement"];
    if let Some(sloc) = estimated_sloc {
        if sloc < 50 {
            return TaskScale::Small;
        }
        if sloc < 500 {
            return TaskScale::Medium;
        }
        if small_keywords.iter().any(|k| desc_lower.contains(k)) {
            return TaskScale::Small;
        }
        if medium_keywords.iter().any(|k| desc_lower.contains(k)) {
            return TaskScale::Medium;
        }
        return TaskScale::Large;
    }
    if small_keywords.iter().any(|k| desc_lower.contains(k)) {
        return TaskScale::Small;
    }
    if medium_keywords.iter().any(|k| desc_lower.contains(k)) {
        return TaskScale::Medium;
    }
    TaskScale::Large
}

pub fn task_relations(scale: TaskScale) -> Vec<crate::ave::FieldRelation> {
    let mut rels = vec![crate::ave::FieldRelation {
        field_a: "started_at".into(),
        operator: "lte".into(),
        field_b: "completed_at".into(),
    }];
    if scale == TaskScale::Medium || scale == TaskScale::Large {
        rels.push(crate::ave::FieldRelation {
            field_a: "changes_count".into(),
            operator: "gte".into(),
            field_b: "0".into(),
        });
    }
    if scale == TaskScale::Large {
        rels.push(crate::ave::FieldRelation {
            field_a: "run_count".into(),
            operator: "gte".into(),
            field_b: "1".into(),
        });
    }
    rels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_schema_has_required_fields() {
        let s = task_schema(TaskScale::Small);
        let required = s["required"].as_array().expect("required");
        assert!(required.contains(&json!("id")));
        assert!(required.contains(&json!("status")));
        assert!(required.contains(&json!("started_at")));
    }

    #[test]
    fn large_schema_has_nodes() {
        let s = task_schema(TaskScale::Large);
        assert!(s["properties"].get("nodes").is_some());
        assert!(s["properties"].get("progress").is_some());
        assert!(s["properties"].get("run_count").is_some());
    }

    #[test]
    fn detect_fix_typo_is_small() {
        assert_eq!(detect_scale("fix typo", None), TaskScale::Small);
    }

    #[test]
    fn detect_implement_oauth_medium() {
        assert_eq!(
            detect_scale("implement OAuth", Some(300)),
            TaskScale::Medium
        );
    }

    #[test]
    fn detect_build_backend_large() {
        assert_eq!(
            detect_scale("build entire backend", Some(2000)),
            TaskScale::Large
        );
    }

    #[test]
    fn task_relations_large_has_run_count() {
        let rels = task_relations(TaskScale::Large);
        assert!(rels
            .iter()
            .any(|r| r.field_a == "run_count" && r.operator == "gte" && r.field_b == "1"));
    }

    #[test]
    fn small_schema_passes_lint() {
        let s = task_schema(TaskScale::Small);
        let warnings = crate::ave::lint_schema_value(&s);
        let errors: Vec<_> = warnings.iter().filter(|w| w.starts_with("error")).collect();
        assert!(errors.is_empty(), "lint errors: {errors:?}");
    }
}
