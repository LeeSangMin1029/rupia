# rupia Rust API 참조

## 기본 패턴

```rust
use rupia::{Harness, HasSchema, parse_validate_typed};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema, Harness)]
struct MyOutput {
    task_id: String,
    status: String,
}

let output: MyOutput = parse_validate_typed(raw)?;
```

## 프로덕션 패턴 (guard)

```rust
use rupia::guard::{self, Config};
use std::time::Duration;

let config = Config {
    max_input_bytes: 16 * 1024 * 1024,
    max_retries: 10,
    timeout: Duration::from_secs(30),
    strict: false,
    verbose: false,
};

match guard::check(raw, &schema, &config) {
    Ok(result) => use_data(result.value),
    Err(e) => {
        for d in &e.diagnostics { eprintln!("{d}"); }
        if let Some(fb) = &e.last_feedback { retry_with_feedback(fb); }
    }
}
```

## 검증 엔진

jsonschema 크레이트 기반 (Draft 4~2020-12).
JSON Schema 전체 키워드 지원:
- type (배열 포함), enum, const
- allOf, anyOf, oneOf, not
- if/then/else
- properties, required, additionalProperties
- patternProperties, propertyNames
- minProperties, maxProperties
- items, prefixItems, contains, uniqueItems
- minItems, maxItems, additionalItems
- minimum, maximum, exclusiveMinimum, exclusiveMaximum, multipleOf
- minLength, maxLength, pattern, format
- $ref, $defs, dependencies, dependentRequired, dependentSchemas
- unevaluatedProperties

## 성능

| 작업 | 시간 |
|------|------|
| 파싱 (valid 56B) | 675ns |
| 전체 파이프라인 (valid) | 3.2µs |
| 전체 파이프라인 (malformed) | 4.4µs |
