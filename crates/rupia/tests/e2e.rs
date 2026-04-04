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
    assert_eq!(*result.errors_per_attempt.last().unwrap(), 0);
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
