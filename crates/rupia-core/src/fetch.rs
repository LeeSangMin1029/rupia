use crate::schema_ops::{CrossRefResult, Divergence, UniversalConstraint, UniversalEnum};
use serde_json::Value;
use std::path::PathBuf;

const APIS_GURU_LIST: &str = "https://api.apis.guru/v2/list.json";
const CACHE_DIR: &str = ".cache/rupia";
const LIST_CACHE_HOURS: u64 = 24;
const SPEC_CACHE_DAYS: u64 = 7;

#[derive(Debug, Clone)]
pub struct ApiInfo {
    pub name: String,
    pub title: String,
    pub openapi_url: String,
}

#[derive(Debug)]
pub struct CrossRefReport {
    pub domain: String,
    pub apis_analyzed: Vec<String>,
    pub schemas_found: usize,
    pub universal_enums: Vec<UniversalEnum>,
    pub universal_constraints: Vec<UniversalConstraint>,
    pub divergences: Vec<Divergence>,
}

pub(crate) fn cache_dir() -> PathBuf {
    dirs_home().join(CACHE_DIR)
}

fn dirs_home() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home);
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return PathBuf::from(profile);
    }
    PathBuf::from(".")
}

fn is_cache_fresh(path: &std::path::Path, max_age_secs: u64) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let Ok(elapsed) = modified.elapsed() else {
        return false;
    };
    elapsed.as_secs() < max_age_secs
}

pub fn fetch_api_list() -> Result<Value, String> {
    let dir = cache_dir();
    let cache_path = dir.join("list.json");
    if is_cache_fresh(&cache_path, LIST_CACHE_HOURS * 3600) {
        let content = std::fs::read_to_string(&cache_path)
            .map_err(|e| format!("failed to read cache: {e}"))?;
        return serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse cached list: {e}"));
    }
    let body = reqwest::blocking::get(APIS_GURU_LIST)
        .map_err(|e| format!("[RUPIA-NET001] network error fetching API list: {e}"))?
        .text()
        .map_err(|e| format!("[RUPIA-NET002] failed to read response body: {e}"))?;
    let val: Value = serde_json::from_str(&body)
        .map_err(|e| format!("[RUPIA-NET003] API list is not valid JSON: {e}"))?;
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(&cache_path, &body).ok();
    Ok(val)
}

