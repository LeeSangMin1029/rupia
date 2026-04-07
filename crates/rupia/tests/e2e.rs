use rupia::{
    HasSchema, Harness, HarnessConfig, Validation, parse_validate, parse_validate_typed,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema, Harness, PartialEq)]
struct Member {
    name: String,
    email: String,
    age: u32,
    role: Role,
}

#[derive(Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Role {
    Admin,
    User,
    Guest,
}

#[test]
fn full_pipeline_valid() {
    let schema = Member::rupia_schema();
    let raw = r#"{"name":"홍길동","email":"hong@ex.com","age":25,"role":"admin"}"#;
    let result = parse_validate(raw, &schema);
    assert!(result.is_success());
}

#[test]
fn full_pipeline_typed() {
    let raw = r#"{"name":"홍길동","email":"hong@ex.com","age":25,"role":"admin"}"#;
    let member: Member = parse_validate_typed(raw).unwrap();
    assert_eq!(member.name, "홍길동");
    assert_eq!(member.age, 25);
    assert_eq!(member.role, Role::Admin);
}

#[test]
fn pipeline_with_coercion() {
    let schema = Member::rupia_schema();
    let raw = r#"{"name":"test","email":"t@e.co","age":"30","role":"user"}"#;
    let result = parse_validate(raw, &schema);
    assert!(result.is_success());
    if let Validation::Success(v) = result {
        assert_eq!(v["age"], 30);
    }
}

#[test]
fn pipeline_markdown_wrapped() {
    let schema = Member::rupia_schema();
    let raw = "Here is the member:\n```json\n{\"name\":\"test\",\"email\":\"t@e.co\",\"age\":25,\"role\":\"guest\"}\n```";
    let result = parse_validate(raw, &schema);
    assert!(result.is_success());
}

#[test]
fn pipeline_junk_prefix_and_trailing_comma() {
    let schema = Member::rupia_schema();
    let raw = r#"Sure! {"name":"test","email":"t@e.co","age":25,"role":"user",}"#;
    let result = parse_validate(raw, &schema);
    assert!(result.is_success());
}

#[test]
fn pipeline_invalid_produces_feedback() {
    let schema = Member::rupia_schema();
    let raw = r#"{"name":"test","email":"bad","age":-5,"role":"superadmin"}"#;
    let result = parse_validate(raw, &schema);
    assert!(!result.is_success());
    if let Validation::Failure(f) = result {
        let feedback = f.to_llm_feedback();
        assert!(feedback.contains("// ❌"));
        assert!(feedback.contains("```json"));
        assert!(f.error_count() > 0);
    }
}

#[test]
fn pipeline_missing_required() {
    let schema = Member::rupia_schema();
    let raw = r#"{"name":"test"}"#;
    let result = parse_validate(raw, &schema);
    assert!(!result.is_success());
    if let Validation::Failure(f) = result {
        let feedback = f.to_llm_feedback();
        assert!(feedback.contains("email"));
        assert!(feedback.contains("age"));
        assert!(feedback.contains("role"));
    }
}

#[test]
fn pipeline_unquoted_keys_and_comments() {
    let schema = Member::rupia_schema();
    let raw = r#"{
        // member info
        name: "test",
        email: "t@e.co",
        age: 25,
        role: "admin"
    }"#;
    let result = parse_validate(raw, &schema);
    assert!(result.is_success());
}

#[derive(Debug, Deserialize, JsonSchema, Harness)]
struct NestedOutput {
    task_id: String,
    result: TaskResult,
    changed_files: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TaskResult {
    status: String,
    message: String,
}

#[test]
fn nested_struct_pipeline() {
    let raw = r#"{
        "task_id": "T001",
        "result": {"status": "done", "message": "All tests pass"},
        "changed_files": ["src/lib.rs", "src/main.rs"]
    }"#;
    let result: NestedOutput = parse_validate_typed(raw).unwrap();
    assert_eq!(result.task_id, "T001");
    assert_eq!(result.result.status, "done");
    assert_eq!(result.result.message, "All tests pass");
    assert_eq!(result.changed_files.len(), 2);
}

#[test]
fn nested_struct_coerced_from_string() {
    let schema = NestedOutput::rupia_schema();
    let raw = r#"{"task_id":"T001","result":"{\"status\":\"done\",\"message\":\"ok\"}","changed_files":["a.rs"]}"#;
    let result = parse_validate(raw, &schema);
    assert!(result.is_success());
    if let Validation::Success(v) = result {
        assert_eq!(v["result"]["status"], "done");
    }
}

