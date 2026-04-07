# rupia

LLM 출력 검증 하네스. 파싱→교정→검증→피드백→수렴. jsonschema Draft 4~2020-12.

## 아키텍처

```
crates/rupia-core/   lenient, coerce, validator, feedback, format, random,
                     llm, harness, guard, diagnostic, schema_ops, schema_util,
                     ave, boundary, registry, task_schemas
crates/rupia-derive/ #[derive(Harness)]
crates/rupia-cli/    CLI (check/feedback/ave/boundary-gen/random/lint-schema/parse/validate)
crates/rupia/        facade
```

## 테스트

```bash
cargo nextest r --status-level fail
cargo clippy --all-targets -- -D warnings
```

## Pipeline Scope Guidelines

- Modify at most 10 files total.
- Add at most 500 lines of new code.
- Do NOT modify CLAUDE.md.
