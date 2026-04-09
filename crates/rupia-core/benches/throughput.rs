use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use serde_json::json;

use rupia_core::coerce::coerce_with_schema;
use rupia_core::feedback::stringify;
use rupia_core::lenient::parse;
use rupia_core::types::{Validation, ValidationError, ValidationFailure};
use rupia_core::validator::validate;

fn valid_json_small() -> &'static str {
    r#"{"name":"test","email":"t@e.co","age":25,"role":"admin"}"#
}

fn valid_json_medium() -> String {
    let items: Vec<String> = (0..50)
        .map(|i| {
            format!(
                r#"{{"id":{},"name":"item-{}","price":{},"in_stock":true,"tags":["tag-a","tag-b"]}}"#,
                i,
                i,
                f64::from(i) * 1.5
            )
        })
        .collect();
    format!(r#"{{"items":[{}],"total":50,"page":1}}"#, items.join(","))
}

fn malformed_json() -> &'static str {
    r#"Sure! Here is the JSON:
```json
{
    name: "test",
    // this is a comment
    "email": "t@e.co",
    "age": "25",
    "role": "admin",
}
```
Hope this helps!"#
}

fn schema_member() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "email": {"type": "string", "format": "email"},
            "age": {"type": "number", "minimum": 0, "maximum": 150},
            "role": {"type": "string", "enum": ["admin", "user", "guest"]}
        },
        "required": ["name", "email", "age", "role"]
    })
}

fn schema_items() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "name": {"type": "string"},
                        "price": {"type": "number", "minimum": 0},
                        "in_stock": {"type": "boolean"},
                        "tags": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["id", "name", "price", "in_stock"]
                }
            },
            "total": {"type": "integer"},
            "page": {"type": "integer"}
        },
        "required": ["items", "total", "page"]
    })
}

fn bench_lenient_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("lenient_parse");
    let small = valid_json_small();
    let medium = valid_json_medium();
    let malformed = malformed_json();

    group.throughput(Throughput::Bytes(small.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("valid_small", small.len()),
        &small,
        |b, input| {
            b.iter(|| parse(black_box(input)));
        },
    );

    group.throughput(Throughput::Bytes(medium.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("valid_medium", medium.len()),
        &medium,
        |b, input| {
            b.iter(|| parse(black_box(input)));
        },
    );

    group.throughput(Throughput::Bytes(malformed.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("malformed", malformed.len()),
        &malformed,
        |b, input| {
            b.iter(|| parse(black_box(input)));
        },
    );

    group.finish();
}

fn bench_coerce(c: &mut Criterion) {
    let mut group = c.benchmark_group("coerce");
    let schema = schema_member();
    let input = json!({"name": "test", "email": "t@e.co", "age": "25", "role": "admin"});

    group.bench_function("member_with_string_age", |b| {
        b.iter(|| coerce_with_schema(black_box(input.clone()), black_box(&schema)));
    });

    let schema_items = schema_items();
    let medium_val: serde_json::Value = serde_json::from_str(&valid_json_medium()).unwrap();
    group.bench_function("50_items_array", |b| {
        b.iter(|| coerce_with_schema(black_box(medium_val.clone()), black_box(&schema_items)));
    });

    group.finish();
}

fn bench_validate(c: &mut Criterion) {
    let mut group = c.benchmark_group("validate");
    let schema = schema_member();
    let valid = json!({"name": "test", "email": "t@e.co", "age": 25, "role": "admin"});
    let invalid = json!({"name": "test", "email": "bad", "age": -5, "role": "superadmin"});

    group.bench_function("valid_member", |b| {
        b.iter(|| validate(black_box(&valid), black_box(&schema)));
    });

    group.bench_function("invalid_member", |b| {
        b.iter(|| validate(black_box(&invalid), black_box(&schema)));
    });

    let schema_items = schema_items();
    let medium_val: serde_json::Value = serde_json::from_str(&valid_json_medium()).unwrap();
    group.bench_function("50_items_valid", |b| {
        b.iter(|| validate(black_box(&medium_val), black_box(&schema_items)));
    });

    group.finish();
}

fn bench_feedback(c: &mut Criterion) {
    let mut group = c.benchmark_group("feedback");
    let failure = ValidationFailure {
        data: json!({"name": "test", "email": "bad", "age": -5, "role": "superadmin"}),
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
            ValidationError {
                path: "$input.role".into(),
                expected: "one of [\"admin\",\"user\",\"guest\"]".into(),
                value: json!("superadmin"),
                description: None,
            },
        ],
    };

    group.bench_function("3_errors", |b| {
        b.iter(|| stringify(black_box(&failure)));
    });

    group.finish();
}

fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");
    let schema = schema_member();
    let raw_valid = valid_json_small();
    let raw_malformed = malformed_json();

    group.bench_function("valid_input", |b| {
        b.iter(|| {
            let parsed = parse(black_box(raw_valid));
            if let rupia_core::types::ParseResult::Success(v) = parsed {
                let coerced = coerce_with_schema(v, &schema);
                validate(&coerced, &schema)
            } else {
                Validation::Failure(ValidationFailure {
                    data: serde_json::Value::Null,
                    errors: vec![],
                })
            }
        });
    });

    group.bench_function("malformed_input", |b| {
        b.iter(|| {
            let parsed = parse(black_box(raw_malformed));
            if let rupia_core::types::ParseResult::Success(v) = parsed {
                let coerced = coerce_with_schema(v, &schema);
                validate(&coerced, &schema)
            } else {
                Validation::Failure(ValidationFailure {
                    data: serde_json::Value::Null,
                    errors: vec![],
                })
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_lenient_parse,
    bench_coerce,
    bench_validate,
    bench_feedback,
    bench_full_pipeline,
);
criterion_main!(benches);
