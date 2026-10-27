# Changelog

All notable changes to this project will be documented in this file.

## [0.2.2] - 2026-10-27

### Bug Fixes

- *(mcp)* Recover from poisoned step cache mutex instead of panicking
A panic while holding the cache lock poisoned it, after which every
  tool call crashed the server via expect(). Recover the inner value at
  both lock sites.
- *(mcp)* Reply to null-id requests and reject malformed ones
serde collapsed an explicit "id": null into None, so such requests
  were treated as notifications and never answered. Inspect the raw
  object instead: only an absent id is a notification. Valid JSON that
  is not a well-formed request (non-object, missing method, wrong
  jsonrpc version, bad id type) now gets -32600 instead of -32700.
- *(mcp)* Answer ping with an empty result
MCP requires servers to respond to ping with {}; we returned -32601,
  which hosts can read as a dead server.
- *(mcp)* Follow mcp error shape for tool calls
Unknown tool names and invalid arguments are now -32602 and are
  checked before the session file is loaded. Failures while running a
  valid call are returned as a successful result with isError: true
  and the message as text content, instead of a -32603 RPC error the
  model never sees.

### Build

- Raise msrv to 1.88
The declared 1.85 was already false: ratatui 0.30 needs 1.86, and darling, time and ort (embedding-search) need 1.88. Users on older toolchains got dependency build errors instead of a clear rust-version message. The workspace checks clean on 1.88 with every feature.


## [0.2.1] - 2026-06-19

### Refactoring

- *(mcp)* Use const-initialized Mutex for the step cache

