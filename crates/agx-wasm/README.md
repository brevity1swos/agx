# agx-wasm

WebAssembly / TypeScript bindings for [agx-core](../agx-core/README.md).
Drive agx's agent-trace parsers from browsers, Node, Deno, or any
wasm-bindgen host — no native install, no Python, no terminal.

## Build

```sh
cargo install wasm-pack
cd crates/agx-wasm

wasm-pack build --target web        # for <script type="module">
wasm-pack build --target nodejs     # for `require("./pkg")` / `import` in Node
wasm-pack build --target bundler    # for webpack / vite / rollup / esbuild
```

Each target produces a `pkg/` with TypeScript declarations.

## JS surface

```js
import init, { load, scan_pii, version } from "agx-wasm";

await init();  // one-time panic-hook install

// Read a session file however the host allows.
const bytes = await fetch("session.jsonl").then(r => r.arrayBuffer());
const steps = load("session.jsonl", new Uint8Array(bytes));
for (const step of steps) {
  console.log(step.kind, step.label);
}

// Arbitrary-text PII scan.
for (const m of scan_pii("my api key is sk-abc...")) {
  console.log(m.category, m.snippet);
}

console.log(version());
```

## Schema

Every Step object mirrors the stable JSON schema documented in
[docs/eval-integration.md](https://github.com/brevity1swos/agx/blob/main/docs/eval-integration.md)
— same field names the CLI's `--export json` emits, same shape the
Python `agx.load` returns. Contract is versioned across the three
surfaces.

## Why bytes, not paths?

WASM doesn't get a filesystem by default. The JS side owns I/O
(File / Blob / fetch / fs.readFileSync / Deno.readFile) and passes
raw bytes in. Format detection + parsing happen in the Rust wasm.

## Distribution

npm publish is the eventual target (same as Python → PyPI). The
Phase 7.4 CI matrix commit will wire `wasm-pack publish` into release
automation.

## License

Dual-licensed under MIT OR Apache-2.0.
