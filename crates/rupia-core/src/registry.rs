use serde_json::Value;

pub const APIS_GURU_LIST_URL: &str = "https://api.apis.guru/v2/list.json";

pub struct ApiEntry {
    pub name: String,
    pub preferred_version: String,
    pub openapi_url: String,
    pub title: String,
    pub description: String,
}

pub fn search_apis(list_json: &Value, keyword: &str) -> Vec<ApiEntry> {
    let kw = keyword.to_lowercase();
    let Some(map) = list_json.as_object() else {
        return vec![];
    };
    let mut results = vec![];
    for (api_name, api_val) in map {
        let preferred = api_val
            .get("preferred")
            .and_then(Value::as_str)
            .unwrap_or("");
        let version_info = api_val
            .get("versions")
            .and_then(|v| v.get(preferred))
            .unwrap_or(api_val);
        let info = version_info.get("info").unwrap_or(version_info);
        let title = info.get("title").and_then(Value::as_str).unwrap_or("");
        let desc = info
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let openapi_url = version_info
            .get("swaggerUrl")
            .or_else(|| version_info.get("openapiVer"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let name_lower = api_name.to_lowercase();
        let title_lower = title.to_lowercase();
        let desc_lower = desc.to_lowercase();
        if name_lower.contains(&kw) || title_lower.contains(&kw) || desc_lower.contains(&kw) {
            results.push(ApiEntry {
                name: api_name.clone(),
                preferred_version: preferred.to_string(),
                openapi_url: openapi_url.to_string(),
                title: title.to_string(),
                description: desc.to_string(),
            });
        }
    }
    results
}

pub fn extract_entity_schemas(specs: &[Value], entity_hint: &str) -> Vec<Value> {
    let hint = entity_hint.to_lowercase();
    let mut results = vec![];
    for spec in specs {
        let schemas = spec
            .get("components")
            .and_then(|c| c.get("schemas"))
            .or_else(|| spec.get("definitions"));
        let Some(schemas_map) = schemas.and_then(Value::as_object) else {
            continue;
        };
        for (name, schema) in schemas_map {
            if name.to_lowercase().contains(&hint) {
                results.push(schema.clone());
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mock_list_json() -> Value {
        json!({
            "stripe.com": {
                "preferred": "v1",
                "versions": {
                    "v1": {
                        "info": {
                            "title": "Stripe Payment API",
                            "description": "Online payment processing"
                        },
                        "swaggerUrl": "https://api.stripe.com/v1/spec"
                    }
                }
            },
            "paypal.com": {
                "preferred": "v2",
                "versions": {
                    "v2": {
                        "info": {
                            "title": "PayPal Payment Gateway",
                            "description": "Payment and checkout services"
                        },
                        "swaggerUrl": "https://api.paypal.com/v2/spec"
                    }
                }
            },
            "github.com": {
                "preferred": "v3",
                "versions": {
                    "v3": {
                        "info": {
                            "title": "GitHub REST API",
                            "description": "Source code hosting platform"
                        },
                        "swaggerUrl": "https://api.github.com/v3/spec"
                    }
                }
            },
            "weather.gov": {
                "preferred": "v1",
                "versions": {
                    "v1": {
                        "info": {
                            "title": "Weather API",
                            "description": "Weather forecast data"
                        },
                        "swaggerUrl": "https://api.weather.gov/spec"
                    }
                }
            },
            "maps.google.com": {
                "preferred": "v1",
                "versions": {
                    "v1": {
                        "info": {
                            "title": "Google Maps API",
                            "description": "Geolocation and mapping"
                        },
                        "swaggerUrl": "https://maps.google.com/spec"
                    }
                }
            }
        })
    }

    #[test]
    fn search_payment_finds_two() {
        let list = mock_list_json();
        let results = search_apis(&list, "payment");
        assert_eq!(results.len(), 2);
        let names: Vec<&str> = results.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"stripe.com"));
        assert!(names.contains(&"paypal.com"));
    }

    #[test]
    fn extract_order_schema() {
        let spec = json!({
            "components": {
                "schemas": {
                    "Order": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "total": {"type": "number"}
                        }
                    },
                    "User": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"}
                        }
                    }
                }
            }
        });
        let results = extract_entity_schemas(&[spec], "order");
        assert_eq!(results.len(), 1);
        assert!(results[0].get("properties").is_some());
        assert!(results[0]["properties"].get("total").is_some());
    }

    #[test]
    fn extract_from_definitions() {
        let spec = json!({
            "definitions": {
                "OrderItem": {
                    "type": "object",
                    "properties": {"quantity": {"type": "integer"}}
                }
            }
        });
        let results = extract_entity_schemas(&[spec], "order");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_case_insensitive() {
        let list = mock_list_json();
        let results = search_apis(&list, "WEATHER");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "weather.gov");
    }
}
