---
name: Format drift
about: Report a session file that agx misparses or no longer handles correctly
title: "[format drift] <CLI name> <version>: <one-line summary>"
labels: format-drift
---

## CLI and version

- **CLI:** Claude Code / Codex / Gemini / Aider / Cline / other
- **Version:** (output of `<cli> --version`)
- **agx version:** (output of `agx --version`)
- **OS:** macOS / Linux / Windows + version

## What's wrong

Describe what agx does vs. what you expected. Examples:

- Crashes with a serde error
- Silently drops messages / tool calls
- Tool input/output not paired correctly
- Wrong step kind (e.g. user shown as assistant)

## Sample (anonymized)

Paste the **first 10-20 lines** of an affected session file. Please
anonymize before pasting:

- Replace absolute paths with `/tmp/<placeholder>`
- Strip API keys, tokens, email addresses
- Replace usernames with `user`
- Inspect line-by-line; `grep` is not enough

```
<paste here>
```

## `agx --debug-unknowns` output

If your version of agx has the `--debug-unknowns` flag, paste the output
here — it tells us exactly what entry types or fields the parser didn't
recognize.

```
<paste here>
```

## Anything else

Optional: link to the CLI's release notes if you suspect this is from a
recent format change, your `~/.<cli>/` directory layout if it's
non-standard, etc.
