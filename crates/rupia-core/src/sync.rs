use crate::fetch;
use crate::schema_ops::{self, SchemaDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

const MANIFEST_FILE: &str = "sync-manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifest {
    pub domain: String,
    pub last_sync: String,
    pub apis: HashMap<String, ApiEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEntry {
    pub name: String,
    pub title: String,
    pub url: String,
    pub content_hash: String,
    pub synced_at: String,
    pub spec_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeReport {
    pub domain: String,
    pub checked_at: String,
    pub changes: Vec<ApiChange>,
    pub summary: ChangeSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiChange {
    pub api_name: String,
    pub change_type: ChangeType,
    pub diff: Option<SpecDiff>,
}

#[derive(Debug, Clone, Serialize)]
pub enum ChangeType {
    New,
    Updated,
    Removed,
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpecDiff {
    pub schemas_added: Vec<String>,
    pub schemas_removed: Vec<String>,
    pub schemas_changed: Vec<SchemaChangeDetail>,
    pub breaking: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaChangeDetail {
    pub schema_name: String,
    pub added_fields: Vec<String>,
    pub removed_fields: Vec<String>,
    pub changed_fields: Vec<FieldChange>,
    pub breaking: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub old_value: String,
    pub new_value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeSummary {
    pub total_apis: usize,
    pub new_apis: usize,
    pub updated_apis: usize,
    pub removed_apis: usize,
    pub breaking_changes: usize,
}

fn manifest_path(domain: &str) -> PathBuf {
    let dir = fetch::cache_dir().join("sync");
    let safe = domain.replace(['/', '\\', ':', ' '], "_");
    dir.join(format!("{safe}-{MANIFEST_FILE}"))
}

fn now_iso() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let epoch = dur.as_secs();
    format!("{epoch}")
}

fn content_hash(val: &Value) -> String {
    let s = serde_json::to_string(val).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn load_manifest(domain: &str) -> Option<SyncManifest> {
    let path = manifest_path(domain);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_manifest(manifest: &SyncManifest) -> Result<(), String> {
    let path = manifest_path(&manifest.domain);
    std::fs::create_dir_all(path.parent().unwrap()).ok();
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("failed to serialize manifest: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("failed to write manifest: {e}"))
}

fn save_synced_spec(domain: &str, api_name: &str, spec: &Value) -> Result<String, String> {
    let dir = fetch::cache_dir()
        .join("sync")
        .join(domain.replace(['/', '\\', ':', ' '], "_"));
    std::fs::create_dir_all(&dir).ok();
    let safe = api_name.replace(['/', '\\', ':', ' '], "_");
    let path = dir.join(format!("{safe}.json"));
    let json =
        serde_json::to_string_pretty(spec).map_err(|e| format!("failed to serialize spec: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("failed to write spec: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

fn load_synced_spec(spec_path: &str) -> Option<Value> {
    let content = std::fs::read_to_string(spec_path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn sync_domain(domain: &str, max_apis: usize) -> Result<SyncManifest, String> {
    let list = fetch::fetch_api_list()?;
    let apis = fetch::search_apis(&list, domain, max_apis);
    if apis.is_empty() {
        return Err(format!(
            "[RUPIA-SYNC001] no APIs found for domain '{domain}'"
        ));
    }
    let now = now_iso();
    let mut entries = HashMap::new();
    for api in &apis {
        if api.openapi_url.is_empty() {
            continue;
        }
        match fetch::fetch_spec(&api.openapi_url, &api.name) {
            Ok(spec) => {
                let hash = content_hash(&spec);
                let spec_path = save_synced_spec(domain, &api.name, &spec)?;
                entries.insert(
                    api.name.clone(),
                    ApiEntry {
                        name: api.name.clone(),
                        title: api.title.clone(),
                        url: api.openapi_url.clone(),
                        content_hash: hash,
                        synced_at: now.clone(),
                        spec_path,
                    },
                );
            }
            Err(e) => {
                eprintln!("[warn] skipping {}: {e}", api.name);
            }
        }
    }
    if entries.is_empty() {
        return Err(format!(
            "[RUPIA-SYNC002] downloaded 0 specs for domain '{domain}'"
        ));
    }
    let manifest = SyncManifest {
        domain: domain.to_string(),
        last_sync: now,
        apis: entries,
    };
    save_manifest(&manifest)?;
    Ok(manifest)
}

fn extract_named_schemas(spec: &Value) -> HashMap<String, Value> {
    let schemas = spec
        .pointer("/components/schemas")
        .or_else(|| spec.get("definitions"))
        .and_then(Value::as_object);
    match schemas {
        Some(obj) => obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        None => HashMap::new(),
    }
}

fn diff_spec(old_spec: &Value, new_spec: &Value) -> SpecDiff {
    let old_schemas = extract_named_schemas(old_spec);
    let new_schemas = extract_named_schemas(new_spec);
    let added: Vec<String> = new_schemas
        .keys()
        .filter(|k| !old_schemas.contains_key(*k))
        .cloned()
        .collect();
    let removed: Vec<String> = old_schemas
        .keys()
        .filter(|k| !new_schemas.contains_key(*k))
        .cloned()
        .collect();
    let mut changed = Vec::new();
    for (name, old_schema) in &old_schemas {
        if let Some(new_schema) = new_schemas.get(name) {
            if content_hash(old_schema) != content_hash(new_schema) {
                let sd: SchemaDiff = schema_ops::diff_schemas(old_schema, new_schema);
                if !sd.is_empty() {
                    let breaking = !sd.is_compatible();
                    changed.push(SchemaChangeDetail {
                        schema_name: name.clone(),
                        added_fields: sd.added,
                        removed_fields: sd.removed.clone(),
                        changed_fields: sd
                            .changed
                            .iter()
                            .map(|c| FieldChange {
                                field: c.field.clone(),
                                old_value: c.old.clone(),
                                new_value: c.new.clone(),
                            })
                            .collect(),
                        breaking,
                    });
                }
            }
        }
    }
    let breaking = !removed.is_empty() || changed.iter().any(|c| c.breaking);
    SpecDiff {
        schemas_added: added,
        schemas_removed: removed,
        schemas_changed: changed,
        breaking,
    }
}

pub fn detect_changes(domain: &str, max_apis: usize) -> Result<ChangeReport, String> {
    let old_manifest = load_manifest(domain).ok_or_else(|| {
        format!("[RUPIA-SYNC003] no previous sync for domain '{domain}' — run --sync first")
    })?;
    let list = fetch::fetch_api_list()?;
    let apis = fetch::search_apis(&list, domain, max_apis);
    let now = now_iso();
    let mut changes = Vec::new();
    let mut new_count = 0usize;
    let mut updated_count = 0usize;
    let mut breaking_count = 0usize;
    for api in &apis {
        if api.openapi_url.is_empty() {
            continue;
        }
        let new_spec = match fetch::fetch_spec_fresh(&api.openapi_url, &api.name) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[warn] skipping {}: {e}", api.name);
                continue;
            }
        };
        let new_hash = content_hash(&new_spec);
        if let Some(old_entry) = old_manifest.apis.get(&api.name) {
            if old_entry.content_hash == new_hash {
                changes.push(ApiChange {
                    api_name: api.name.clone(),
                    change_type: ChangeType::Unchanged,
                    diff: None,
                });
            } else {
                let old_spec = load_synced_spec(&old_entry.spec_path);
                let diff = old_spec.map(|os| diff_spec(&os, &new_spec));
                if let Some(ref d) = diff {
                    if d.breaking {
                        breaking_count += 1;
                    }
                }
                updated_count += 1;
                changes.push(ApiChange {
                    api_name: api.name.clone(),
                    change_type: ChangeType::Updated,
                    diff,
                });
            }
        } else {
            new_count += 1;
            changes.push(ApiChange {
                api_name: api.name.clone(),
                change_type: ChangeType::New,
                diff: None,
            });
        }
    }
    let current_names: std::collections::HashSet<&str> =
        apis.iter().map(|a| a.name.as_str()).collect();
    let removed_count = old_manifest
        .apis
        .keys()
        .filter(|k| !current_names.contains(k.as_str()))
        .count();
    for old_name in old_manifest.apis.keys() {
        if !current_names.contains(old_name.as_str()) {
            changes.push(ApiChange {
                api_name: old_name.clone(),
                change_type: ChangeType::Removed,
                diff: None,
            });
        }
    }
    Ok(ChangeReport {
        domain: domain.to_string(),
        checked_at: now,
        changes,
        summary: ChangeSummary {
            total_apis: apis.len(),
            new_apis: new_count,
            updated_apis: updated_count,
            removed_apis: removed_count,
            breaking_changes: breaking_count,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn content_hash_deterministic() {
        let v = json!({"a": 1, "b": "hello"});
        assert_eq!(content_hash(&v), content_hash(&v));
    }

    #[test]
    fn content_hash_differs() {
        let v1 = json!({"a": 1});
        let v2 = json!({"a": 2});
        assert_ne!(content_hash(&v1), content_hash(&v2));
    }

    #[test]
    fn diff_spec_detects_added_schema() {
        let old = json!({"components": {"schemas": {"Order": {"type": "object", "properties": {"id": {"type": "string"}}}}}});
        let new = json!({"components": {"schemas": {
            "Order": {"type": "object", "properties": {"id": {"type": "string"}}},
            "Payment": {"type": "object", "properties": {"amount": {"type": "number"}}}
        }}});
        let diff = diff_spec(&old, &new);
        assert!(diff.schemas_added.contains(&"Payment".to_string()));
        assert!(!diff.breaking);
    }

    #[test]
    fn diff_spec_detects_removed_schema() {
        let old = json!({"components": {"schemas": {
            "Order": {"type": "object", "properties": {"id": {"type": "string"}}},
            "Payment": {"type": "object", "properties": {"amount": {"type": "number"}}}
        }}});
        let new = json!({"components": {"schemas": {"Order": {"type": "object", "properties": {"id": {"type": "string"}}}}}});
        let diff = diff_spec(&old, &new);
        assert!(diff.schemas_removed.contains(&"Payment".to_string()));
        assert!(diff.breaking);
    }

    #[test]
    fn diff_spec_detects_field_change() {
        let old = json!({"components": {"schemas": {"Order": {"type": "object", "properties": {
            "id": {"type": "string"},
            "status": {"type": "string"}
        }}}}});
        let new = json!({"components": {"schemas": {"Order": {"type": "object", "properties": {
            "id": {"type": "string"},
            "state": {"type": "string"}
        }}}}});
        let diff = diff_spec(&old, &new);
        assert_eq!(diff.schemas_changed.len(), 1);
        assert_eq!(diff.schemas_changed[0].schema_name, "Order");
        assert!(diff.schemas_changed[0]
            .removed_fields
            .contains(&"status".to_string()));
        assert!(diff.schemas_changed[0]
            .added_fields
            .contains(&"state".to_string()));
        assert!(diff.schemas_changed[0].breaking);
    }

    #[test]
    fn diff_spec_no_change() {
        let spec = json!({"components": {"schemas": {"Order": {"type": "object", "properties": {"id": {"type": "string"}}}}}});
        let diff = diff_spec(&spec, &spec);
        assert!(diff.schemas_added.is_empty());
        assert!(diff.schemas_removed.is_empty());
        assert!(diff.schemas_changed.is_empty());
        assert!(!diff.breaking);
    }

    #[test]
    fn extract_schemas_from_definitions() {
        let spec = json!({"definitions": {"User": {"type": "object"}}});
        let schemas = extract_named_schemas(&spec);
        assert!(schemas.contains_key("User"));
    }

    #[test]
    fn manifest_roundtrip() {
        let manifest = SyncManifest {
            domain: "test".to_string(),
            last_sync: "2026-01-01".to_string(),
            apis: HashMap::new(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: SyncManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.domain, "test");
    }
}
