# rupia

LLM output validation harness. Parse broken JSON, coerce types, validate against schemas, generate feedback so the AI fixes itself.

## Install

```bash
# From crates.io
cargo install rupia-cli

# From source
cargo install --git https://github.com/LeeSangMin1029/rupia rupia-cli
```

Pre-built binaries for Linux, macOS (x86_64 + ARM), and Windows are available on [Releases](https://github.com/LeeSangMin1029/rupia/releases).

## What it does

AI gives you this:

```
Sure! Here is your data:
```json
{name: "Alice", "age": "25", "role": "Admin",}
```
```

rupia turns it into this (3 microseconds, no LLM call):

```json
{"name": "Alice", "age": 25, "role": "admin"}
```

Markdown stripped, unquoted key fixed, trailing comma removed, `"25"` coerced to `25`, `"Admin"` lowered to match enum. If it still fails schema validation, rupia generates `// ❌` inline feedback the AI can read to fix itself.

## Usage

```bash
# Validate AI output (most common)
echo "$AI_OUTPUT" | rupia check --schema schema.json --json

# Generate schema from domain description
rupia ave --domain "hospital appointment system"

# Boundary value test generation
rupia boundary-gen --schema schema.json

# Random test data
rupia random --schema schema.json --count 10

# Schema quality check
rupia lint-schema --schema schema.json

# Cross-reference against public APIs
rupia cross-ref --domain "payment" --json

# Monitor API changes across domains
rupia watch --domains payment messaging maps --sync-first

# LLM feedback only
rupia feedback --schema schema.json
```

## Features

| Feature | Description |
|---------|-------------|
| Lenient parsing | Markdown blocks, junk prefix, trailing commas, JS comments, unquoted keys |
| 10 auto-coercions | `"25"`->25, `"Admin"`->"admin", `"tag"`->["tag"], null->default, trim, etc. |
| JSON Schema validation | jsonschema crate (Draft 4 through 2020-12, allOf/oneOf/if-then-else/not) |
| Feedback generation | `// ❌ [{"path":"$input.age","expected":"Minimum<0>"}]` inline |
| JSONLogic rules | Conditional required, arithmetic relations, mutual exclusion |
| AVE pipeline | Auto schema generation, confidence scoring, selective retry, schema evolution |
| Boundary generation | min/max, enum, format, required edges. Nested/allOf/$ref supported |
| API cross-reference | 2,529 public APIs from apis.guru. Universal enums, constraints, divergences |
| API change monitoring | Sync snapshots, diff for breaking changes, multi-domain watch |
| Rule consistency | Order contradictions, range contradictions, arithmetic infeasibility |
| Anti-pattern detection | 0 required fields, all-string, missing root type, plus 5 more |
| Loosening prevention | Blocks required removal, format removal, type downgrade |
| LLM Function Calling | OpenAI/Claude tools format auto-generation |
| Custom derive attributes | `#[rupia(format="email", min=0, max=150)]` |

## As a Rust library

```rust
use rupia::{Harness, HasSchema, parse_validate_typed};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema, Harness)]
struct Order {
    #[rupia(format = "uuid")]
    order_id: String,
    #[rupia(min = 0)]
    total: f64,
    status: String,
}

let order: Order = parse_validate_typed(raw_llm_output)?;
```

## Security

10 vulnerabilities found and fixed. CSO audit: 0 critical, 0 high.

- SSRF blocking (private IPs, hex/octal/IPv6 encoding, cloud metadata)
- 50MB response size limit, 30s request timeout, rate limiting
- Atomic cache writes (no corruption on concurrent access)
- Prompt injection filter (20 patterns, case-insensitive, DoS-capped)
- Unsafe mmap bounds validation (safetensors tensor size check)
- Path traversal prevention on all cache paths

See [docs/rupia-architecture.md](docs/rupia-architecture.md) for the full defense matrix.

## Specs

- **275 tests**, clippy clean (pedantic)
- **21 modules**, ~10,000 lines
- **1 unsafe** (mmap, bounds checked)
- jsonschema crate (Draft 4 through 2020-12)
- GitHub Actions CI (Linux/macOS/Windows)
- [crates.io v0.2.0](https://crates.io/crates/rupia)
- MIT license
