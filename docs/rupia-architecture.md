---
title: rupia 아키텍처 — 역할 범위와 설계 원칙
date: 2026-04-07
---

# rupia는 무엇을 하고, 무엇을 안 하는가

## 핵심 정체성

AI가 만든 구조화된 출력(JSON)을 스키마 기반으로 자동 검증·교정하는 하네스.
스키마 자체의 정확성은 실제 API 교차검증으로 보장.

## 레이어 구조

```
1층 (핵심)     AI 출력 → 파싱 → 교정 → 검증 → 피드백 → 자기수정
2층 (스키마)   AVE 자동 생성, 경계값 테스트, 안티패턴 감지, 느슨화 방지
3층 (교차검증)  2,529개 공개 API와 비교해서 스키마 품질 확인
4층 (동기화)   교차검증 대상 API를 추적·비교 (sync/diff)
```

## rupia가 강한 곳

| 영역 | 이유 |
|------|------|
| AI 에이전트 자기 검증 루프 | 3μs에 교정+검증, LLM 재호출 최소화 |
| "거의 맞지만 조금 틀린" LLM 출력 | "25"→25, "Admin"→"admin", trailing comma 등 0비용 교정 |
| 스키마 없는 프로젝트 빠른 시작 | `rupia ave --domain "..."` 30초에 스키마+관계+반례 생성 |
| 복잡한 필드 간 관계 | JSONLogic으로 조건부 필수, 산술 관계, 상호 배타 검증 |
| 외부 API 변경 감지 | sync/diff로 breaking change 사전 감지 |

## rupia가 안 하는 것

| 영역 | 이유 | 대안 |
|------|------|------|
| 내부 정합성 (프론트↔백) | 컴파일 타임에 잡아야 함. 런타임은 늦음 | 공유 타입, OpenAPI codegen, GraphQL |
| 비정형 텍스트 검증 | JSON Schema 기반이라 범위 밖 | 별도 NLP/LLM 평가 |
| 실시간 스트리밍 | 완성된 JSON을 받아야 검증 가능 | 청크 버퍼링 후 검증 |
| 의미 판단 (hallucination) | 형식은 맞지만 내용이 틀린 건 못 잡음 | RAG + 사실 검증 |
| API 변경 대응 코드 수정 | rupia는 감지만, 수정은 사람/에이전트 | 이슈 생성 → 개발자 처리 |

## 외부 API 의존성 관리 흐름

앱이 여러 외부 API(Stripe, Twilio, Google Maps 등)에 종속적일 때:

```
rupia: 감지 → "Stripe가 legacy_id 삭제함" (사실 전달)
이슈:  판단 → "우리 앱에 영향 있나? 어디를 고쳐야 하나?"
코드:  처리 → 앱 수정, 배포
```

### 왜 컴파일러가 못 잡는가

외부 API 응답은 런타임 JSON이라 타입 시스템 밖에 있음.
OpenAPI codegen을 써도 "스펙을 누가 갱신하느냐"의 문제가 남음.
rupia가 그 갱신 감지를 자동화.

### 운영 흐름

```bash
# 1. 의존하는 API들 스냅샷
rupia cross-ref --sync --domain "payment"

# 2. 주기적으로 변경 감지 (cron/CI)
rupia cross-ref --diff --domain "payment"

# 3. breaking change 감지 → 이슈 생성 → 개발자 처리
```

## 내부 정합성은 왜 rupia가 안 하는가

내부 정합성(프론트↔백 타입 일치)은 이미 해결된 문제:
- TypeScript: 공유 타입 패키지
- GraphQL: 스키마 하나 → codegen
- OpenAPI: 스펙 하나 → 양쪽 코드 생성
- Protobuf: .proto → 양쪽 코드 생성

rupia가 런타임에 비교하는 건 늦고, 이미 더 잘하는 도구들과 겹침.
rupia는 컴파일러가 못 잡는 것(LLM 출력, 외부 API 변경, 도메인 규칙)에 집중.

## v0.2 기능 전체 목록

