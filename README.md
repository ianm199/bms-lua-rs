# bevy_mod_scripting + lua-rs — Lua scripting in a Bevy **wasm** build, no C

A pure-Rust Lua backend (`lua-rs`) for [`bevy_mod_scripting`](https://github.com/makspll/bevy_mod_scripting)
(bms), so a Bevy game can have **Lua scripting that compiles and runs on
`wasm32-unknown-unknown`** — which the stock mlua (C Lua) backend cannot do
([bms #166](https://github.com/makspll/bevy_mod_scripting/issues/166)).

**Status: working browser demo.** `web-demo/` runs a playable **Snake game** in the
browser, driven entirely by the pure-Rust lua-rs VM. The whole game — state, the
food-seeking autopilot, collision, growth, scoring — lives in `assets/script.lua`;
Bevy only feeds input (`on_update(dt, left, right, up, down)`) and renders the grid the
script draws via `clear`/`rect` host functions. It plays itself and a human can take
over with the arrow keys / WASD. No `liblua`, no Emscripten, no C toolchain anywhere.

It is **additive**: lua-rs is a sibling backend alongside mlua/rhai/rune. mlua is
untouched — this is the backend you reach for where mlua can't go (the web).

## Why this exists

- mlua needs `wasm32-unknown-emscripten` (C Lua needs libc/`setjmp`); that target's
  output **can't link into the wasm-bindgen/wgpu module** Bevy emits for the web. So
  with mlua, Lua scripting simply can't enter a Bevy web build.
- piccolo (the other pure-Rust Lua) is a deliberate subset — no comprehensive
  userdata/metamethods, partial stdlib — unfit for bms's reflection bridge.
- lua-rs is a *conformant* pure-Rust Lua (passes the upstream 5.4 suite), so it's the
  one pure-Rust VM that can actually back bms.

## The recipe (everything needed to make it work)

1. **The backend crate** `bevy_mod_scripting_lua_rs/` — implements bms's
   `IntoScriptPluginParams` against lua-rs (context = lua-rs runtime with
   `unsafe impl Send`; `Runtime = ()`; loader/reloader/handler).
2. **bms `std::time::Instant` → `bevy_platform::time::Instant`** in `machines.rs`
   (the pipeline's per-frame budget loop). `std::time::Instant::now()` panics on
   `wasm32-unknown-unknown`. Submitted upstream as
   [PR #543](https://github.com/makspll/bevy_mod_scripting/pull/543); vendored locally
   in `vendor/bevy_mod_scripting_core/` and injected via `[patch.crates-io]`.
3. **`AssetPlugin { meta_check: AssetMetaCheck::Never }`** — on wasm the dev server
   returns non-meta content for the `.meta` probe, which otherwise fails the script load.
4. **A type-checking handler** — skip undefined callbacks cleanly (don't `pcall` a
   non-function), pop the error on `pcall` failure (no stack leak).
5. **A larger wasm stack** — `web-demo/.cargo/config.toml`:
   `rustflags = ["-C", "link-arg=-zstack-size=16777216"]` (16 MB). **This was the final
   blocker** (see below).

## Build & run the demo

```bash
cd web-demo
trunk serve --release --port 8091   # open http://127.0.0.1:8091/
```
Requires `rustup target add wasm32-unknown-unknown` and `cargo install trunk`. No
emscripten, no C compiler.

## The bug that took the longest: a wasm stack overflow

The demo compiled and booted but **froze** the instant the Lua callback ran — no error,
no panic, just a dead frame. The chase (and the method, which is reusable):

- **Minimal repro** (`/tmp/lua-wasm-repro`, Bevy-free, step-logged to the DOM): lua-rs
  core — create → register host fn → move into Send storage → exec → `pcall` → host-fn
  call — **all fine in wasm.**
- **Rhai control** (`rhai-control/`, bms + Rhai, no lua-rs): **responsive** in wasm →
  bms's pipeline runs in wasm.
- **Log-free moving-box freeze detector** in the demo (a `bevy_log` heartbeat is
  unreliable — it can be masked by the very hang): **empty** `on_update` → box sweeps
  (works); `on_update` calling **one host fn** → box frozen.

That bisected it to: *a Lua script calling a host cclosure from `pcall`, only when nested
inside bms's deep machine stack, under wasm.* **Root cause: stack overflow.** lua-rs's
`pcall`→cclosure path is stack-hungry; wasm's default stack is ~1 MB; bms's
schedule→system→machine→polled-future→handler nesting eats a big base before `pcall`
starts. The overflow happened *inside* bms's polled future, so it surfaced as a silent
freeze, not a clean trap. **Fix: bump the wasm stack (step 5).** Matches the known
"Lua-on-wasm needs a big stack via RUSTFLAGS" issue.

Every prior data point then made sense: native works (8 MB stack), standalone lua-rs
works (shallow stack), bms+Rhai works (Rhai's path is less stack-hungry), empty
`on_update` works (no cclosure call), one host-fn call freezes (tips it over 1 MB).

## Layout

```
bms-lua-rs/
  DEEP_PLAN.md                      # the original staged plan
  README.md                         # this file
  bevy_mod_scripting_lua_rs/        # the lua-rs bms backend crate
  web-demo/                         # the working browser demo (+ .cargo stack config)
  rhai-control/                     # bms+Rhai control (proved the pipeline runs in wasm)
  gate0-rhai-wasm/                  # earliest: proves bms+rhai compiles to wasm
  vendor/bevy_mod_scripting_core/   # bms core 0.19.0 + the std::time wasm fix (PR #543)
```

## Status & limitations (honest)

- **Demo-grade, not production.** It runs Lua callbacks and host functions; the full
  **Bevy-reflection userdata bridge** (`entity.component.field` access) is not yet
  implemented — scripts interact via registered host functions only.
- **Depends on a patched bms core** (the `std::time` fix). A clean crates.io release is
  gated on that landing upstream (or shipping against the fork).
- **Needs the 16 MB wasm stack** workaround. The cleaner long-term fix is to make
  lua-rs's `pcall` less stack-hungry / iterative so it fits the default — a lua-rs-lane
  optimization, now well-motivated.
- The bms maintainer is **not currently accepting new language backends** (#166), so
  this lives as a **fork/demo**, not an upstream backend — for now.
