# bevy_mod_scripting + lua-rs: Lua scripting in a Bevy **wasm** build, no C

A pure-Rust Lua backend (`lua-rs`) for [`bevy_mod_scripting`](https://github.com/makspll/bevy_mod_scripting)
(bms), so a Bevy game can have **Lua scripting that compiles and runs on
`wasm32-unknown-unknown`**, which the stock mlua (C Lua) backend cannot do
([bms #166](https://github.com/makspll/bevy_mod_scripting/issues/166)).

**Live demo: https://ianm199.github.io/bms-lua-rs/**

It runs **Conway's Game of Life, scripted in Lua, in the browser**. The script is bms's
own `game_of_life.lua` example: it reaches into the Bevy `World` through the reflection
bridge (`world.get_type_by_name`, `world.query`, reading and writing the `LifeState`
component's `cells`), and a Bevy system renders the grid. No `liblua`, no Emscripten, no
C toolchain anywhere. The page also lets you **edit the Lua live** and **declare your own
sliders** from inside the script via `param(name, default, min, max)`, so you can change
the birth/survival rules as it runs.

It is **additive**: lua-rs is a sibling backend alongside mlua/rhai/rune. mlua is
untouched. This is the backend you reach for where mlua cannot go (the web).

## Why this exists

- mlua needs `wasm32-unknown-emscripten` (C Lua needs libc/`setjmp`); that target's
  output **cannot link into the wasm-bindgen/wgpu module** Bevy emits for the web. So
  with mlua, Lua scripting simply cannot enter a Bevy web build.
- piccolo (the other pure-Rust Lua) is a deliberate subset: no comprehensive
  userdata/metamethods, partial stdlib, unfit for bms's reflection bridge.
- lua-rs is a *conformant* pure-Rust Lua (passes the upstream 5.4 suite), so it is the
  one pure-Rust VM that can actually back bms's reflection bridge.

## What makes it work

1. **The backend crate** `bevy_mod_scripting_lua_rs/` implements bms's
   `IntoScriptPluginParams` against lua-rs (context = lua-rs runtime with
   `unsafe impl Send`; `Runtime = ()`; loader/reloader/handler).
2. **The reflection bridge** (`bevy_mod_scripting_lua_rs/src/reflection.rs`) maps bms's
   `ReflectReference` / `WorldGuard` onto lua-rs userdata with `__index` / `__newindex`
   / `__len` / `__pairs` metamethods, plus a `world` global. That is what lets the
   unmodified `game_of_life.lua` read and mutate Bevy components from Lua. It depends on
   lua-rs's userdata + metamethod API, including `__pairs`, which was added upstream in
   lua-rs 0.0.7.
3. **lua-rs from crates.io** (`lua-rs-runtime`, `lua-vm`, `lua-types` at `0.0.7`). No
   vendored copy of the VM.
4. **bms `std::time::Instant` to `bevy_platform::time::Instant`** in the pipeline's
   per-frame budget loop. `std::time::Instant::now()` panics on
   `wasm32-unknown-unknown`. Submitted upstream as
   [PR #543](https://github.com/makspll/bevy_mod_scripting/pull/543); vendored locally in
   `vendor/bevy_mod_scripting_core/` and injected via `[patch.crates-io]` until it lands.
5. **`AssetPlugin { meta_check: AssetMetaCheck::Never }`**: on wasm the dev server
   returns non-meta content for the `.meta` probe, which otherwise fails the script load.
6. **A larger wasm stack** in `web-demo/.cargo/config.toml`:
   `rustflags = ["-C", "link-arg=-zstack-size=16777216"]` (16 MB). This was the final
   blocker (see below).

## Build & run the demo

```bash
cd web-demo
trunk serve --release --port 8091   # open http://127.0.0.1:8091/
```

Requires `rustup target add wasm32-unknown-unknown` and `cargo install trunk`. No
emscripten, no C compiler. The native test harnesses under `demo/` run the same scripts
headlessly (`cargo run --bin game_of_life_test`, `param_test`, `reload_test`).

## The bug that took the longest: a wasm stack overflow

The demo compiled and booted but **froze** the instant the Lua callback ran. No error, no
panic, just a dead frame. The chase, and the method, which is reusable:

- **Minimal repro** (Bevy-free, step-logged to the DOM): lua-rs core, create, register
  host fn, move into Send storage, exec, `pcall`, host-fn call, all fine in wasm.
- **Rhai control** (`rhai-control/`, bms + Rhai, no lua-rs): responsive in wasm, so bms's
  pipeline runs in wasm.
- **Log-free moving-box freeze detector** in the demo (a `bevy_log` heartbeat is
  unreliable, it can be masked by the very hang): empty `on_update` sweeps the box
  (works); `on_update` calling one host fn freezes the box.

That bisected it to: a Lua script calling a host cclosure from `pcall`, only when nested
inside bms's deep machine stack, under wasm. **Root cause: stack overflow.** lua-rs's
`pcall` to cclosure path is stack-hungry; wasm's default stack is about 1 MB; bms's
schedule to system to machine to polled-future to handler nesting eats a big base before
`pcall` starts. The overflow happened inside bms's polled future, so it surfaced as a
silent freeze, not a clean trap. **Fix: bump the wasm stack (step 6).**

Every prior data point then made sense: native works (8 MB stack), standalone lua-rs
works (shallow stack), bms+Rhai works (Rhai's path is less stack-hungry), empty
`on_update` works (no cclosure call), one host-fn call freezes (tips it over 1 MB).

## Layout

```
bms-lua-rs/
  README.md                          # this file
  DEEP_PLAN.md                       # the original staged plan
  bevy_mod_scripting_lua_rs/         # the lua-rs bms backend crate
    src/lib.rs                       #   plugin, loader/handler, param() host fn
    src/reflection.rs                #   the Bevy-reflection userdata bridge
  web-demo/                          # the browser demo (editor + sliders + .cargo stack config)
    assets/game_of_life.lua          #   bms's example, byte-for-byte unmodified
    assets/tweakable.lua             #   same, plus param() sliders (what the page loads)
  demo/                              # native headless harnesses for the same scripts
  rhai-control/                      # bms+Rhai control (proved the pipeline runs in wasm)
  gate0-rhai-wasm/                   # earliest: proves bms+rhai compiles to wasm
  vendor/bevy_mod_scripting_core/    # bms core 0.19.0 + the std::time wasm fix (PR #543)
```

The live page loads `tweakable.lua` because the sliders make it interactive. That file is
`game_of_life.lua` with `param(...)` calls added; the byte-for-byte original is in the
repo and runs identically (it is what `demo/src/bin/game_of_life_test.rs` exercises).

## Status & limitations (honest)

- **Demo-grade, not production.** The reflection bridge covers component field read/write,
  queries, resource access, and iteration, enough to run the Game of Life example
  unmodified. It is not a complete binding of every bms reflection surface.
- **Depends on a patched bms core** (the `std::time` fix). A fully clean crates.io build
  is gated on [PR #543](https://github.com/makspll/bevy_mod_scripting/pull/543) landing
  upstream (or shipping against the fork). The lua-rs VM itself is on crates.io.
- **Needs the 16 MB wasm stack** workaround. The cleaner long-term fix is to make lua-rs's
  `pcall` less stack-hungry / iterative so it fits the default stack.
- The bms maintainer is **not currently accepting new language backends** (#166), so this
  lives as a **fork/demo**, not an upstream backend, for now.
