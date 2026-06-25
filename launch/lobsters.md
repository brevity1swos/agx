# Lobste.rs Submission

Requires an invite from an existing member. Lobste.rs is parsing/data-model
literate and allergic to hype — lead with the engineering, not "AI."

## Tags

`rust`, `ai` (and `debugging` if available)

## Title

agx: a terminal step-through debugger for AI agent session files (Rust)

## URL

https://github.com/brevity1swos/agx

## Authored-by

Check the "authored by me" box only if comfortable; otherwise submit as a link.

## Suggested first comment (the engineering angle)

> Author here. The interesting part isn't the TUI, it's the normalization: every
> agent CLI writes a different trace shape and pairs tool calls to results
> differently — Claude Code uses a `tool_use_id` two-pass map, Codex uses
> `call_id` and batches calls before outputs, Gemini nests the result inside the
> same `toolCall` object, OpenTelemetry GenAI splits it across spans. agx
> auto-detects the format from the first line / wrapper shape and collapses all
> 8 into one `Step` timeline so the rendering and cost-aggregation code never
> sees format-specific concerns. The parsers deliberately *aren't* unified behind
> a shared trait — each keeps its own deserialize types and just emits `Vec<Step>`.
>
> Pure Rust, one detail view shows a tool call's input and result together, and
> there's an MCP server that exposes the same introspection to the agent itself.
> Happy to talk about the parser design or the MCP angle.
