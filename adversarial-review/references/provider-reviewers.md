# Provider Reviewer Adapters

Read this reference during reviewer discovery, before selecting a same-provider
fallback. Prefer a native provider-aware subagent over a CLI when the runtime
exposes one, but do not treat absence from the native subagent picker as proof
that another provider is unavailable.

## Availability Discovery

Identify the author provider, inspect the native reviewer choices, and check
whether the adapters documented below are installed, for example with
`command -v claude` and `command -v codex`. When an installed different-provider
adapter's authentication or health is uncertain, run a minimal read-only smoke
prompt with a bounded timeout before falling back. Do not include repository
artifacts in that smoke prompt.

Record the mechanisms checked and their outcomes in the work item. A
same-provider fallback is permitted only when no suitable different-provider
mechanism is installed or an installed mechanism fails the bounded smoke check
or review invocation. Native-picker absence alone never permits fallback.

## Common Contract

Set `PROMPT`, `REVIEW_FILE`, and `REVIEW_SCOPE` before using an adapter. The
orchestrator may also set optional `REVIEWER_MODEL` and `REVIEWER_EFFORT` from
its selected model/reasoning profile. Keep the review read-only, run it in the
foreground, preserve streaming output when available, and require the
adversarial status block. Do not force a particular subscription or credential
source; inherit the user's configured provider access by default.

## Anthropic CLI

Use only when Anthropic differs from the author provider, or when a fresh
same-provider Anthropic session is the selected fallback.

```bash
CLAUDE_STREAM_FILE="${REVIEW_FILE%.md}.anthropic.jsonl"
CLAUDE_ARGS=(
  -p
  --verbose
  --permission-mode bypassPermissions
  --output-format stream-json
  --disallowedTools "Bash,Edit,Write,NotebookEdit"
  --add-dir "$REVIEW_SCOPE"
)

if [[ -n "${REVIEWER_MODEL:-}" ]]; then
  CLAUDE_ARGS+=(--model "$REVIEWER_MODEL")
fi
if [[ -n "${REVIEWER_EFFORT:-}" ]]; then
  CLAUDE_ARGS+=(--effort "$REVIEWER_EFFORT")
fi

printf '%s\n' "$PROMPT" | claude "${CLAUDE_ARGS[@]}" | tee "$CLAUDE_STREAM_FILE"

python3 - "$CLAUDE_STREAM_FILE" "$REVIEW_FILE" <<'PY'
import json
import pathlib
import sys

stream_path, review_path = map(pathlib.Path, sys.argv[1:])
result = None
for line in stream_path.read_text(encoding="utf-8").splitlines():
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    if event.get("type") == "result" and isinstance(event.get("result"), str):
        result = event["result"]
if result is None:
    raise SystemExit("Anthropic stream did not contain a final result")
review_path.write_text(result.rstrip() + "\n", encoding="utf-8")
PY
```

Pass the prompt through stdin. `claude --help` supports `--model` and `--effort`;
the adapter passes each only when the orchestrator selected it, and never
hardcodes a model or effort level. Preserve the selected model/reasoning profile
in the review record even when the runtime cannot report it back. The prompt
must prohibit mutating Bash commands and external-system changes. Extract the
final result from the stream and verify the status block before accepting the
review.

## OpenAI CLI

Use only when OpenAI differs from the author provider, or when a fresh
same-provider OpenAI session is the selected fallback.

```bash
CODEX_STREAM_FILE="${REVIEW_FILE%.md}.openai.jsonl"

printf '%s\n' "$PROMPT" | codex --search exec \
  --sandbox read-only \
  --cd "$PWD" \
  --add-dir "$REVIEW_SCOPE" \
  --skip-git-repo-check \
  --json \
  --output-last-message "$REVIEW_FILE" | tee "$CODEX_STREAM_FILE"
```

Pass the prompt through stdin and verify the saved result contains the required
status block.

## Other Providers

Use another provider only when its available mechanism supports a fresh
context, the required evidence access, read-only operation, and capture of a
final review artifact. Record provider and model as reported by the runtime.
