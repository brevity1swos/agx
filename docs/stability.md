# Stability commitments

This document defines what agx promises to hold stable, across which
versions, and what breaking changes look like. Lives at
[`docs/stability.md`](docs/stability.md) in the repo so it travels
with the code; referenced from README and `docs/eval-integration.md`.

## Scope

Four public surfaces are tracked for stability:

1. **agx CLI flags and subcommands** — `agx`, `agx corpus`, all
   long-form flags (`--export`, `--scan-pii`, `--jump-to`, etc.).
2. **agx export JSON schema** — the shape `agx --export json`
   produces, and its trajectory-openai variant.
3. **`agx-core` public Rust API** — every item under `pub` in
   `crates/agx-core/src/lib.rs` and its sub-modules.
4. **`agx` (Python) and `agx-wasm` (JS) surfaces** — the function
   signatures and returned object shapes.

## Versioning policy

agx follows [SemVer](https://semver.org). Version numbers are
`MAJOR.MINOR.PATCH`:

| Change kind                                          | Version bump |
|------------------------------------------------------|--------------|
| Bug fix, docs, non-breaking addition                 | PATCH        |
| New feature, new CLI flag, new field in JSON schema  | MINOR        |
| Removed / renamed CLI flag                           | MAJOR        |
| Removed / renamed JSON field, enum variant removal   | MAJOR        |
| `agx-core` trait / function signature change         | MAJOR        |
| Python / JS binding argument rename or return shape  | MAJOR        |

**Pre-1.0** (current): breaking changes land in MINOR bumps but are
still flagged in `CHANGELOG.md` under a **Breaking** section and in
the cross-tool compat table in README. From v1.0 onward, strict
SemVer applies.

## Cross-tool compatibility

When agx breaks a contract sift depends on — `agx --export json` shape,
`agx --jump-to`, `agx --version` format — the consumer tool (sift) has
to update. Signals:

1. agx CHANGELOG's **Breaking** section names the flag/field.
2. agx README's compat table bumps the minimum sift version row.
3. sift's `sift doctor` reports incompatibility with a suggested
   upgrade path.

Per [suite-conventions §6](suite-conventions.md), agx never reads
from sift or rgx. The compat contract is one-directional: agx is the
producer, sift is the consumer, changes flow downstream.

## JSON schema stability (the most-used contract)

`agx --export json` emits:

```json
{
  "totals": { ... },
  "steps": [ {step}, ... ],
  "annotations": [ ... ]  // optional
}
```

Within that:

- **Field names are stable.** Renaming `step.tokens_in` to
  `step.input_tokens` is a MAJOR break.
- **Field types are stable.** Changing `step.duration_ms` from
  `u64` to `string` is a MAJOR break.
- **Enum values are stable.** Renaming `"user_text"` to `"user"` in
  `step.kind` is a MAJOR break.
- **Field additions are a MINOR bump.** New optional fields with
  `#[serde(skip_serializing_if = "Option::is_none")]` don't break
  existing consumers — they see the same shape they knew about.
- **Field removals are a MAJOR bump.** Even if a field has always
  been `null`, dropping it is a break because downstream JSON
  parsers may rely on its presence.

For the full field reference, see
[`docs/eval-integration.md`](eval-integration.md).

## `agx-core` Rust API stability

The library crate is pre-1.0 until `cargo publish -p agx-core`
runs on crates.io. Once published:

- Every `pub` item in `agx-core` is subject to SemVer.
- Items marked `pub(crate)` or `pub(super)` are not public API.
- `#[doc(hidden)]` items are not public API — use at your own risk.
- The `#[non_exhaustive]` attribute guards enums / structs that are
  expected to gain variants (`Format`, `StepKind` — currently missing
  this; will be added before v1.0).

Library consumers pin like:

```toml
[dependencies]
agx-core = "0.1"   # accepts 0.1.x updates (patch-compatible)
```

In pre-1.0, MINOR bumps can be breaking — pin exact if you need
strict compat:

```toml
agx-core = "=0.1.0"
```

Post-1.0, `"1"` (caret) is safe for any same-major update.

## Feature flag stability

agx-core exposes three optional features: `otel-proto`,
`embedding-search`, `notifications`. Each feature:

- Cannot be removed without a MAJOR bump.
- Its transitive deps (prost, fastembed, notify-rust) are pinned to
  major versions in `Cargo.toml` and stay there — we eat the churn
  of pointing at patch updates, not consumers.
- Enabling a feature must not change the behavior of any non-feature
  code path (no leaked feature semantics in the default build).

## Python / WASM surface stability

The Python `agx` and WASM `agx-wasm` packages mirror the same shape
as `--export json`:

- `agx.load(path) -> list[dict]` — dict keys match JSON step fields.
- `agx.load_corpus(dir) -> list[dict]` — per-session dict keys match
  the `--jsonl` session-line shape.
- `agx.scan_pii(text) -> list[dict]` — `{category, step_index, snippet}`.

Adding a new top-level function is a MINOR bump. Renaming `agx.load`
to `agx.parse` is a MAJOR bump. Return-shape changes follow the
same rules as the JSON schema above — adding a key is MINOR,
removing or renaming is MAJOR.

## CI wheel / WASM matrix (Phase 7.4b)

Pre-requisite for actual PyPI / npm publish: GitHub Actions matrix
building wheels for Linux-x86_64, Linux-aarch64, macOS-arm64, and
Windows-x86_64 (Python); and for web / nodejs / bundler (WASM). Lives
in `.github/workflows/` and runs on every tag push.

That workflow is a **separate commit** from this doc — the shape of
the matrix and the release-automation trigger (e.g. release-plz or
manual tag) is its own design decision. Until it ships, `maturin
build --release` + `wasm-pack build --release` locally are the
supported paths.

## Deprecation policy

When removing a CLI flag or a public API item:

1. The deprecated item must remain functional for at least one
   MINOR release after the CHANGELOG notes the deprecation.
2. Emit a stderr warning on use when possible (CLI flags).
3. Only remove the item in the next MAJOR bump.

## Reporting breakage

Open a GitHub issue with:

- agx version that worked (`agx --version`).
- agx version that broke.
- A minimal reproduction (a synthetic session fixture in the shape
  of `assets/sample_*`, never a real trace).
- Which field / flag / function changed.

Schema breaks are treated as release blockers.