| 기능 | CLI 커맨드 | 설명 |
|------|-----------|------|
| 관대한 파싱 | `rupia parse` | markdown, junk prefix, trailing comma, unquoted key, JS 주석 |
| 자동 교정 | `rupia check` | "25"→25, "Admin"→"admin", null→default, 단일값→배열 등 10종 |
| JSON Schema 검증 | `rupia validate` | jsonschema 크레이트 (Draft 4~2020-12, allOf/oneOf/if-then-else/not/patternProperties) |
| 피드백 생성 | `rupia feedback` | `// ❌` 인라인 피드백, LLM 자기수정용 |
| AVE 파이프라인 | `rupia ave` | 스키마 자동 생성, confidence 검증, 선택적 재시도, 스키마 진화 |
| 경계값 생성 | `rupia boundary-gen` | min/max/enum/format/required 경계 — nested/allOf/$ref 지원 |
| 랜덤 데이터 | `rupia random` | 22개 format별 생성기, 스키마 제약 준수 |
| 스키마 품질 | `rupia lint-schema` | 8종 안티패턴 + 느슨화 방지 + counterexamples 자동 검증 |
| 교차 분석 | `rupia cross-ref` | apis.guru 2,529개 API에서 보편 enum/제약/발산 추출 |
| API 동기화 | `rupia cross-ref --sync` | 도메인 API 스냅샷 로컬 저장 |
| 변경 감지 | `rupia cross-ref --diff` | 이전 스냅샷 대비 breaking change 감지 |
| 다중 도메인 모니터링 | `rupia watch` | 설정 파일 기반 일괄 sync/diff, CI 연동 (exit 1 on breaking) |
| JSONLogic 규칙 | `check` + rules 필드 | 조건부 필수, 산술 관계, 상호 배타 — RuleEngine 사전 컴파일 |
| LLM Function Calling | 라이브러리 | LlmFunction/Application/Controller — OpenAI/Claude tools 포맷 |
| 커스텀 속성 | `#[rupia(...)]` | format, min, max, min_length, max_length, pattern 6종 |
| 규칙 일관성 검증 | 라이브러리 | 순서 모순, 범위 모순, 산술 불가능 감지 |

## 보안 방어 현황

### CSO 감사 결과 (2026-04-08)

**CRITICAL: 0 / HIGH: 0 / MEDIUM: 1** (Cargo.lock 미추적)

### 수정된 취약점 10건

| # | 취약점 | 심각도 | 대응 |
|---|--------|--------|------|
| 1 | SSRF — localhost/private IP 접근 | 높 | validate_url() — 블랙리스트 + IPv4 파싱 |
| 2 | SSRF 우회 — hex/octal/IPv6 인코딩 | 높 | 0x 접두사, 선행 0, ::, ffff 차단 |
| 3 | 응답 크기 무제한 | 높 | 50MB 제한 (Content-Length + body 이중 검증) |
| 4 | unsafe mmap OOB read | 높 | tensor data 바이트 수 검증 후 포인터 접근 |
| 5 | sanitize_feedback 위치 드리프트 | 높 | 매 치환 후 재스캔 |
| 6 | sanitize_feedback DoS (O(n²)) | 중 | 100회 상한 |
| 7 | 경로 조작 (..) | 중 | sanitize_path_component — 모든 위험 문자 치환 |
| 8 | 캐시 파일 경쟁 조건 | 중 | atomic write (tmp→rename) |
| 9 | lenient.rs unwrap | 중 | 방어적 None 처리 |
| 10 | injection 필터 부족 | 낮 | 5→20 패턴, 대소문자 무시 |

### 방어 체계

| 공격 벡터 | 방어 |
|----------|------|
| 대용량 입력 | 16MB (guard), 50MB (HTTP), 1MB (JSONLogic) |
| 깊은 중첩 | 512 깊이 (parser), 128 (serde_json), 16 ($ref 재귀) |
| 무한 루프 | JSONLogic 50ms 타임아웃, 순환 $ref 깊이 제한 |
| ReDoS | format 10KB 길이 가드 |
| 프롬프트 인젝션 | 20개 패턴, 대소문자 무시, 100회 상한 |
| SSRF | IP 파싱, private 범위 차단, hex/octal/IPv6 차단 |
| 메모리 안전 | unsafe 1곳 — bounds check 적용 |
| prototype pollution | Rust/serde_json 면역 |
| 커맨드 인젝션 | 외부 프로세스 호출 0건 |

## 스펙

- **275 tests**, clippy clean (pedantic)
- **21 modules**, ~10,000 lines
- **unsafe 1곳** (mmap, bounds checked)
- jsonschema 크레이트 (Draft 4~2020-12)
- **crates.io v0.2.0** 배포 완료
- MIT license
