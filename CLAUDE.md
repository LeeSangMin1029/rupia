# rupia

Rust LLM 하네스. Typia의 "타입→스키마→검증→피드백→자가수복" 패턴을 Rust/Go로 제공.

## 아키텍처

```
crates/rupia-core/     lenient parser, coerce, validator, feedback, harness loop
crates/rupia-derive/   proc macro: #[derive(Harness)]
crates/rupia/          facade re-export
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
