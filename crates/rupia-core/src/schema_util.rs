use serde_json::{json, Map, Value};

const MAX_DEPTH: u32 = 16;

fn find_defs(root: &Value) -> Option<&Value> {
    root.get("$defs").or_else(|| root.get("definitions"))
}

pub fn resolve_schema<'a>(schema: &'a Value, root: &'a Value) -> &'a Value {
    resolve_inner(schema, root, 0)
}

fn resolve_inner<'a>(schema: &'a Value, root: &'a Value, depth: u32) -> &'a Value {
    if depth > MAX_DEPTH {
        return schema;
    }
    let Some(ref_str) = schema.get("$ref").and_then(Value::as_str) else {
        return schema;
    };
    let Some(defs) = find_defs(root) else {
        return schema;
    };
    let name = ref_str.rsplit_once('/').map_or(ref_str, |(_, n)| n);
    match defs.get(name) {
        Some(resolved) => resolve_inner(resolved, root, depth + 1),
        None => schema,
    }
}

pub fn merged_schema(schema: &Value, root: &Value) -> Value {
    merged_inner(schema, root, 0)
}

fn merged_inner(schema: &Value, root: &Value, depth: u32) -> Value {
    if depth > MAX_DEPTH {
        return schema.clone();
    }
    let resolved = resolve_inner(schema, root, 0);
    if let Some(all_of) = resolved.get("allOf").and_then(Value::as_array) {
        let mut merged_props = Map::new();
        let mut merged_required = vec![];
        for sub in all_of {
            let sub_merged = merged_inner(sub, root, depth + 1);
            if let Some(props) = sub_merged.get("properties").and_then(Value::as_object) {
                for (k, v) in props {
                    merged_props.insert(k.clone(), v.clone());
                }
            }
            if let Some(req) = sub_merged.get("required").and_then(Value::as_array) {
                for r in req {
                    if let Some(s) = r.as_str() {
                        merged_required.push(Value::String(s.to_owned()));
                    }
                }
            }
        }
        let mut result = json!({"type": "object"});
        if !merged_props.is_empty() {
            result["properties"] = Value::Object(merged_props);
        }
        if !merged_required.is_empty() {
            result["required"] = Value::Array(merged_required);
        }
        return result;
    }
    resolved.clone()
}

pub fn flatten_properties(schema: &Value, root: &Value) -> Vec<(String, Value)> {
    flatten_inner(schema, root, "", 0)
}

fn flatten_inner(schema: &Value, root: &Value, prefix: &str, depth: u32) -> Vec<(String, Value)> {
    if depth > MAX_DEPTH {
        return vec![];
    }
    let resolved = resolve_inner(schema, root, 0);
    if resolved.get("allOf").and_then(Value::as_array).is_some() {
        let m = merged_inner(resolved, root, depth);
        return flatten_inner(&m, root, prefix, depth + 1);
    }
    if let Some(variants) = resolved
        .get("oneOf")
        .or_else(|| resolved.get("anyOf"))
        .and_then(Value::as_array)
    {
        let mut result = vec![];
        for variant in variants {
            result.extend(flatten_inner(variant, root, prefix, depth + 1));
        }
        return result;
    }
    let Some(props) = resolved.get("properties").and_then(Value::as_object) else {
        return vec![];
    };
    let mut result = vec![];
    for (field, prop_schema) in props {
        let path = if prefix.is_empty() {
            field.clone()
        } else {
            format!("{prefix}.{field}")
        };
        let prop_resolved = resolve_inner(prop_schema, root, 0);
        let typ = prop_resolved
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("");
        if typ == "object"
            || prop_resolved.get("properties").is_some()
            || prop_resolved.get("allOf").is_some()
            || prop_resolved.get("oneOf").is_some()
            || prop_resolved.get("anyOf").is_some()
        {
            result.extend(flatten_inner(prop_resolved, root, &path, depth + 1));
        } else {
            result.push((path, prop_resolved.clone()));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_ref() {
        let root = json!({
            "type": "object",
            "properties": {
                "addr": {"$ref": "#/$defs/Addr"}
            },
            "$defs": {
                "Addr": {"type": "object", "properties": {"zip": {"type": "string"}}}
            }
        });
        let prop = &root["properties"]["addr"];
        let resolved = resolve_schema(prop, &root);
        assert_eq!(resolved["type"], "object");
        assert!(resolved.get("properties").is_some());
    }

    #[test]
    fn resolve_circular_ref() {
        let root = json!({
            "$defs": {
                "Node": {
                    "type": "object",
                    "properties": {
                        "child": {"$ref": "#/$defs/Node"}
                    }
                }
            }
        });
        let schema = &json!({"$ref": "#/$defs/Node"});
        let resolved = resolve_schema(schema, &root);
        assert_eq!(resolved["type"], "object");
    }

    #[test]
    fn flatten_nested() {
        let root = json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "age": {"type": "integer", "minimum": 0}
                    }
                }
            }
        });
        let props = flatten_properties(&root, &root);
        let paths: Vec<&str> = props.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"user.age"), "got: {paths:?}");
    }

    #[test]
    fn flatten_allof() {
        let root = json!({
            "allOf": [
                {"type": "object", "properties": {"name": {"type": "string"}}},
                {"type": "object", "properties": {"age": {"type": "integer"}}}
            ]
        });
        let props = flatten_properties(&root, &root);
        let paths: Vec<&str> = props.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"name"), "got: {paths:?}");
        assert!(paths.contains(&"age"), "got: {paths:?}");
    }

    #[test]
    fn flatten_ref() {
        let root = json!({
            "type": "object",
            "properties": {
                "addr": {"$ref": "#/$defs/Addr"}
            },
            "$defs": {
                "Addr": {
                    "type": "object",
                    "properties": {
                        "zip": {"type": "string", "minLength": 5}
                    }
                }
            }
        });
        let props = flatten_properties(&root, &root);
        let paths: Vec<&str> = props.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"addr.zip"), "got: {paths:?}");
    }
}
