# rupia

Rust LLM output validation harness. Typia's "type → schema → validate → feedback → self-heal" pattern, in Rust.

LLMs produce broken JSON. Wrong types, missing fields, markdown wrappers, trailing commas.
rupia fixes what it can, validates the rest, and generates precise feedback so the LLM corrects itself.

**6.75% → 100% convergence** — the same pattern that makes Typia work for TypeScript.

## Quick Start

### Rust Library

```rust
use rupia::{Harness, HasSchema, parse_validate_typed};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema, Harness)]
struct TaskOutput {
    task_id: String,
    status: String,
    changed_files: Vec<String>,
}

let raw = r#"{"task_id": "T001", "status": "done", "changed_files": ["src/lib.rs"]}"#;
let output: TaskOutput = parse_validate_typed(raw).unwrap();
```

### CLI (Go, Python, any language)

```bash
# Install
cargo install --path crates/rupia-cli

# Parse lenient JSON
echo '{"name": "test", }' | rupia parse

# Validate against schema
echo '{"age": -5}' | rupia check --schema schema.json

# Get LLM feedback
echo '{"age": -5}' | rupia feedback --schema schema.json
# Output:
# ```json
# {
#   "age": -5 // ❌ [{"path":"$input.age","expected":"number & Minimum<0>"}]
# }
# ```

# Lint your schema
rupia lint-schema --schema schema.json
```

### Self-Healing Loop

```rust
use rupia::{GuardConfig, guard};

let result = guard::guarded_loop(
    &schema,
    |feedback| async {
        call_llm(prompt, feedback).await
    },
    &GuardConfig::default(),
).await?;
// result.value — validated output
// result.attempts — how many tries it took
// result.diagnostics — any warnings
```

## What It Does

| Stage | Typia Equivalent | What Happens |
|-------|-----------------|--------------|
| **Parse** | `parseLenientJson` | Strips markdown blocks, skips junk prefix, fixes trailing commas, completes partial keywords (`tru`→`true`), handles JS comments |
| **Coerce** | `coerceLlmArguments` | `"42"`→`42` when schema expects number. Resolves `$ref`, discriminated unions |
| **Validate** | `OpenApiStationValidator` | Checks types, ranges, formats (email/uri/uuid), required fields, enum values |
| **Feedback** | `stringifyValidationFailure` | `// ❌ [{"path":"$input.age","expected":"number & Minimum<0>"}]` inline annotations |
| **Guard** | — | Full pipeline + diagnostics with error codes (RUPIA-P001..V005..G003) |

## Diagnostics

Every error has a code, a message, and a **help** string that tells you exactly what to do:

```
[error][RUPIA-P002] LLM output contains no recognizable JSON
  help: The output was not JSON and no JSON object/array was found.
        First 100 chars: "Sure! I'll help you with that..."
        Ensure your prompt asks for JSON output explicitly.
        Try adding: "Respond with a JSON object only, no explanation."
```

Error codes:
- `RUPIA-P0xx` — Parse errors (empty input, no JSON found, malformed, depth exceeded)
- `RUPIA-V0xx` — Validation errors (format, range, enum, required, type mismatch)
- `RUPIA-G0xx` — Guard errors (size limit, timeout, convergence failure)
- `RUPIA-S0xx` — Schema errors (file not found, invalid JSON, missing type)

## Performance

| Operation | Size | Time | Throughput |
|-----------|------|------|-----------|
| Parse (valid JSON) | 56B | 675ns | 79 MiB/s |
| Parse (malformed) | 160B | 1.7us | 89 MiB/s |
| Full pipeline (valid) | 56B | 3.2us | — |
| Full pipeline (malformed) | 160B | 4.4us | — |
| Validate (50 items) | 4KB | 145us | — |
| Feedback (3 errors) | — | 5.8us | — |

## Go Integration

rupia is language-agnostic via CLI + JSON Schema files. See `examples/go-integration/` for the pattern:

1. Define Go struct → `go generate` → `schema.json`
2. Pipe LLM output through `rupia check --schema schema.json`
3. Parse result: `"valid"` → use data, `"invalid"` → feed `feedback` back to LLM

## Project Structure

```
crates/
  rupia-core/     — Parse, coerce, validate, feedback, guard, diagnostic
  rupia-derive/   — #[derive(Harness)] proc macro
  rupia-cli/      — CLI binary (rupia parse/check/validate/feedback/lint-schema)
  rupia/          — Facade crate re-exporting everything
```

## Security

- Zero `unsafe` code
- Input size limit: 16MB (configurable)
- Parse recursion depth limit: 512
- No external process execution
- No network access
- Feedback strings sanitized against prompt injection
- Memory-safe by Rust guarantees

## License

MIT