pub fn search_apis(list: &Value, keyword: &str, max_results: usize) -> Vec<ApiInfo> {
    let Some(obj) = list.as_object() else {
        return vec![];
    };
    let kw = keyword.to_lowercase();
    let mut results: Vec<(usize, ApiInfo)> = Vec::new();
    for (key, val) in obj {
        let key_lower = key.to_lowercase();
        let versions = val.get("versions").and_then(Value::as_object);
        let preferred = val.get("preferred").and_then(Value::as_str).unwrap_or("");
        let api_val = versions.and_then(|v| v.get(preferred).or_else(|| v.values().next_back()));
        let title = api_val
            .and_then(|v| v.get("info"))
            .and_then(|i| i.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let description = api_val
            .and_then(|v| v.get("info"))
            .and_then(|i| i.get("description"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let openapi_url = api_val
            .and_then(|v| v.get("swaggerUrl").or_else(|| v.get("openapiVer")))
            .and_then(Value::as_str)
            .unwrap_or("");
        let link = if openapi_url.is_empty() {
            api_val
                .and_then(|v| v.get("link"))
                .and_then(Value::as_str)
                .unwrap_or("")
        } else {
            openapi_url
        };
        let title_lower = title.to_lowercase();
        let desc_lower = description.to_lowercase();
        let score = if key_lower.contains(&kw) {
            3
        } else if title_lower.contains(&kw) {
            2
        } else if desc_lower.contains(&kw) {
            1
        } else {
            continue;
        };
        results.push((
            score,
            ApiInfo {
                name: key.clone(),
                title: title.to_string(),
                openapi_url: link.to_string(),
            },
        ));
    }
    results.sort_by(|a, b| b.0.cmp(&a.0));
    results.truncate(max_results);
    results.into_iter().map(|(_, info)| info).collect()
}

pub fn fetch_spec(url: &str, name: &str) -> Result<Value, String> {
    let dir = cache_dir().join("specs");
    let safe_name = name.replace(['/', '\\', ':', ' '], "_");
    let cache_path = dir.join(format!("{safe_name}.json"));
    if is_cache_fresh(&cache_path, SPEC_CACHE_DAYS * 24 * 3600) {
        let content = std::fs::read_to_string(&cache_path)
            .map_err(|e| format!("failed to read cached spec: {e}"))?;
        return serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse cached spec: {e}"));
    }
    let body = reqwest::blocking::get(url)
        .map_err(|e| format!("[RUPIA-NET001] network error fetching spec '{name}': {e}"))?
        .text()
        .map_err(|e| format!("[RUPIA-NET002] failed to read spec body '{name}': {e}"))?;
    let val: Value = serde_json::from_str(&body)
        .map_err(|e| format!("[RUPIA-NET003] spec '{name}' is not valid JSON: {e}"))?;
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(&cache_path, &body).ok();
    Ok(val)
}

pub fn fetch_spec_fresh(url: &str, name: &str) -> Result<Value, String> {
    let body = reqwest::blocking::get(url)
        .map_err(|e| format!("[RUPIA-NET001] network error fetching spec '{name}': {e}"))?
        .text()
        .map_err(|e| format!("[RUPIA-NET002] failed to read spec body '{name}': {e}"))?;
    serde_json::from_str(&body)
        .map_err(|e| format!("[RUPIA-NET003] spec '{name}' is not valid JSON: {e}"))
}

pub fn cross_ref_by_domain(
    domain: &str,
    entity_hint: Option<&str>,
    max_apis: usize,
) -> Result<CrossRefReport, String> {
    let list = fetch_api_list()?;
    let apis = search_apis(&list, domain, max_apis);
    if apis.is_empty() {
        return Err(format!(
            "[RUPIA-FETCH001] no APIs found for domain '{domain}'"
        ));
    }
    let mut specs = Vec::new();
    let mut analyzed = Vec::new();
    for api in &apis {
        if api.openapi_url.is_empty() {
            continue;
        }
        match fetch_spec(&api.openapi_url, &api.name) {
            Ok(spec) => {
                analyzed.push(api.name.clone());
                specs.push(spec);
            }
            Err(e) => {
                eprintln!("[warn] skipping {}: {e}", api.name);
            }
        }
    }
    if specs.is_empty() {
        return Err(format!(
            "[RUPIA-FETCH002] downloaded 0 specs for domain '{domain}'"
        ));
    }
    let schemas = if let Some(hint) = entity_hint {
        crate::registry::extract_entity_schemas(&specs, hint)
    } else {
        specs
            .iter()
            .filter_map(|s| {
                s.pointer("/components/schemas")
                    .or_else(|| s.get("definitions"))
            })
            .cloned()
            .collect()
    };
    let schemas_found = schemas.len();
    let CrossRefResult {
        universal_enums,
        universal_constraints,
        divergences,
    } = crate::schema_ops::cross_reference_schemas(&schemas);
    Ok(CrossRefReport {
        domain: domain.to_string(),
        apis_analyzed: analyzed,
        schemas_found,
        universal_enums,
        universal_constraints,
        divergences,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mock_list() -> Value {
        json!({
            "stripe.com": {
                "preferred": "v3",
                "versions": {
                    "v3": {
                        "info": {
                            "title": "Stripe Payment API",
                            "description": "Online payment processing for internet businesses"
                        },
                        "swaggerUrl": "https://example.com/stripe.json"
                    }
                }
            },
            "paypal.com": {
                "preferred": "v1",
                "versions": {
                    "v1": {
                        "info": {
                            "title": "PayPal REST API",
                            "description": "Payment and checkout APIs"
                        },
                        "swaggerUrl": "https://example.com/paypal.json"
                    }
                }
            },
            "github.com": {
                "preferred": "v3",
                "versions": {
                    "v3": {
                        "info": {
                            "title": "GitHub REST API",
                            "description": "GitHub developer platform"
                        },
                        "swaggerUrl": "https://example.com/github.json"
                    }
                }
            }
        })
    }

    #[test]
    fn search_by_name() {
        let list = mock_list();
        let results = search_apis(&list, "stripe", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "stripe.com");
    }

    #[test]
    fn search_by_title() {
        let list = mock_list();
        let results = search_apis(&list, "github", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "github.com");
    }

    #[test]
    fn search_by_description() {
        let list = mock_list();
        let results = search_apis(&list, "payment", 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_no_match() {
        let list = mock_list();
        let results = search_apis(&list, "nonexistent_xyz", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn search_respects_max_results() {
        let list = mock_list();
        let results = search_apis(&list, "payment", 1);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_ranks_name_match_higher() {
        let list = mock_list();
        let results = search_apis(&list, "paypal", 10);
        assert_eq!(results[0].name, "paypal.com");
    }
}
