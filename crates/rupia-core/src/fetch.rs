use crate::schema_ops::{CrossRefResult, Divergence, UniversalConstraint, UniversalEnum};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

const APIS_GURU_LIST: &str = "https://api.apis.guru/v2/list.json";
const CACHE_DIR: &str = ".cache/rupia";
const LIST_CACHE_HOURS: u64 = 24;
const SPEC_CACHE_DAYS: u64 = 7;
const MAX_RESPONSE_BYTES: usize = 50 * 1024 * 1024;
const REQUEST_TIMEOUT_SECS: u64 = 30;
const RETRY_DELAY_MS: u64 = 1000;
const RATE_LIMIT_MS: u64 = 200;

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

fn atomic_write(path: &std::path::Path, content: &str) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, content).is_ok() {
        std::fs::rename(&tmp, path).ok();
    }
}

fn validate_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(format!("[RUPIA-SEC001] invalid URL scheme: {url}"));
    }
    let host = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim_start_matches('[')
        .trim_end_matches(']');
    if host.is_empty() {
        return Err("[RUPIA-SEC001] empty host".to_string());
    }
    if host.contains(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != ':') {
        return Err(format!("[RUPIA-SEC005] suspicious host characters: {host}"));
    }
    if host.chars().all(|c| c.is_ascii_digit() || c == '.') {
        if let Some(ip) = parse_ipv4(host) {
            if is_private_ipv4(ip) {
                return Err(format!(
                    "[RUPIA-SEC003] private/reserved IP blocked: {host}"
                ));
            }
        } else {
            return Err(format!("[RUPIA-SEC005] invalid IP format: {host}"));
        }
    }
    let blocked_hosts = ["localhost", "metadata.google.internal", "metadata.internal"];
    let host_lower = host.to_lowercase();
    if blocked_hosts
        .iter()
        .any(|b| host_lower == *b || host_lower.ends_with(&format!(".{b}")))
    {
        return Err(format!("[RUPIA-SEC002] blocked host: {host}"));
    }
    if host_lower.starts_with("0x") || host_lower.starts_with("0o") {
        return Err(format!("[RUPIA-SEC005] encoded IP format blocked: {host}"));
    }
    if host.contains("::") || host_lower.contains("ffff") {
        return Err(format!(
            "[RUPIA-SEC005] IPv6 address blocked in URL: {host}"
        ));
    }
    Ok(())
}

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        if part.len() > 1 && part.starts_with('0') {
            return None;
        }
        octets[i] = part.parse().ok()?;
    }
    Some(octets)
}

fn is_private_ipv4(ip: [u8; 4]) -> bool {
    matches!(
        ip,
        [0 | 10 | 127 | 224..=255, ..] | [169, 254, ..] | [172, 16..=31, ..] | [192, 168, ..]
    )
}

fn build_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("[RUPIA-NET000] failed to build HTTP client: {e}"))
}

fn safe_get(
    client: &reqwest::blocking::Client,
    url: &str,
    context: &str,
) -> Result<String, String> {
    validate_url(url)?;
    let response = client
        .get(url)
        .send()
        .map_err(|e| format!("[RUPIA-NET001] network error {context}: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("[RUPIA-NET004] HTTP {status} for {context}"));
    }
    let content_length =
        usize::try_from(response.content_length().unwrap_or(0)).unwrap_or(usize::MAX);
    if content_length > MAX_RESPONSE_BYTES {
        return Err(format!(
            "[RUPIA-SEC004] response too large ({content_length} bytes) for {context}"
        ));
    }
    let bytes = response
        .bytes()
        .map_err(|e| format!("[RUPIA-NET002] failed to read body {context}: {e}"))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "[RUPIA-SEC004] response too large ({} bytes) for {context}",
            bytes.len()
        ));
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|e| format!("[RUPIA-NET005] non-UTF8 response {context}: {e}"))
}

fn safe_get_with_retry(
    client: &reqwest::blocking::Client,
    url: &str,
    context: &str,
) -> Result<String, String> {
    match safe_get(client, url, context) {
        Ok(body) => Ok(body),
        Err(first_err) => {
            std::thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
            safe_get(client, url, context).map_err(|_| first_err)
        }
    }
}

