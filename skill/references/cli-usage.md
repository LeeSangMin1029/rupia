# rupia CLI 커맨드 참조

## 설치

```bash
cargo install --git https://github.com/LeeSangMin1029/rupia rupia-cli
```

## 전체 커맨드

```bash
rupia parse                                    # 관대한 JSON 파싱
rupia check --schema <file> [--strict] [--json] # 교정 + 검증 + 피드백
rupia validate --schema <file> [--strict]       # 검증만 (coerce 포함)
rupia feedback --schema <file>                  # // ❌ 피드백만
rupia random --schema <file> --count <N>        # 스키마 준수 랜덤 데이터
rupia lint-schema --schema <file>               # 스키마 품질 검사
rupia boundary-gen --schema <file>              # 경계값 테스트 생성
rupia ave --domain <text> --model <tier>        # AVE 스키마 생성
rupia ave --schema <file> --input <file>        # AVE confidence 검증
```

## 공통 옵션

```
-v, --verbose    stderr에 진단 상세 출력
-s, --schema     JSON Schema 파일 경로
-i, --input      입력 파일 (생략 시 stdin)
--json           JSON 형식 출력
--strict         extra property 거부
```

## check 출력 형식

```json
{"status": "valid", "data": {...}, "diagnostics": []}

{"status": "invalid", "error_count": 2, "feedback": "```json\n{...// ❌...}\n```",
 "errors": [{"code": "RUPIA-V002", "message": "...", "help": "..."}]}
```

## boundary-gen 출력 형식

```json
[
  {"field": "age", "value": 0, "description": "age=minimum(0)", "expected_valid": true},
  {"field": "age", "value": -1, "description": "age=minimum-1(-1)", "expected_valid": false}
]
```

## 파이프 사용 예시

```bash
# LLM 출력 검증
echo "$LLM_OUTPUT" | rupia check --schema schema.json --json

# 랜덤 데이터로 roundtrip 테스트
rupia random -s schema.json > data.json
rupia check -s schema.json -i data.json --json

# 경계값 생성 → 검증
rupia boundary-gen -s schema.json | jq '.[0].value'
```
