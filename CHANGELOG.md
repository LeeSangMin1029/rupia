# Changelog

## v0.2.1 (2026-04-08)

### Security

- Block SSRF via hex IPs (`0x7f000001`), octal IPs (`0177.0.0.1`), IPv6 (`[::ffff:127.0.0.1]`), cloud metadata endpoints
- Fix `sanitize_feedback` position drift that let injection patterns bypass the filter after the first replacement
- Cap sanitize passes at 100 to prevent O(n^2) DoS on repeated patterns
- Validate safetensors tensor data size before unsafe mmap pointer access (prevents out-of-bounds read on malicious model files)
- Atomic cache writes via tmp+rename to prevent file corruption on concurrent access
- Track Cargo.lock for reproducible binary builds (supply chain protection)

### Added

- `rupia watch` command for multi-domain API change monitoring with config file support
- GitHub Actions CI (test on Linux/macOS/Windows, clippy, fmt)
- GitHub Actions release workflow (cross-compile 4 targets on tag push)

### Fixed

- `lint-schema` on AVE packages now checks the inner schema, not the wrapper JSON

## v0.2.0 (2026-04-07)

### Added

- JSONLogic rule engine (`datalogic-rs`) for conditional required fields, arithmetic relations, mutual exclusion
- `RuleEngine` struct with pre-compilation and `evaluate_batch()` for high-throughput validation
- `#[rupia(format, min, max, min_length, max_length, pattern)]` derive attributes
- API sync/diff pipeline: `rupia cross-ref --sync` and `--diff` for tracking spec changes over time
- Counterexamples auto-verification in `check` and `ave` commands
- `ave --json` output now includes rules and schema warnings
- Published to crates.io: rupia, rupia-core, rupia-derive, rupia-cli

### Security

- SSRF blocking on all HTTP requests (private IPs, localhost, metadata endpoints)
- 50MB response size limit with Content-Length pre-check
- 30s request timeout with 1-retry and 200ms rate limiting
- Path traversal prevention via `sanitize_path_component`
- Prompt injection filter expanded from 5 to 20 patterns, case-insensitive
- `lenient.rs` unwrap removed, defensive None handling

## v0.1.0 (2026-04-06)

### Added

- Lenient JSON parser (markdown extraction, junk prefix, trailing comma, JS comments, unquoted keys)
- 10 mechanical coercions (string-to-int, enum case, single-to-array, default fill, trim, etc.)
- JSON Schema validation via jsonschema crate (Draft 4 through 2020-12)
- Inline feedback generation (`// ❌` format for LLM self-correction)
- AVE pipeline (schema generation, confidence scoring, selective retry, schema evolution)
- Boundary value test generation (min/max, enum, format, required edges)
- Random data generation (22 format-specific generators)
- Schema quality checks (8 anti-patterns, loosening prevention)
- Cross-field relation validation
- Rule consistency checking (order/range contradictions, arithmetic infeasibility)
- Multi-API cross-reference via apis.guru (2,529 APIs)
- LLM Function Calling (OpenAI/Claude tools format)
- Task output schemas (Small/Medium/Large)
- `#[derive(Harness)]` proc macro
- CLI: parse, check, validate, feedback, ave, boundary-gen, random, lint-schema, cross-ref
