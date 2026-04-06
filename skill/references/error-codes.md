# rupia 에러 코드 전체 참조

## Parse (RUPIA-P0xx)

| 코드 | 해결 |
|---|---|
| P001 | LLM 빈 출력 → API 키, 모델 가용성, 프롬프트 확인 |
| P002 | JSON 없음 → 프롬프트에 "JSON만 반환" 추가 |
| P003 | 입력 크기 초과 → max_tokens 설정 |
| P004 | 잘못된 JSON 구조 → "표준 JSON 형식" 명시 |
| P005 | 유효하지 않은 key → JSON 잘림 확인 |
| P006 | 중첩 깊이 초과 → 스키마 단순화 |

## Validation (RUPIA-V0xx)

| 코드 | 해결 |
|---|---|
| V001 | format 위반 → 해당 format 예시를 프롬프트에 명시 |
| V002 | 범위 위반 → min/max를 프롬프트에 명시 |
| V003 | enum 불일치 → 허용값 목록을 프롬프트에 나열 |
| V004 | 필수 필드 누락 → "MUST include" 문구 추가 |
| V005 | 타입 불일치 → 스키마 타입 확인, coerce 실패 시 모델 변경 |
| V100 | 5+ errors → 스키마 단순화, 강한 모델, 예시 추가 |

## Guard (RUPIA-G0xx)

| 코드 | 해결 |
|---|---|
| G001 | 입력 크기 제한 초과 → Config.max_input_bytes 조정 |
| G002 | 타임아웃 → Config.timeout 증가 |
| G003 | 수렴 실패 → 스키마 단순화, 강한 모델 |

## Schema (RUPIA-S0xx)

| 코드 | 해결 |
|---|---|
| S001 | 스키마 파일 못 찾음 → 경로 확인 |
| S002 | 스키마 파싱 실패 → 재생성 |
| S003 | top-level type 없음 → "type": "object" 추가 |
| S004 | required 없음 → "required" 배열 추가 |

## AVE Pipeline (AVE-E0xx)

| 코드 | 해결 |
|---|---|
| E001 | 도메인 설명 없음 → `rupia ave --domain "도메인"` |
| E002 | 스키마 생성 2회 실패 → 도메인 설명 구체화 |
| E003 | 전체 재시도 3회 실패 → 엔티티 분리 |
| E004 | cross-field 관계 위반 → 관계 규칙 확인 |
| E005 | merge 후 연쇄 실패 → 필드 그룹 재생성 |
| E006 | 정체 (3회 동일 에러) → enum/format 확인 |
| E007 | 스키마 파일 손상 → lint-schema 후 재생성 |

## Strictness (AVE-S0xx)

| 코드 | 심각도 | 상황 |
|---|---|---|
| S001 | Block | 3+ properties인데 required 0 |
| S002 | Block | 모든 string에 format/enum/pattern 없음 |
| S003 | Warn | 숫자에 min/max 없음 |
| S004 | Warn | 배열에 items 없음 |
| S005 | Block | 모든 필드가 type "string" |
| S006 | Warn | enum 50+ 값 |
| S007 | Block | root type 없음 |
| S008 | Warn | nested object에 properties 없음 |

## Loosening (AVE-L0xx)

| 코드 | 심각도 | 상황 |
|---|---|---|
| L001 | Block | required 필드 제거 |
| L003 | Warn | format 제거 |
| L004 | Warn | min/max 범위 확장 |
| L005 | Block | type → string 다운그레이드 |
| L006 | Warn | 무제약 필드 추가 |
| L007 | Block | required 50%+ 감소 |
