# bevy_mod_scripting + lua-rs: Lua scripting in a Bevy wasm build

A pure-Rust Lua backend (`lua-rs`) for [`bevy_mod_scripting`](https://github.com/makspll/bevy_mod_scripting)
(bms). It lets a Bevy game use Lua scripting on `wasm32-unknown-unknown`, which the default
mlua (C Lua) backend can't do ([bms #166](https://github.com/makspll/bevy_mod_scripting/issues/166)).

Live demo: https://ianm199.github.io/bms-lua-rs/

The demo runs Conway's Game of Life in the browser, scripted in Lua. The script is bms's
`game_of_life.lua` example, used as-is: it reads and writes the `LifeState` component through
the reflection bridge (`world.get_type_by_name`, `world.query`, the component's `cells`), and
a Bevy system renders the grid. You can edit the Lua on the page and it hot-reloads, and the
script can declare its own sliders with `param(name, default, min, max)` to change the rules
while it runs.

lua-rs is a separate backend that sits next to mlua/rhai/rune. mlua isn't changed; this is the
option for the web, where mlua doesn't compile.

## Why

mlua needs `wasm32-unknown-emscripten` (C Lua wants libc and `setjmp`), and that target's
output doesn't link into the wasm-bindgen/wgpu module Bevy produces for the web. So mlua can't
be used for Lua scripting in a Bevy web build at all.

piccolo, the other pure-Rust Lua, is a deliberate subset (limited userdata/metamethods,
partial stdlib), so it doesn't have what bms's reflection bridge needs. lua-rs aims to be a
full Lua 5.4 (it passes the upstream test suite), which is why it can back bms.

## What it takes

1. The backend crate `bevy_mod_scripting_lua_rs/` implements bms's `IntoScriptPluginParams`
   against lua-rs (context is the lua-rs runtime with `unsafe impl Send`, `Runtime = ()`, plus
   loader/reloader/handler).
2. The reflection bridge (`src/reflection.rs`) maps bms's `ReflectReference` / `WorldGuard`
   onto lua-rs userdata with `__index`/`__newindex`/`__len`/`__pairs`, plus a `world` global.
   That is what lets the unmodified `game_of_life.lua` read and write Bevy components. It uses
   lua-rs's userdata and metamethod API, including `__pairs`, which landed in lua-rs 0.0.7.
3. lua-rs from crates.io (`lua-rs-runtime`, `lua-vm`, `lua-types` at 0.0.7).
4. A change to bms core: `std::time::Instant` to `bevy_platform::time::Instant` in the
   per-frame budget loop, because `Instant::now()` panics on `wasm32-unknown-unknown`. Sent
   upstream as [PR #543](https://github.com/makspll/bevy_mod_scripting/pull/543); until it
   lands it is vendored in `vendor/bevy_mod_scripting_core/` and patched in via
   `[patch.crates-io]`.
5. `AssetPlugin { meta_check: AssetMetaCheck::Never }`, because on wasm the dev server's
   response to the `.meta` probe otherwise fails the script load.
6. A bigger wasm stack in `web-demo/.cargo/config.toml`
   (`rustflags = ["-C", "link-arg=-zstack-size=16777216"]`, 16 MB). See below for why.

## Build & run

```bash
cd web-demo
trunk serve --release --port 8091   # open http://127.0.0.1:8091/
```

Needs `rustup target add wasm32-unknown-unknown` and `cargo install trunk`. No emscripten or C
compiler. The harnesses under `demo/` run the same scripts headlessly
(`cargo run --bin game_of_life_test`, `param_test`, `reload_test`).

## The wasm stack overflow

The demo compiled and started, then froze the moment the Lua callback ran, with no error or
panic. What it took to find:

- A Bevy-free repro of the lua-rs core (create, register a host fn, move into Send storage,
  exec, pcall, call the host fn) ran fine on wasm.
- A Rhai version of the same demo (bms + Rhai, no lua-rs) stayed responsive on wasm, so the
  bms pipeline itself runs on wasm.
- A moving box in the demo as a freeze indicator (a `bevy_log` heartbeat is unreliable, since
  the hang can swallow it): an empty `on_update` kept the box moving; an `on_update` that
  called one host function froze it.

That narrowed it to a Lua script calling a host cclosure from pcall, but only inside bms's deep
call stack, on wasm. It was a stack overflow. lua-rs's pcall-to-cclosure path uses a lot of
stack, wasm defaults to about 1 MB, and bms's schedule/system/machine/polled-future/handler
nesting uses a lot of that before pcall even starts. The overflow happened inside a polled
future, so it showed up as a silent freeze instead of a clean trap. Bumping the wasm stack
(step 6) fixed it.

The earlier results line up: native works (8 MB stack), standalone lua-rs works (shallow
stack), bms + Rhai works (Rhai uses less stack), empty `on_update` works, one host call tips it
over 1 MB.

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
  rhai-control/                      # bms+Rhai control (showed the pipeline runs in wasm)
  gate0-rhai-wasm/                   # earliest: bms+rhai compiling to wasm
  vendor/bevy_mod_scripting_core/    # bms core 0.19.0 + the std::time wasm fix (PR #543)
```

The page loads `tweakable.lua` because the sliders make it interactive. That file is
`game_of_life.lua` with `param(...)` calls added; the unmodified original is in the repo and
runs the same way (`demo/src/bin/game_of_life_test.rs` runs it).

## Status and limits

- This is a demo, not production. The reflection bridge covers component field read/write,
  queries, resource access, and iteration, which is enough to run the Game of Life example
  as-is. It is not a full binding of every bms reflection surface.
- It depends on a patched bms core (the `std::time` fix). A fully clean crates.io build waits
  on [PR #543](https://github.com/makspll/bevy_mod_scripting/pull/543) landing (or building
  against the fork). The lua-rs VM itself is on crates.io.
- It needs the 16 MB wasm stack. The better long-term fix is to make lua-rs's pcall use less
  stack so it fits the default.
- The bms maintainer is not taking new language backends right now (#166), so this is a
  fork/demo rather than an upstream backend.
