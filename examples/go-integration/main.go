// Example: Using rupia CLI from Go for LLM output validation.
//
// Pattern:
//   1. Define Go struct with json tags
//   2. Generate JSON Schema via go generate (using invopop/jsonschema)
//   3. Pipe LLM output through `rupia check --schema schema.json`
//   4. Parse result: "valid" → use data, "invalid" → feed back errors to LLM
//
// Install:
//   go install github.com/invopop/jsonschema/cmd/jsonschema@latest
//   cargo install --path crates/rupia-cli
//
//go:generate jsonschema -o schema.json -type TaskOutput
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strings"
)

type TaskOutput struct {
	TaskID       string   `json:"task_id" jsonschema:"required"`
	Status       string   `json:"status" jsonschema:"required,enum=done|failed"`
	ChangedFiles []string `json:"changed_files" jsonschema:"required"`
}

type RupiaResult struct {
	Status     string          `json:"status"`
	Data       json.RawMessage `json:"data,omitempty"`
	Feedback   string          `json:"feedback,omitempty"`
	ErrorCount int             `json:"error_count,omitempty"`
	Errors     []RupiaError    `json:"errors,omitempty"`
}

type RupiaError struct {
	Path     string `json:"path"`
	Expected string `json:"expected"`
}

func validateLLMOutput(schemaPath string, llmOutput string) (*RupiaResult, error) {
	cmd := exec.Command("rupia", "check", "--schema", schemaPath)
	cmd.Stdin = strings.NewReader(llmOutput)

	output, err := cmd.Output()
	if err != nil {
		// rupia check returns 0 even for invalid input (outputs JSON status)
		// Only real errors (missing binary, bad schema) return non-zero
		if exitErr, ok := err.(*exec.ExitError); ok {
			return nil, fmt.Errorf("rupia failed: %s", string(exitErr.Stderr))
		}
		return nil, fmt.Errorf("rupia not found: %w", err)
	}

	var result RupiaResult
	if err := json.Unmarshal(output, &result); err != nil {
		return nil, fmt.Errorf("parse rupia output: %w", err)
	}
	return &result, nil
}

// selfHealingLoop demonstrates the Typia-style self-healing pattern:
// LLM output → rupia validate → feedback → LLM retry → converge
func selfHealingLoop(schemaPath string, callLLM func(feedback string) string, maxRetries int) (*TaskOutput, error) {
	feedback := ""
	for i := 0; i <= maxRetries; i++ {
		llmOutput := callLLM(feedback)

		result, err := validateLLMOutput(schemaPath, llmOutput)
		if err != nil {
			return nil, err
		}

		switch result.Status {
		case "valid":
			var task TaskOutput
			if err := json.Unmarshal(result.Data, &task); err != nil {
				return nil, fmt.Errorf("unmarshal valid data: %w", err)
			}
			fmt.Printf("Converged after %d attempt(s)\n", i+1)
			return &task, nil

		case "invalid":
			feedback = result.Feedback
			fmt.Printf("Attempt %d: %d errors, retrying with feedback...\n", i+1, result.ErrorCount)

		case "parse_error":
			feedback = "Your output was not valid JSON. Please return valid JSON matching the schema."
			fmt.Printf("Attempt %d: parse error, retrying...\n", i+1)
		}
	}
	return nil, fmt.Errorf("failed to converge after %d attempts", maxRetries+1)
}

func main() {
	// Demo: direct validation
	schemaPath := "schema.json"

	// Check if schema exists
	if _, err := os.Stat(schemaPath); os.IsNotExist(err) {
		fmt.Println("schema.json not found. Run: go generate")
		fmt.Println("Or create it manually with your struct definition.")
		fmt.Println()
		fmt.Println("Example schema.json:")
		fmt.Println(`{
  "type": "object",
  "properties": {
    "task_id": {"type": "string"},
    "status": {"type": "string", "enum": ["done", "failed"]},
    "changed_files": {"type": "array", "items": {"type": "string"}}
  },
  "required": ["task_id", "status", "changed_files"]
}`)
		return
	}

	// Simulate LLM that fails first, then succeeds
	attempt := 0
	task, err := selfHealingLoop(schemaPath, func(feedback string) string {
		attempt++
		if attempt == 1 {
			// First attempt: bad output
			return `{"task_id": "T001", "status": "unknown", "changed_files": "not-an-array"}`
		}
		// Second attempt: corrected after feedback
		return `{"task_id": "T001", "status": "done", "changed_files": ["src/lib.rs"]}`
	}, 5)

	if err != nil {
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Result: %+v\n", task)
}
