use serde_json::Value;

enum Segment {
    Key(String),
    Index(usize),
    Iterate,
}

fn parse_path(path: &str) -> Result<Vec<Segment>, String> {
    let mut segments = Vec::new();
    let mut chars = path.chars().peekable();
    while chars.peek().is_some() {
        if chars.peek() == Some(&'.') {
            chars.next();
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            if chars.peek() == Some(&']') {
                chars.next();
                segments.push(Segment::Iterate);
            } else {
                let mut num = String::new();
                while let Some(&c) = chars.peek() {
                    if c == ']' {
                        break;
                    }
                    num.push(c);
                    chars.next();
                }
                if chars.next() != Some(']') {
                    return Err(format!("unclosed bracket in: {path}"));
                }
                let idx: usize = num.parse().map_err(|_| format!("invalid index: {num}"))?;
                segments.push(Segment::Index(idx));
            }
        } else {
            let mut key = String::new();
            while let Some(&c) = chars.peek() {
                if c == '.' || c == '[' {
                    break;
                }
                key.push(c);
                chars.next();
            }
            if key.is_empty() {
                return Err(format!("empty key in: {path}"));
            }
            segments.push(Segment::Key(key));
        }
    }
    Ok(segments)
}

fn resolve<'a>(value: &'a Value, segments: &[Segment]) -> Result<Vec<&'a Value>, String> {
    if segments.is_empty() {
        return Ok(vec![value]);
    }
    match &segments[0] {
        Segment::Key(key) => {
            let child = value
                .get(key.as_str())
                .ok_or_else(|| format!("field not found: {key}"))?;
            resolve(child, &segments[1..])
        }
        Segment::Index(idx) => {
            let arr = value.as_array().ok_or("expected array")?;
            let child = arr
                .get(*idx)
                .ok_or_else(|| format!("index out of bounds: {idx}"))?;
            resolve(child, &segments[1..])
        }
        Segment::Iterate => {
            let arr = value.as_array().ok_or("expected array for []")?;
            let mut out = Vec::new();
            for item in arr {
                out.extend(resolve(item, &segments[1..])?);
            }
            Ok(out)
        }
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

pub fn extract(value: &Value, path: &str) -> Result<String, String> {
    let segments = parse_path(path)?;
    let results = resolve(value, &segments)?;
    if results.len() == 1 {
        if let Some(arr) = results[0].as_array() {
            Ok(arr.iter().map(format_value).collect::<Vec<_>>().join("\n"))
        } else {
            Ok(format_value(results[0]))
        }
    } else {
        Ok(results
            .iter()
            .map(|v| format_value(v))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_key() {
        let v = json!({"type": "feat", "scope": "skills"});
        assert_eq!(extract(&v, "type").unwrap(), "feat");
        assert_eq!(extract(&v, "scope").unwrap(), "skills");
    }

    #[test]
    fn nested_key() {
        let v = json!({"a": {"b": {"c": 42}}});
        assert_eq!(extract(&v, "a.b.c").unwrap(), "42");
    }

    #[test]
    fn array_index() {
        let v = json!({"authors": [{"role": "implement"}, {"role": "review"}]});
        assert_eq!(extract(&v, "authors[0].role").unwrap(), "implement");
        assert_eq!(extract(&v, "authors[1].role").unwrap(), "review");
    }

    #[test]
    fn array_iterate() {
        let v = json!({"authors": [{"role": "implement"}, {"role": "review"}]});
        assert_eq!(extract(&v, "authors[].role").unwrap(), "implement\nreview");
    }

    #[test]
    fn string_array_flat() {
        let v = json!({"files": ["src/a.rs", "src/b.rs"]});
        assert_eq!(extract(&v, "files").unwrap(), "src/a.rs\nsrc/b.rs");
    }

    #[test]
    fn number_and_bool() {
        let v = json!({"count": 3, "ok": true});
        assert_eq!(extract(&v, "count").unwrap(), "3");
        assert_eq!(extract(&v, "ok").unwrap(), "true");
    }

    #[test]
    fn object_as_json() {
        let v = json!({"meta": {"a": 1}});
        let out = extract(&v, "meta").unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed, json!({"a": 1}));
    }

    #[test]
    fn field_not_found() {
        let v = json!({"a": 1});
        assert!(extract(&v, "b").is_err());
    }

    #[test]
    fn index_out_of_bounds() {
        let v = json!({"a": [1]});
        assert!(extract(&v, "a[5]").is_err());
    }

    #[test]
    fn null_value() {
        let v = json!({"x": null});
        assert_eq!(extract(&v, "x").unwrap(), "null");
    }
}
