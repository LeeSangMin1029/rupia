use std::collections::HashMap;
use std::fmt::Write;

use serde_json::Value;

use crate::types::{ValidationError, ValidationFailure};

pub fn stringify(failure: &ValidationFailure) -> String {
    let mut used = Vec::new();
    let errors_by_path = index_errors(&failure.errors);
    let json_output = stringify_value(
        &failure.data,
        &errors_by_path,
        "$input",
        0,
        false,
        &mut used,
    );
    let unmappable: Vec<&ValidationError> = failure
        .errors
        .iter()
        .filter(|e| !used.contains(&e.path))
        .collect();
    if unmappable.is_empty() {
        format!("```json\n{json_output}\n```")
    } else {
        let mut result =
            format!("```json\n{json_output}\n```\n\n**Unmappable validation errors:**\n```json\n[");
        for (i, e) in unmappable.iter().enumerate() {
            if i > 0 {
                result.push(',');
            }
            let _ = write!(
                result,
                "\n  {{\"path\":\"{}\",\"expected\":\"{}\"}}",
                e.path, e.expected
            );
        }
        result.push_str("\n]\n```");
        result
    }
}

fn index_errors(errors: &[ValidationError]) -> HashMap<&str, Vec<&ValidationError>> {
    let mut map: HashMap<&str, Vec<&ValidationError>> = HashMap::new();
    for e in errors {
        map.entry(e.path.as_str()).or_default().push(e);
    }
    map
}

fn error_comment(
    path: &str,
    errors_by_path: &HashMap<&str, Vec<&ValidationError>>,
    used: &mut Vec<String>,
) -> String {
    let Some(path_errors) = errors_by_path.get(path) else {
        return String::new();
    };
    if path_errors.is_empty() {
        return String::new();
    }
    used.push(path.to_owned());
    let entries: Vec<String> = path_errors
        .iter()
        .map(|e| {
            let mut entry = format!("{{\"path\":\"{}\",\"expected\":\"{}\"", e.path, e.expected);
            if let Some(desc) = &e.description {
                let escaped = desc.replace('"', "\\\"").replace('\n', "\\n");
                let _ = write!(entry, ",\"description\":\"{escaped}\"");
            }
            entry.push('}');
            entry
        })
        .collect();
    format!(" // ❌ [{}]", entries.join(","))
}

fn stringify_value(
    value: &Value,
    errors_by_path: &HashMap<&str, Vec<&ValidationError>>,
    path: &str,
    tab: usize,
    in_array: bool,
    used: &mut Vec<String>,
) -> String {
    let indent = "  ".repeat(tab);
    let err = error_comment(path, errors_by_path, used);
    match value {
        Value::Array(arr) => {
            if arr.is_empty() {
                return format!("{indent}[]{err}");
            }
            let mut lines = vec![format!("{indent}[{err}")];
            for (i, item) in arr.iter().enumerate() {
                let item_path = format!("{path}[{i}]");
                let mut item_str =
                    stringify_value(item, errors_by_path, &item_path, tab + 1, true, used);
                if i < arr.len() - 1 {
                    item_str = insert_comma_before_comment(&item_str);
                }
                lines.push(item_str);
            }
            lines.push(format!("{indent}]"));
            lines.join("\n")
        }
        Value::Object(obj) => {
            let missing = find_missing_properties(path, value, errors_by_path);
            if obj.is_empty() && missing.is_empty() {
                return format!("{indent}{{}}{err}");
            }
            let mut lines = vec![format!("{indent}{{{err}")];
            let keys: Vec<&String> = obj.keys().collect();
            let all_keys: Vec<String> = keys.iter().map(|k| (*k).clone()).chain(missing).collect();
            for (i, key) in all_keys.iter().enumerate() {
                let prop_path = format!("{path}.{key}");
                let inner_indent = "  ".repeat(tab + 1);
                let val = obj.get(key).unwrap_or(&Value::Null);
                let is_last = i == all_keys.len() - 1;
                match val {
                    Value::Object(_) | Value::Array(_) => {
                        let val_str =
                            stringify_value(val, errors_by_path, &prop_path, tab + 1, false, used);
                        let val_trimmed = val_str.trim_start();
                        let mut combined = format!("{inner_indent}\"{key}\": {val_trimmed}");
                        if !is_last {
                            combined = insert_comma_before_comment(&combined);
                        }
                        lines.push(combined);
                    }
                    _ => {
                        let prop_err = error_comment(&prop_path, errors_by_path, used);
                        let val_str = if val.is_null() && !obj.contains_key(key) {
                            "undefined".to_owned()
                        } else {
                            format_primitive(val)
                        };
                        let comma = if is_last { "" } else { "," };
                        lines.push(format!(
                            "{inner_indent}\"{key}\": {val_str}{comma}{prop_err}"
                        ));
                    }
                }
            }
            lines.push(format!("{indent}}}"));
            lines.join("\n")
        }
        _ => {
            if in_array && value.is_null() {
                return format!("{indent}undefined{err}");
            }
            format!("{indent}{}{err}", format_primitive(value))
        }
    }
}