#[tokio::test]
async fn harness_loop_converges() {
    let schema = json!({
        "type": "object",
        "properties": {
            "answer": {"type": "number", "minimum": 0, "maximum": 100}
        },
        "required": ["answer"]
    });
    let result = rupia::harness_loop(
        &schema,
        |feedback| {
            let has_fb = feedback.is_some();
            async move {
                if has_fb {
                    r#"{"answer": 42}"#.to_owned()
                } else {
                    r#"{"answer": -999}"#.to_owned()
                }
            }
        },
        HarnessConfig {
            max_retries: 5,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(result.value["answer"], 42);
    assert!(result.attempts >= 2);
}

#[tokio::test]
async fn harness_loop_first_try() {
    let schema = json!({
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"]
    });
    let result = rupia::harness_loop(
        &schema,
        |_| async { r#"{"name": "ok"}"#.to_owned() },
        HarnessConfig::default(),
    )
    .await
    .unwrap();
    assert_eq!(result.attempts, 1);
}

#[derive(Debug, Deserialize, JsonSchema, Harness, PartialEq)]
struct Constrained {
    #[rupia(format = "email")]
    pub email: String,
    #[rupia(min = 0, max = 150)]
    pub age: u32,
    #[rupia(min_length = 1, max_length = 50)]
    pub name: String,
    #[rupia(pattern = r"^[A-Z]{2}\d{4}$")]
    pub code: String,
}

#[test]
fn rupia_attr_format_injected() {
    let schema = Constrained::rupia_schema();
    assert_eq!(schema["properties"]["email"]["format"], "email");
}

#[test]
fn rupia_attr_min_max_injected() {
    let schema = Constrained::rupia_schema();
    assert_eq!(schema["properties"]["age"]["minimum"], 0.0);
    assert_eq!(schema["properties"]["age"]["maximum"], 150.0);
}

#[test]
fn rupia_attr_length_injected() {
    let schema = Constrained::rupia_schema();
    assert_eq!(schema["properties"]["name"]["minLength"], 1);
    assert_eq!(schema["properties"]["name"]["maxLength"], 50);
}

#[test]
fn rupia_attr_pattern_injected() {
    let schema = Constrained::rupia_schema();
    assert_eq!(schema["properties"]["code"]["pattern"], r"^[A-Z]{2}\d{4}$");
}

#[test]
fn rupia_attr_validation_enforced() {
    let schema = Constrained::rupia_schema();
    let raw = r#"{"email":"not-email","age":200,"name":"","code":"bad"}"#;
    let result = parse_validate(raw, &schema);
    assert!(!result.is_success());
}

#[test]
fn rupia_attr_valid_data_passes() {
    let raw = r#"{"email":"user@example.com","age":25,"name":"Alice","code":"AB1234"}"#;
    let c: Constrained = parse_validate_typed(raw).unwrap();
    assert_eq!(c.email, "user@example.com");
    assert_eq!(c.age, 25);
    assert_eq!(c.name, "Alice");
    assert_eq!(c.code, "AB1234");
}

#[derive(Debug, Deserialize, JsonSchema, Harness, PartialEq)]
struct NoAttrs {
    pub plain: String,
}

#[test]
fn no_rupia_attr_still_works() {
    let raw = r#"{"plain":"hello"}"#;
    let n: NoAttrs = parse_validate_typed(raw).unwrap();
    assert_eq!(n.plain, "hello");
}

#[test]
fn rule_engine_e2e_with_check() {
    use rupia_core::ave::{JsonLogicRule, RuleEngine};
    let rules = vec![
        JsonLogicRule {
            description: "total = subtotal + tax".into(),
            logic: json!({"==": [{"var": "total"}, {"+": [{"var": "subtotal"}, {"var": "tax"}]}]}),
        },
        JsonLogicRule {
            description: "shipped needs tracking".into(),
            logic: json!({"if": [
                {"==": [{"var": "status"}, "shipped"]},
                {"!!": {"var": "tracking"}},
                true
            ]}),
        },
    ];
    let engine = RuleEngine::new(&rules);
    assert!(engine.evaluate(&json!({"subtotal": 100, "tax": 10, "total": 110, "status": "pending"})).is_empty());
    assert_eq!(engine.evaluate(&json!({"subtotal": 100, "tax": 10, "total": 999, "status": "pending"})).len(), 1);
    assert_eq!(engine.evaluate(&json!({"subtotal": 100, "tax": 10, "total": 110, "status": "shipped"})).len(), 1);
    assert_eq!(engine.evaluate(&json!({"subtotal": 100, "tax": 10, "total": 999, "status": "shipped"})).len(), 2);
    assert!(engine.evaluate(&json!({"subtotal": 100, "tax": 10, "total": 110, "status": "shipped", "tracking": "TRK1"})).is_empty());
}

#[test]
fn rule_engine_batch_e2e() {
    use rupia_core::ave::{JsonLogicRule, RuleEngine};
    let rules = vec![JsonLogicRule {
        description: "amount > 0".into(),
        logic: json!({">": [{"var": "amount"}, 0]}),
    }];
    let engine = RuleEngine::new(&rules);
    let items: Vec<serde_json::Value> = (0..100)
        .map(|i| json!({"amount": i64::from(i) - 50}))
        .collect();
    let failures = engine.evaluate_batch(&items);
    assert_eq!(failures.len(), 51);
    assert_eq!(failures[0].0, 0);
}
