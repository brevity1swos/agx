# tests/corpus

Anonymized real-world session fixtures, organized by format. The
integration test at `tests/corpus_test.rs` walks this directory and
asserts every file parses cleanly via `agx --summary`.

## Layout

```
tests/corpus/
├── claude_code/
│   └── <descriptive-name>.jsonl
├── codex/
│   └── <descriptive-name>.jsonl
├── gemini/
│   └── <descriptive-name>.json
└── generic/
    └── <descriptive-name>.json
```

## Contributing

See `CONTRIBUTING.md` at the repo root. Before submitting:

1. Anonymize paths, emails, API keys, usernames.
2. `agx --debug-unknowns --summary <your-file>` should succeed (drift
   reports are fine — that's the point).
3. Commit with a message describing what's distinctive about the fixture.

The directory can be empty — that's the v0.1 baseline, and the corpus
test no-ops gracefully when there are no files to scan.
