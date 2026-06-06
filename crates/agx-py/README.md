# agx-py

Python bindings for [agx-core](../agx-core/README.md) — load and
inspect AI agent traces (Claude Code, Codex, Gemini, Generic OpenAI,
LangChain / LangSmith, Vercel AI SDK, OpenTelemetry GenAI JSON) from
Python.

Use this when you want to drive agx's parsers from a training-data
pipeline, custom eval harness, or CI guard without shelling out to
the `agx` CLI. The Rust implementation is the same pure parsers that
back the agx terminal debugger — no TUI deps compiled in.

## Install

```sh
pip install agx
```

Wheels are built with [maturin](https://github.com/PyO3/maturin); the
abi3-py310 target means one wheel works on every Python ≥ 3.10.

## Quick start

```python
import agx

# Load a single session. Format auto-detected.
steps = agx.load("session.jsonl")
for step in steps:
    print(step["kind"], step["label"])
    # step keys: label, detail, kind, tool_name, timestamp_ms,
    # duration_ms, model, tokens_in, tokens_out, cache_read,
    # cache_create, is_fork_root, tool_call_id

# Scan a directory of sessions in parallel (rayon under the hood).
# Yields per-session `ParsedSession`-shaped dicts.
for session in agx.load_corpus("sessions/"):
    print(session["path"], session["step_count"], session["totals"]["tokens_in"])

# Scan arbitrary text for credentials / PII — mirrors `agx --scan-pii`.
for m in agx.scan_pii("api key is sk-abc…"):
    print(m["category"], m["snippet"])
```

## Schema

`steps[]` dict field names and shapes mirror `agx --export json`
exactly; the same stability contract applies. See
[docs/eval-integration.md](https://github.com/brevity1swos/agx/blob/main/docs/eval-integration.md)
for the full reference.

## Build from source

```sh
git clone https://github.com/brevity1swos/agx.git
cd agx/crates/agx-py
pip install maturin
maturin develop --release
python -c 'import agx; print(agx.__version__)'
```

## License

Dual-licensed under MIT OR Apache-2.0.
