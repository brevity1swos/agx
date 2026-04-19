# Eval / training-harness integration guide

This doc is for anyone piping agx output into a downstream pipeline —
custom eval harnesses, training-data prep, dataset publishing. It
covers the three things an integrator actually needs:

1. [Stable JSON schema](#stable-json-schema) — the contract agx promises to hold
2. [Anonymization checklist](#anonymization-checklist) — before you publish
3. [Adapter recipes](#adapter-recipes) — concrete copy-paste starters for common harnesses

If the JSON schema below drifts without a minor-version bump + a
CHANGELOG entry, that's a bug. File an issue.

---

## Stable JSON schema

`agx --export json <session>` produces a single pretty-printed JSON
document. `agx --export trajectory-openai <session>` produces a
single line of compact JSONL in OpenAI fine-tuning format.

### `--export json`

```json
{
  "totals": {
    "tokens_in": 740,
    "tokens_out": 345,
    "cache_read": 6810,
    "cache_create": 1500,
    "cost_usd": 0.0753,
    "unique_models": ["claude-opus-4-6"]
  },
  "steps": [
    {
      "label": "[user]   Write a Python function …",
      "detail": "Write a Python function that returns …",
      "kind": "user_text",
      "tool_name": null,
      "timestamp_ms": 1776412800000,
      "duration_ms": null,
      "model": null,
      "tokens_in": null,
      "tokens_out": null,
      "cache_read": null,
      "cache_create": null,
      "is_fork_root": false,
      "tool_call_id": null
    },
    {
      "label": "[asst]   I'll create a fib.py …",
      "detail": "I'll create a fib.py with an iterative implementation …",
      "kind": "assistant_text",
      "tool_name": null,
      "timestamp_ms": 1776412801000,
      "duration_ms": 1000,
      "model": "claude-opus-4-6",
      "tokens_in": 500,
      "tokens_out": 120,
      "cache_read": 0,
      "cache_create": 1500
    }
  ],
  "annotations": [
    {
      "step_index": 3,
      "text": "tool call under review",
      "created_at_ms": 1776412850000,
      "updated_at_ms": 1776412850000
    }
  ]
}
```

Field reference:

| Field | Type | Notes |
|---|---|---|
| `totals.tokens_in` | u64 | Sum across all steps. Never negative. |
| `totals.tokens_out` | u64 | Same. |
| `totals.cache_read` | u64 | Anthropic cache-read tokens. `0` when the provider doesn't report it. |
| `totals.cache_create` | u64 | Anthropic cache-create tokens. Same fallback. |
| `totals.cost_usd` | f64 or null | `null` when no step had a known model in the pricing table. |
| `totals.unique_models` | string[] | Models observed in this session. Order: first-seen. |
| `steps[]` | array | Chronological. One entry per emitted step. |
| `steps[].kind` | string enum | `"user_text"` \| `"assistant_text"` \| `"tool_use"` \| `"tool_result"`. |
| `steps[].tool_name` | string or null | Set on tool_use / tool_result. |
| `steps[].tool_call_id` | string or null | Pairs tool_use ↔ tool_result. Omitted when null. |
| `steps[].timestamp_ms` | u64 or null | Unix ms. `null` when the source format has no timestamps. |
| `steps[].duration_ms` | u64 or null | Sequential duration since the previous step's timestamp. |
| `steps[].model` | string or null | Set only on the first step emitted from each assistant message. |
| `steps[].tokens_*` / `cache_*` | u64 or null | Same attach-to-first convention. |
| `steps[].is_fork_root` | bool | True for Claude Code edit/resume branch roots. Always false for other formats. |
| `annotations` | array or omitted | Present only when the session has any stored notes. |

**Stability commitment:** existing field names / types / enum values
are load-bearing public surface. New fields may appear. Removals or
renames are breaking changes that require a minor-version bump and
an entry in the README cross-tool compatibility table.

### `--export trajectory-openai`

One line of compact JSONL per session, directly consumable by OpenAI
fine-tuning / batch endpoints.

```json
{"messages":[
  {"role":"user","content":"Write a Python function …"},
  {"role":"assistant","content":"I'll create a fib.py …"},
  {"role":"assistant","tool_calls":[{"id":"toolu_abc","type":"function","function":{"name":"Write","arguments":"{\"file_path\":\"fib.py\"}"}}]},
  {"role":"tool","tool_call_id":"toolu_abc","content":"File created successfully"}
]}
```

Mapping rules:

- `user_text` → `{role: "user", content}`
- `assistant_text` → `{role: "assistant", content}`
- `tool_use` → `{role: "assistant", tool_calls: [{id, type: "function", function: {name, arguments}}]}`
  - `arguments` is a JSON-encoded string per OpenAI spec (the value is
    a string, not an object).
- `tool_result` → `{role: "tool", tool_call_id, content}`
  - `tool_call_id` matches the corresponding tool_use's `id`.

When `--redact <NEEDLE>` is passed, every occurrence of every needle
is replaced with `[REDACTED]` in both `content` and `arguments`
fields.

---

## Anonymization checklist

Before publishing a session (or corpus) as a dataset:

1. **Scan for credentials.**
   ```bash
   agx --scan-pii session.jsonl
   ```
   Covers AWS / Stripe / GitHub / OpenAI / Anthropic keys, JWTs,
   SSH private-key PEM headers, emails, IPv4 addresses. Per-session;
   wrap with `xargs` for corpus-wide.

2. **Redact every match.**
   ```bash
   agx --scan-pii session.jsonl \
       | awk '/eg:/ {for (i=NF; i>=1; i--) if ($i ~ /^[A-Za-z0-9_.@:-]+$/) {print $i; break}}' \
       | sort -u > /tmp/needles.txt
   # Review /tmp/needles.txt, then build the --redact invocation:
   REDACT_ARGS=$(awk '{printf "--redact %s ", $0}' /tmp/needles.txt)
   agx --export json $REDACT_ARGS session.jsonl > session.redacted.json
   ```

3. **Re-scan to confirm.**
   ```bash
   agx --scan-pii session.redacted.json   # should print "no matches"
   ```
   (If `--scan-pii` finds new matches, your `--redact` list missed
   something. Iterate.)

4. **Check the export for stray internal state.** Open
   `session.redacted.json` in your browser / editor and grep for:
   - Your username / hostname / home-dir path
   - Internal repo names, tickets, internal URLs
   - Environment-specific paths (`~/work/`, `/Users/<you>/`, etc.)

5. **For corpus releases**, also:
   ```bash
   agx corpus --trajectory-stats <dir>
   ```
   Check that the session count / token distribution matches what
   you advertise — a stray sensitive session in the corpus is worse
   than a labeling mistake.

6. **Strip agx annotations if they're private.** Annotations live
   under `~/.agx/notes/` keyed by session path and are included in
   every `--export` format when present. To exclude them from the
   release, either delete the notes file before export or use a
   fresh `AGX_HOME=/tmp/clean-notes` so no notes attach.

7. **Verify the license of every tool output you're redistributing.**
   Agent session traces can contain third-party API responses (web
   fetches, web search results). Those aren't yours to publish by
   default.

---

## Adapter recipes

These are minimal starters. The goal is to show the shape — adapt
to your harness's actual API.

### inspect-ai

[inspect-ai](https://github.com/UKGovernmentBEIS/inspect_ai) — use
agx to prepare the per-session transcripts its solver-chain expects.

```python
import json
import subprocess
from pathlib import Path

def load_session(path: Path) -> dict:
    """Run agx --export json and return the parsed dict."""
    result = subprocess.run(
        ["agx", "--export", "json", "--no-cost", str(path)],
        capture_output=True, text=True, check=True,
    )
    return json.loads(result.stdout)

def session_to_inspect_sample(session: dict) -> dict:
    """Map an agx session to an inspect-ai Sample."""
    user_steps = [s for s in session["steps"] if s["kind"] == "user_text"]
    input_text = user_steps[0]["detail"] if user_steps else ""
    return {
        "input": input_text,
        "target": None,  # whatever ground-truth your eval wants
        "metadata": {
            "models": session["totals"]["unique_models"],
            "tokens_in": session["totals"]["tokens_in"],
            "tokens_out": session["totals"]["tokens_out"],
            "cost_usd": session["totals"].get("cost_usd"),
        },
    }

# Usage
samples = [session_to_inspect_sample(load_session(p))
           for p in Path("~/.claude/projects").expanduser().glob("**/*.jsonl")]
```

### lm-evaluation-harness

[lm-eval-harness](https://github.com/EleutherAI/lm-evaluation-harness)
expects doc-level jsonl with task-specific fields. Use agx's
trajectory-openai export as the backbone, strip any tool-call
branches the task doesn't care about:

```bash
# Export every session under a corpus dir, one line per session.
find sessions/ -name '*.jsonl' | while read f; do
  agx --export trajectory-openai --redact "$USER" "$f"
done > corpus.jsonl

# Then filter to just the text turns for a chat eval:
jq -c '{messages: [.messages[] | select(.content != null and .role != "tool")]}' \
   corpus.jsonl > corpus.chat.jsonl
```

### Custom Python pipeline (training-data prep)

```python
import json
import subprocess
from pathlib import Path

def corpus_trajectories(dir: Path, redact: list[str] = None) -> iter[dict]:
    """Yield one OpenAI-shape trajectory dict per session file."""
    redact = redact or []
    args = ["agx", "--export", "trajectory-openai"]
    for pat in redact:
        args += ["--redact", pat]
    for path in dir.glob("**/*.jsonl"):
        result = subprocess.run(
            args + [str(path)],
            capture_output=True, text=True,
        )
        if result.returncode != 0:
            continue  # skip non-sessions silently
        yield json.loads(result.stdout.strip())

# Example: filter to sessions with at least one tool call, drop the rest.
tool_using = [
    t for t in corpus_trajectories(Path("sessions/"), redact=["MY_USER"])
    if any(m.get("tool_calls") for m in t["messages"])
]
print(f"{len(tool_using)} tool-using sessions")
```

### Corpus-level stats (eval harness setup validation)

Before running an expensive eval pass, confirm your corpus shape:

```bash
# Get the distribution — are these sessions in the length range your
# eval harness expects?
agx corpus --trajectory-stats sessions/

# Machine-readable for scripting:
agx corpus --trajectory-stats --json sessions/ | jq '.steps_per_session.p90'

# Spot-check the 20 most-recent sessions before a full run:
agx corpus --sample 20 --tui sessions/
```

---

## Reporting schema drift

If an agx update breaks your adapter, that's a schema regression —
file an issue with:

1. The agx version that worked (`agx --version`).
2. The agx version that broke.
3. A reduced session fixture that reproduces it (synthetic, not a
   real trace — see `assets/sample_*` for the shape).
4. Which field changed / disappeared / got renamed.

The maintainer treats schema breaks as release blockers.