fn rate_limit() {
    std::thread::sleep(Duration::from_millis(RATE_LIMIT_MS));
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
    let client = build_client()?;
    let body = safe_get_with_retry(&client, APIS_GURU_LIST, "API list")?;
    let val: Value = serde_json::from_str(&body)
        .map_err(|e| format!("[RUPIA-NET003] API list is not valid JSON: {e}"))?;
    std::fs::create_dir_all(&dir).ok();
    atomic_write(&cache_path, &body);
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
    let safe_name = sanitize_path_component(name);
    let cache_path = dir.join(format!("{safe_name}.json"));
    if is_cache_fresh(&cache_path, SPEC_CACHE_DAYS * 24 * 3600) {
        let content = std::fs::read_to_string(&cache_path)
            .map_err(|e| format!("failed to read cached spec: {e}"))?;
        return serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse cached spec: {e}"));
    }
    let client = build_client()?;
    let body = safe_get_with_retry(&client, url, &format!("spec '{name}'"))?;
    let val: Value = serde_json::from_str(&body)
        .map_err(|e| format!("[RUPIA-NET003] spec '{name}' is not valid JSON: {e}"))?;
    std::fs::create_dir_all(&dir).ok();
    atomic_write(&cache_path, &body);
    Ok(val)
}

pub fn fetch_spec_fresh(url: &str, name: &str) -> Result<Value, String> {
    let client = build_client()?;
    let body = safe_get_with_retry(&client, url, &format!("spec '{name}'"))?;
    serde_json::from_str(&body)
        .map_err(|e| format!("[RUPIA-NET003] spec '{name}' is not valid JSON: {e}"))
}

pub(crate) fn sanitize_path_component(name: &str) -> String {
    name.replace(['/', '\\', ':', ' ', '.'], "_")
        .replace("..", "_")
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
        rate_limit();
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

    #[test]
    fn ssrf_blocks_localhost() {
        assert!(validate_url("http://localhost/secret").is_err());
        assert!(validate_url("http://127.0.0.1/secret").is_err());
        assert!(validate_url("http://169.254.169.254/metadata").is_err());
    }

    #[test]
    fn ssrf_blocks_private_networks() {
        assert!(validate_url("http://10.0.0.1/internal").is_err());
        assert!(validate_url("http://192.168.1.1/admin").is_err());
        assert!(validate_url("http://172.16.0.1/api").is_err());
    }

    #[test]
    fn ssrf_allows_public_urls() {
        assert!(validate_url("https://api.apis.guru/v2/list.json").is_ok());
        assert!(validate_url("https://example.com/spec.json").is_ok());
    }

    #[test]
    fn ssrf_blocks_invalid_scheme() {
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("ftp://internal/data").is_err());
    }

    #[test]
    fn ssrf_blocks_encoded_ips() {
        assert!(validate_url("http://0x7f000001/secret").is_err());
        assert!(validate_url("http://0177.0.0.1/secret").is_err());
    }

    #[test]
    fn ssrf_blocks_ipv6() {
        assert!(validate_url("http://[::1]/secret").is_err());
        assert!(validate_url("http://[::ffff:127.0.0.1]/secret").is_err());
    }

    #[test]
    fn ssrf_blocks_zero_ip() {
        assert!(validate_url("http://0.0.0.0/secret").is_err());
    }

    #[test]
    fn ssrf_private_172_correct_range() {
        assert!(validate_url("http://172.16.0.1/x").is_err());
        assert!(validate_url("http://172.31.255.1/x").is_err());
        assert!(validate_url("http://172.32.0.1/x").is_ok());
        assert!(validate_url("http://172.15.0.1/x").is_ok());
    }

    #[test]
    fn ssrf_allows_normal_domains() {
        assert!(validate_url("https://stripe.com/v1/charges").is_ok());
        assert!(validate_url("https://api.github.com/repos").is_ok());
    }

    #[test]
    fn sanitize_path_blocks_traversal() {
        assert!(!sanitize_path_component("../../etc/passwd").contains(".."));
        assert!(!sanitize_path_component("foo/../bar").contains(".."));
        assert_eq!(sanitize_path_component("normal-api"), "normal-api");
    }
}
