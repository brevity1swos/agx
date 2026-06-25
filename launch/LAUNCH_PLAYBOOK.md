# agx Launch Playbook

Step-by-step guidance for promoting agx — a terminal step-through debugger for
AI agent session files. agx's framing is **a terminal dev tool that reads files
your agent CLI already writes** (zero instrumentation). Lead with that, not with
"AI" — it sidesteps the AI-content policies that close some channels, and it's
the honest hook anyway.

---

## Status (Updated 2026-06-24)

| Channel | Status | Notes |
|---------|--------|-------|
| crates.io | **Published** | `agx-tui` v0.2.1 (binary `agx`); also `agx-core`, `agx-mcp` |
| GitHub release | **Cut** | `agx-tui-v0.2.1` + per-crate releases, demo GIF |
| CI + release-plz | **Live & green** | 3-OS test matrix + clippy + fmt + docs; releases automated |
| awesome-ratatui | **DONE** | PR #324 **merged 2026-06-23** — agx is listed |
| Show HN | **Draft ready** | `show_hn.md` — post manually (US weekday AM ET) |
| Lobste.rs | **Draft ready** | `lobsters.md` — needs an invite; tags `rust` + `ai` |
| Terminal Trove | **Draft ready** | `terminal_trove.md` — submit via terminaltrove.com/submit |
| MCP registries | **Draft ready** | `mcp_directories.md` — agx-only channel (`agx-mcp` is a real MCP server) |
| Twitter / X | **Draft ready** | `twitter.md` |
| awesome-rust | **Deferred** | bar is >50★ OR >2000 dl; agx is below both — revisit after a spike |
| r/rust | **Closed** | AI-generated-projects policy — do not attempt |
| r/commandline | **Closed** | AI disclosure rules — do not attempt |

**Current metrics (2026-06-24):** 11 stars · `agx-tui` 39 dl / `agx-core` 95 / `agx-mcp` 51 · v0.2.1.

---

## Pre-launch checklist (do these first)

1. **Generate `assets/preview.png`** — Terminal Trove wants a still image; agx
   only ships `assets/demo.gif`. Crop one clean frame (a `[result]` step showing
   the paired tool-call input + response — that's the differentiator) the same
   way rxray's `preview.png` was made. Commit under `chore:`.
2. **Set the repo homepage** to `https://docs.rs/agx-core` (currently unset) —
   `gh repo edit brevity1swos/agx --homepage https://docs.rs/agx-core`.
3. **Verify asset URLs return 200** before any submission:
   `https://raw.githubusercontent.com/brevity1swos/agx/main/assets/demo.gif`.

> Launch-doc commits use the `chore:` prefix so they stay out of the release-plz
> changelog (see the launch-commit-prefix convention).

---

## Immediate Next Actions (priority order)

### 1. Submit to MCP registries — *the agx-only channel, do this first*
Use `mcp_directories.md`. `agx-mcp` is a genuine MCP server (agent
self-introspection: session summary, recent errors, tool distribution, PII
scan). The MCP-server lists (`punkpeye/awesome-mcp-servers`,
`wong2/awesome-mcp-servers`, modelcontextprotocol registry) are warm, on-topic,
and uncontested — no other terminal debugger sits there. Highest signal-to-effort.

### 2. Submit to Terminal Trove
Use `terminal_trove.md`. Curated form, no hard star bar; rgx is already listed,
so the channel is warm. Needs the `preview.png` from the checklist.

### 3. Post Show HN
Use `show_hn.md`. Hook = zero-instrumentation cross-CLI parsing + the
agent-self-introspection (MCP) angle. Be ready for "how is this different from
Langfuse/LangSmith?" (those are hosted team observability; agx is local,
terminal, zero-setup — different job).

### 4. Lobste.rs (if you have an invite)
Use `lobsters.md`, tags `rust` + `ai`. Lead with the parsing/data-model angle,
not the AI hype.

### 5. Twitter / X
Use `twitter.md` once the GIF renders well in-feed.

### 6. awesome-rust — WAIT for the bar
Do **not** submit until agx clears **>50 stars OR >2000 downloads** (the list's
CONTRIBUTING bar). Revisit if a Show HN / MCP-list spike pushes it over.

---

## Positioning

agx's niche is **local, zero-instrumentation, multi-CLI**: it reads the
JSONL/JSON your agent CLI already writes (Claude Code, Codex, Gemini,
OpenAI-generic, LangChain, Vercel AI SDK, OTel GenAI JSON, OTel binary) and gives
a vim-bound, navigable timeline where one keypress shows a tool call's input and
its result on the same screen. Three real differentiators:

1. **Zero instrumentation** — no SDK changes, no hosted dashboard, no telemetry.
2. **One detail view pairs `[tool]` input ↔ `[result]` output** across 8 formats.
3. **`agx-mcp`** — the agent can introspect its *own* session mid-run (detect
   tool-call loops, escalating errors, token budget). Novel; no direct competitor.

**Honest framing beats overclaiming.** agx is *not* a Langfuse/LangSmith/Helicone
competitor — those are team observability with retention and alerting; agx is the
thing you reach for when you're already in the terminal. Say so. The durable moat
is cross-CLI breadth; the standing risk is a first-party trace viewer shipping in
Claude Code — lead with breadth + the MCP self-introspection that a vendor viewer
won't replicate.

---

## Monitoring

```bash
# Stars
gh api repos/brevity1swos/agx --jq '.stargazers_count'

# crates.io downloads (all three crates)
for c in agx-tui agx-core agx-mcp; do
  curl -s https://crates.io/api/v1/crates/$c | jq -c "{($c): .crate | {downloads, recent_downloads}}"
done

# Traffic referrers (auth)
gh api repos/brevity1swos/agx/traffic/popular/referrers

# Open PRs and issues
gh pr list --repo brevity1swos/agx
gh issue list --repo brevity1swos/agx
```

**Decision rule (from the original launch plan):** flat signal after a promotion
wave → back to maintenance mode; a spike that clears 50★ → open the awesome-rust PR.
