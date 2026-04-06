---
name: rupia
description: AI가 생성한 JSON을 자동 교정 + 검증. 실패 시 LLM이 스스로 고칠 수 있는 // ❌ 피드백 생성. jsonschema Draft 4~2020-12 완전 지원. 10가지 자동 교정(타입변환, enum case, default 채움 등). 에이전트가 JSON을 만들거나, 스키마 검증이 필요하거나, "validate", "검증", "check" 언급 시 사용.
argument-hint: "check --schema <file> | ave --domain <text> | lint | boundary-gen | random"
user-invocable: true
---

# rupia — AI 출력 검증

AI가 JSON을 만들 때마다 이 루프를 따른다:

1. JSON 출력 생성
2. `rupia check --schema <schema> --json` 으로 검증
3. `"status":"valid"` → 다음 작업
4. `"status":"invalid"` → `feedback` 필드의 `// ❌` 마커를 읽고 해당 필드만 수정
5. 2번으로 돌아감 (최대 3회)

```bash
echo "$OUTPUT" | rupia check --schema schema.json --json
```

## 전제 조건

```bash
which rupia 2>/dev/null && rupia --version || echo "NOT_INSTALLED"
```

미설치: `cargo install --git https://github.com/LeeSangMin1029/rupia rupia-cli`

## 커맨드

### check — 검증 + 교정 + 피드백

```bash
echo "$INPUT" | rupia check --schema schema.json --json
echo "$INPUT" | rupia check --schema schema.json --strict --json
```

깨진 JSON도 자동 복구 (markdown, trailing comma, unquoted key).
`"25"`→`25`, `"Admin"`→`"admin"`, `"tag"`→`["tag"]` 등 10가지 자동 교정.

### feedback — LLM용 피드백만

```bash
echo "$INPUT" | rupia feedback --schema schema.json
```

### ave — 도메인에서 스키마 생성

```bash
rupia ave --domain "쇼핑몰 주문" --model sonnet
```

### boundary-gen — 경계값 테스트

```bash
rupia boundary-gen --schema schema.json
```

### random — 스키마 준수 랜덤 데이터

```bash
rupia random --schema schema.json --count 10
```

### lint-schema — 스키마 품질

```bash
rupia lint-schema --schema schema.json
```

### parse — 관대한 JSON 파싱

```bash
echo "$BROKEN" | rupia parse
```

## 에러 코드

RUPIA-P(파싱), V(검증), G(가드), S(스키마), AVE-E(파이프라인), S(엄격도), L(느슨화).
상세: [error-codes.md](references/error-codes.md)

## 참고

- Rust API: [rust-api.md](references/rust-api.md)
- 모듈 상세: [modules.md](references/modules.md)
- CLI 상세: [cli-usage.md](references/cli-usage.md)
