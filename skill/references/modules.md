# rupia 모듈 참조

| 모듈 | 역할 |
|---|---|
| `lenient` | 관대한 JSON 파싱 (markdown, junk prefix, trailing comma, JS 주석, unquoted key, 불완전 키워드) |
| `coerce` | 10가지 기계적 교정 (string→int, enum case, single→array, default fill, trim, 숫자 구분자, indexed obj→array) |
| `validator` | jsonschema 크레이트 래퍼 (Draft 4~2020-12 전체 키워드) |
| `feedback` | `// ❌` 인라인 피드백 생성 (missing property, array element placeholder 포함) |
| `format` | 22종 포맷 검증 (email, uri, uuid, date-time, ipv4, ipv6, hostname 등) |
| `guard` | 프로덕션 방어 (size limit, timeout, 정체 감지, diagnostics) |
| `harness` | 경량 자가 수복 루프 (sanitize, stall detection) |
| `ave` | AVE 파이프라인 (스키마 해석, confidence, 선택적 재시도, 스키마 진화, 버전 관리, 안티패턴 감지, 느슨화 방지) |
| `llm` | LlmFunction / LlmApplication / LlmController (OpenAI/Claude tools 포맷) |
| `schema_ops` | schema diff, partial, infer, compress, inject constraints, openapi→tools, cross-reference, rule consistency |
| `schema_util` | 공통 $ref 해석, allOf 병합, nested 평탄화 (모든 모듈이 사용) |
| `random` | 스키마 준수 랜덤 데이터 생성 (22종 format별 생성기) |
| `boundary` | 경계값 자동 생성 (min/max 경계, enum, format, required — nested/allOf/$ref 지원) |
| `registry` | apis.guru 2,529개 API 검색 + OpenAPI에서 엔티티 스키마 추출 |
| `task_schemas` | 소/중/대 규모별 태스크 출력 스키마 내장 + 규모 자동 감지 |
| `diagnostic` | RUPIA/AVE 에러 코드 + 정확한 해결 안내 메시지 |
| `types` | 공통 타입 (Validation, ValidationError, ParseResult, HarnessConfig, HasSchema) |

## 검증 엔진 지원 키워드 (jsonschema 크레이트)

type, enum, const, allOf, anyOf, oneOf, not, if/then/else, $ref, $defs,
properties, required, additionalProperties, patternProperties, propertyNames,
minProperties, maxProperties, dependencies, dependentRequired, dependentSchemas,
items, prefixItems, contains, uniqueItems, minItems, maxItems, additionalItems,
unevaluatedProperties, unevaluatedItems,
minimum, maximum, exclusiveMinimum, exclusiveMaximum, multipleOf,
minLength, maxLength, pattern, format, contentEncoding, contentMediaType

## 자동 교정 10가지

1. `"25"` → `25` (string→integer, schema가 integer일 때)
2. `"true"` → `true` (string→boolean)
3. `"Admin"` → `"admin"` (enum case-insensitive 매칭)
4. `"tag"` → `["tag"]` (single value→array, schema가 array일 때)
5. 누락 + default 있음 → default 채움
6. `" hello "` → `"hello"` (string trim)
7. `"1,000"` → `1000` (숫자 구분자)
8. `"1.5k"` → `1500` (숫자 접미사)
9. `{"0":"a","1":"b"}` → `["a","b"]` (indexed object→array)
10. `1` → `"1"` or `"1"` → `1` (enum number↔string 교차 변환)
