# rupia

LLM 출력 검증 하네스. AI가 뭘 뱉든 고치고, 검증하고, 틀리면 AI가 스스로 고치게 만드는 도구.

## 설치

```bash
cargo install --git https://github.com/LeeSangMin1029/rupia rupia-cli
```

## 사용

```bash
# AI 출력 검증 (가장 많이 사용)
echo "$AI_OUTPUT" | rupia check --schema schema.json --json

# 도메인에서 스키마 자동 생성
rupia ave --domain "쇼핑몰 주문 시스템"

# 경계값 테스트 생성
rupia boundary-gen --schema schema.json

# 랜덤 데이터 생성
rupia random --schema schema.json --count 10

# 스키마 품질 검사
rupia lint-schema --schema schema.json
```

## 기능

| 기능 | 설명 |
|------|------|
| 관대한 파싱 | markdown, junk prefix, trailing comma, JS 주석, unquoted key 자동 복구 |
| 자동 교정 10가지 | `"25"`→`25`, `"Admin"`→`"admin"`, `"tag"`→`["tag"]`, default 채움, trim 등 |
| JSON Schema 검증 | jsonschema 크레이트 (Draft 4~2020-12 전체 키워드) |
| 피드백 생성 | `// ❌ [{"path":"$input.age","expected":"Minimum<0>"}]` 인라인 |
| AVE 파이프라인 | 스키마 자동 생성, confidence 검증, 선택적 재시도, 스키마 진화, 버전 관리 |
| 경계값 생성 | min/max 경계, enum, format, required — nested/allOf/$ref 지원 |
| 다중 API 교차 분석 | apis.guru 2,529개 API에서 보편 규칙 추출 |
| 규칙 일관성 검증 | 순서 모순, 범위 모순, 산술 불가능 감지 |
| 안티패턴 감지 | required 0, 전부 string, root type 없음 등 8가지 |
| 느슨화 방지 | required 제거, format 제거, type 다운그레이드 차단 |
| 태스크 스키마 | 소/중/대 규모별 내장 + 자동 감지 |
| LLM Function Calling | OpenAI/Claude tools 포맷 자동 생성 |

## 스펙

- **214 tests**, clippy clean
- **19 modules**, ~8,000 lines
- **0 unsafe**, 0 panic, 0 network access
- jsonschema 크레이트 (Draft 4~2020-12)
- MIT license
