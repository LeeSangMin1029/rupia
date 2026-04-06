# rupia

Rust LLM output validation harness. Typia's "type → schema → validate → feedback → self-heal" pattern, in Rust.

LLMs produce broken JSON. Wrong types, missing fields, markdown wrappers, trailing commas.
rupia fixes what it can, validates the rest, and generates precise feedback so the LLM corrects itself.

**6.75% → 100% convergence** — the same pattern that makes Typia work for TypeScript.

## Complete Feature Table

### Core Pipeline

| Feature | Module | What It Does |
|---------|--------|-------------|
| Lenient Parse | `lenient` | Markdown block extraction, junk prefix skip, trailing comma, JS comments, unquoted keys, incomplete keywords (`tru`→`true`), unclosed brackets, unicode surrogate pairs |
| Coerce (10 types) | `coerce` | `"42"`→`42`, `"true"`→`true`, enum case insensitive, single→array wrap, default fill, string trim, number separators (`"1,000"`→`1000`), `"1.5k"`→`1500`, indexed object→array, enum number/string cross-convert |
| Validate | `validator` | JSON Schema: type, range (min/max/exclusive), format (22 types), enum, required, oneOf/anyOf, x-discriminator, uniqueItems |
| Feedback | `feedback` | `// ❌ [{"path":"$input.age","expected":"number & Minimum<0>"}]` inline annotations, missing property detection, array element placeholders |
| Guard | `guard` | Production pipeline: size limit, timeout, verbose, diagnostics (RUPIA error codes), self-healing loop |

### Format Validators (22 types, Typia regex 1:1)

| Format | Format | Format | Format |
|--------|--------|--------|--------|
| email | idn-email | uri | url |
| uri-reference | uri-template | iri | iri-reference |
| uuid | date-time | date | time |
| duration | ipv4 | ipv6 | hostname |
| idn-hostname | json-pointer | relative-json-pointer | byte (base64) |
| regex | password | | |

### LLM Function Calling

| Feature | What It Does |
|---------|-------------|
| `LlmFunction` | Schema + parse + validate + `to_openai_tool()` + `to_claude_tool()` |
| `LlmApplication` | Function collection + `find()` + `to_openai_tools()` / `to_claude_tools()` |
| `LlmController<T>` | Instance + dispatch + `execute()` / `execute_raw()` with auto coerce + validate |

### AVE (Adaptive Validation Engine)

| Phase | Feature | What It Does |
|-------|---------|-------------|
| 0 | Schema Resolution | Domain description → schema + relations + counterexamples (1 LLM call). 3-level summary (Haiku/Sonnet/Opus) |
| 2-3 | Confidence Validation | Coerce + validate + per-field confidence score (deterministic, no LLM) |
| 4 | Selective Retry | Failed fields only → LLM retry → merge into original → re-validate. Field group detection from relations |
| 5 | Schema Evolution | Trace analysis → auto proposals. 3-tier approval: Auto (description), Async (enum add), Sync (type change). Direction-limited: no loosening |

### Schema Operations

| Feature | What It Does |
|---------|-------------|
| `inject_constraints_to_description` | Inject min/max/format/enum into description for LLM awareness |
| `diff_schemas` | Compare old/new schemas, detect added/removed/changed fields, `is_compatible()` |
| `make_partial` | Remove required (PATCH scenarios) |
| `infer_schema` | Auto-infer schema from sample JSON values |
| `compress_feedback` | Deduplicate repeated errors, reduce tokens |
| `openapi_to_llm_tools` | OpenAPI paths → LLM function calling tools |
| `ValidationStats` | Track field error frequencies, `prompt_hints()` for improvement suggestions |

### Diagnostics

| Code | Category | Example |
|------|----------|---------|
| RUPIA-P001~P006 | Parse | Empty input, no JSON, size limit, malformed, depth exceeded |
| RUPIA-V001~V005 | Validation | Format violation, range, enum, required, type mismatch |
| RUPIA-G001~G003 | Guard | Size limit, timeout, convergence failure |
| RUPIA-S001~S004 | Schema | File not found, invalid JSON, missing type, no required |
| AVE-E001~E007 | AVE | No domain, schema gen fail, retry exhausted, relation violation, merge cascade, stall, schema corrupt |

### Other

| Feature | What It Does |
|---------|-------------|
| `random::generate` | Schema-aware random data generation (respects format/min/max/enum) |
| `#[derive(Harness)]` | Proc macro: Rust struct → JSON Schema via schemars |
| `sanitize_feedback` | Prompt injection pattern filtering |
| `is_stalled` | Detect 3 consecutive identical errors → early exit |
| Trace (ring buffer) | Success: stats only. Failure: full context preserved |

### CLI

```bash
rupia parse                           # Lenient JSON parse
rupia check --schema X [--strict]     # Full pipeline + diagnostics
rupia validate --schema X             # Schema validation + coerce
rupia feedback --schema X             # // ❌ inline feedback
rupia random --schema X --count N     # Schema-aware random data
rupia lint-schema --schema X          # Schema quality check
rupia ave --domain "shopping mall"    # AVE Phase 0: schema generation
rupia ave --schema X --input Y        # AVE Phase 2-3: confidence validation
```

## Stats

- **154 tests**, clippy clean
- **14 modules**, ~6,700 lines (production + test)
- **0 unsafe**, 0 panic, 0 network access
- Full pipeline: **3.2µs** (valid), **4.4µs** (malformed)

## Install

```bash
cargo install --git https://github.com/LeeSangMin1029/rupia rupia-cli
```

## License

MIT
