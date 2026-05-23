# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2026-05-23

### Miscellaneous

- Release ([#1](https://github.com/brevity1swos/agx/pull/1))
* chore: release

  * chore(release): bump workspace to 0.2.0 and restore hand-written CHANGELOG

  Two overrides to what release-plz proposed:

    1. Version: 0.1.0 → 0.2.0 across all 5 crates, not the 0.1.1
       semver-checks suggested. The bump reflects the substance of
       v0.2.0 — Phase 2 follow-on (LangChain, Vercel AI SDK, OTel
       GenAI JSON + binary protobuf), Phase 5 (fork detection,
       notifications, --jump-to, experimental shell replay with
       triple-gate safety and bounded resource use), Phase 6
       (trajectory-openai export, --redact, --scan-pii,
       --trajectory-stats / --sample), Phase 7 (workspace split,
       agx-py + agx-wasm scaffolds, formal stability + non_exhaustive
       enums), and the agx-mcp / agx doctor tooling tracks. Minor
       across the suite is the correct shape; semver-checks reported
       "API compatible changes" because the new public surfaces are
       additive rather than breaking.

    2. CHANGELOG: restore the hand-written v0.2.0 prose authored in
       `docs(changelog) aa3d26a`. Cliff prepended ~2000 lines
       covering every commit since the project started, because no
       v0.1.0 git tag exists for it to anchor against. The
       hand-written entry is denser and reads better; cliff takes
       over for v0.2.1 onward when there will be an anchor tag.

  Path-deps on agx-core also bumped to 0.2.0 so `cargo publish`
  resolves the registry version correctly.

