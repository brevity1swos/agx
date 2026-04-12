# Changelog

All notable changes to agx are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/).

## [0.1.0] - 2026-04-12

First release. A step-through debugger for AI agent execution traces — *just another gdb, but for your agent.*

### Format Support
- Claude Code session JSONL parser with graceful unknown-type handling
- Codex CLI session JSONL parser (response_item + function_call pairing)
- Gemini CLI single-JSON parser (atomic toolCall splitting)
- Generic conversation JSON parser (OpenAI/Anthropic SDK/Vercel AI SDK/LangChain)
- Auto-detection of session format by content sniffing (no flags needed)
- Multi-session browser — launch with no args to scan ~/.claude, ~/.codex, ~/.gemini

### Debugger Features
- Three-pane TUI layout: timeline / conversation view / detail (Tab toggles 2-pane)
- Bidirectional tool_use ↔ tool_result pairing with originating call input visible
- Alternating step colors for visual clarity between adjacent tool calls
- Batch/fork visualization — ║ markers for parallel tool dispatches
- Tool error detection — heuristic-based red highlighting across all formats
- Heatmap mode (h) — color-coded tool-call density with 5-level gradient
- Time-travel scrubbing bar with position indicator
- Latency annotations — per-step duration computed from timestamps

### Navigation & Workflow
- Jump to step (:N command mode)
- Filter by tool name / step kind (f) — case-insensitive substring match
- Content search with match highlighting (/) and n/N navigation with wrap
- Bookmarks (m\<char\> / '\<char\>) — survive filter cycles, report hidden-by-filter
- Mouse support — click-to-select on timeline, scroll wheel navigation
- Vim-style count prefixes (3j, 5k, 42G, 2d, 7n, ...)
- Clipboard copy (y) — copies current step detail to system clipboard

### Analysis
- Tool usage statistics overlay (s) — per-tool counts, error rates, sorted by frequency
- Session comparison (--diff) — cross-format text summary of tool usage and errors

### Modes
- Live attach (--live) — watches session file and auto-refreshes on changes
- Non-interactive --summary mode for scripts and CI
- Help overlay (? / F1) with keybinding reference and color legend
- Shell completions (--completions bash/zsh/fish)

### Quality
- 116 unit tests
- Clippy strict + pedantic clean (zero warnings)
- cargo audit clean
- Panic-safe terminal cleanup via TerminalGuard Drop impl
- Synthetic sample fixtures for all 4 formats (zero personal data)