fn format_primitive(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        _ => value.to_string(),
    }
}

fn insert_comma_before_comment(s: &str) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    let mut result: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    if let Some(last) = result.last_mut() {
        if let Some(idx) = last.rfind(" // ❌") {
            let (before, after) = last.split_at(idx);
            *last = format!("{before},{after}");
        } else {
            last.push(',');
        }
    }
    result.join("\n")
}

fn find_missing_properties(
    path: &str,
    value: &Value,
    errors_by_path: &HashMap<&str, Vec<&ValidationError>>,
) -> Vec<String> {
    let Some(obj) = value.as_object() else {
        return Vec::new();
    };
    let prefix_dot = format!("{path}.");
    let mut missing = Vec::new();
    for error_path in errors_by_path.keys() {
        if let Some(suffix) = error_path.strip_prefix(&prefix_dot) {
            if !suffix.contains('.') && !suffix.contains('[') && !obj.contains_key(suffix) {
                missing.push(suffix.to_owned());
            }
        }
    }
    missing.sort();
    missing.dedup();
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ValidationFailure;
    use serde_json::json;

    #[test]
    fn basic_feedback() {
        let failure = ValidationFailure {
            data: json!({"name": "test", "age": "twenty"}),
            errors: vec![ValidationError {
                path: "$input.age".into(),
                expected: "number".into(),
                value: json!("twenty"),
                description: None,
            }],
        };
        let result = stringify(&failure);
        assert!(result.contains("// ❌"));
        assert!(result.contains("$input.age"));
        assert!(result.contains("number"));
        assert!(result.contains("```json"));
    }

    #[test]
    fn multiple_errors() {
        let failure = ValidationFailure {
            data: json!({"email": "bad", "age": -5}),
            errors: vec![
                ValidationError {
                    path: "$input.email".into(),
                    expected: "string & Format<\"email\">".into(),
                    value: json!("bad"),
                    description: None,
                },
                ValidationError {
                    path: "$input.age".into(),
                    expected: "number & Minimum<0>".into(),
                    value: json!(-5),
                    description: None,
                },
            ],
        };
        let result = stringify(&failure);
        assert!(result.contains("email"));
        assert!(result.contains("age"));
        assert_eq!(result.matches("// ❌").count(), 2);
    }

    #[test]
    fn missing_property_feedback() {
        let failure = ValidationFailure {
            data: json!({}),
            errors: vec![ValidationError {
                path: "$input.name".into(),
                expected: "string".into(),
                value: json!(null),
                description: Some("required property".into()),
            }],
        };
        let result = stringify(&failure);
        assert!(result.contains("\"name\""));
        assert!(result.contains("undefined"));
    }

    #[test]
    fn nested_object_feedback() {
        let failure = ValidationFailure {
            data: json!({"user": {"email": "bad"}}),
            errors: vec![ValidationError {
                path: "$input.user.email".into(),
                expected: "string & Format<\"email\">".into(),
                value: json!("bad"),
                description: None,
            }],
        };
        let result = stringify(&failure);
        assert!(result.contains("// ❌"));
        assert!(result.contains("$input.user.email"));
    }
}
