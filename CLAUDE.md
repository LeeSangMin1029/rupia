# rupia

Rust LLM 하네스. "파싱→교정→검증→피드백→자가수복" 파이프라인 + AVE(Adaptive Validation Engine).

## 아키텍처

```
crates/rupia-core/     lenient, coerce, validator, feedback, format, random,
                       llm, harness, guard, diagnostic, schema_ops, ave
crates/rupia-derive/   proc macro: #[derive(Harness)]
crates/rupia-cli/      CLI: parse/check/validate/feedback/random/lint-schema/ave
crates/rupia/          facade re-export
```

## 핵심 모듈

- `lenient` — 관대한 JSON 파싱 (markdown, junk, 불완전 JSON)
- `coerce` — 10가지 기계적 교정 (enum case, single→array, default, trim, 숫자 구분자 등)
- `validator` — JSON Schema 검증 (22종 format, range, enum, uniqueItems, x-discriminator)
- `feedback` — `// ❌` 인라인 피드백 생성
- `guard` — 프로덕션 방어 (timeout, 정체 감지, 진단 코드)
- `ave` — AVE 파이프라인 (스키마 해석, confidence 검증, 선택적 재시도, 스키마 진화, 버전 관리, 안티패턴 감지, 느슨화 방지)
- `llm` — LlmFunction/Application/Controller (OpenAI/Claude tools 포맷)
- `schema_ops` — diff, partial, infer, compress, openapi→tools, stats

## 테스트

```bash
cargo nextest r --status-level fail
cargo clippy --all-targets -- -D warnings
```

## Pipeline Scope Guidelines

- Modify at most 10 files total.
- Add at most 500 lines of new code.
- Do NOT modify CLAUDE.md.
