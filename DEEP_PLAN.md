# Deep plan: a pure-Rust Lua (lua-rs) backend for bevy_mod_scripting → close #166

## Goal
Make Lua-scripted Bevy games run in the browser (`wasm32-unknown-unknown`) by giving
**bevy_mod_scripting (bms)** a Lua backend powered by **lua-rs** (pure-Rust Lua 5.4)
instead of mlua (C Lua, which cannot target that triple — bms issue #166).
Proof target: **nano9** ("Bevy in PICO-8 clothing", runs real PICO-8 carts), which
already wants web and is blocked solely by #166.

## Why this is novel (and why piccolo didn't do it)
- bms's reflection bridge needs **userdata + full metamethods** (`__index`/`__newindex`/
  arithmetic) and real scripts need **string lib / pattern matching**. Piccolo is a
  deliberate *subset* missing exactly these, with an alien gc-arena API → unfit for bms.
- lua-rs is a **complete** PUC port (44/44, full userdata/metatables/stdlib) → it *can*
  back bms where piccolo can't. That completeness is the differentiation.
- Nobody's done it because: web is low priority for bms (#166 `blocked`+`enhancement`),
  and the backend work is real against any non-mlua VM. We'd be first.

## Verified facts (recon)
- bms targets **Bevy 0.18**; bms version **0.19**.
- Backend contract is **modular**: implement `IntoScriptPluginParams` (assoc
  `C: Context (Send)`, `R: Runtime (Default+Send+Sync)`, + `handler`/`context_loader`/
  `context_reloader` fn pointers) and a `ScriptingPlugin<Self>` wrapper.
- **bms-core is wasm-clean** (no threads/target_arch hacks). The only wasm blocker for
  the Rhai (pure-Rust) backend is `bevy_asset/file_watcher` (#260) — a feature-flag fix.
- **Reflection bridge is OPTIONAL for Phase 1**: a ~350-450 LoC backend can load/run
  scripts, call callbacks, and register host functions without it (`ScriptValue::Reference`
  → error initially).
- Smallest template = the Rhai backend (~1273 LoC incl. reflection; ~350 LoC core).

## Known frictions (named in advance)
1. **`Context: Send`** but lua-rs `LuaState` holds GC raw pointers → `!Send`.
   → `unsafe impl Send` on the context wrapper. Sound on single-threaded wasm (our
   target); caveat for native multithreaded scheduler (bms holds context behind a Mutex,
   but moving across threads needs the GC to have no thread-affinity — flag, wasm-first).
2. **Stateful host fns**: bms functions are `Arc<dyn Fn(..)->ScriptValue>`, but lua-rs
   cclosures are bare `fn` pointers. → use the registry/trampoline pattern that
   `lua-hlua-shim` already implements (push registry index as upvalue + trampoline fn).
3. **lua-rs embedding API is rough** (manual stack). Phase 1 stays minimal; an ergonomic
   layer is the broader investment (also what frees `redis-rs-port` from mlua).

## Gates (fail cheap)
- **Gate 0 — verification (CHEAP, decisive):** bms + Rhai (pure-Rust) compiles to
  `wasm32-unknown-unknown` (file_watcher off) and ideally runs in browser. Proves the
  pure-Rust bms stack reaches the web and the *only* Lua-specific blocker is the VM.
  → If this fails, STOP: a lua-rs backend can't help.
- **Gate 1 — the backend:** new crate `bevy_mod_scripting_lua_rs` implementing the
  contract against lua-rs. Milestones: (1a) compiles native; (1b) loads+runs a Lua
  script; (1c) bms calls a Lua callback; (1d) register host fns; (1e) compiles+runs wasm.
  Reflection deferred to Phase 2.
- **Gate 2 — the proof:** a Lua-scripted Bevy app in the browser via the lua-rs backend;
  stretch = nano9 / a PICO-8 cart.

## File layout (this dir)
```
bms-lua-rs/
  DEEP_PLAN.md                      (this file)
  gate0-rhai-wasm/                  (Gate 0 verification crate)
  bevy_mod_scripting_lua_rs/        (Gate 1: the backend crate)
  demo/                             (Gate 2: Bevy app using the backend, → browser)
```
Backend deps on the **published** bms crates (`bevy_mod_scripting_core`/`_bindings`/
`_functions` 0.19) + lua-rs path deps — no need to fork the whole bms tree for the POC.

## Backend implementation checklist (from the rhai template)
- `src/lib.rs` (~250 LoC): `LuaRsContext { rt: LuaRuntime }` (+ `unsafe impl Send`),
  `type R = ()`, `impl IntoScriptPluginParams`, `make_plugin_config_static!`,
  `ScriptingPlugin<Self>` default (extensions=["lua"], context/pre-handling initializers
  that register globals + functions), `Plugin` impl, `context_load`/`reload` (exec chunk),
  `handler` (get_global callback, push ScriptValue args, pcall, convert return), error→
  `InteropError` conversions.
- `src/bindings/script_value.rs` (~120 LoC): `ScriptValue` ↔ lua-rs `LuaValue`
  (Unit/Bool/Integer/Float/String/List/Map/Function; Reference → error in Phase 1).
- `src/bindings/reference.rs` (Phase 2, ~300 LoC): `LuaReflectReference` userdata +
  metamethods bridging Bevy reflection. Deferred.

## Honest effort & ownership
- Realistically multi-session: ~350-450 LoC of dense bms-internals adapter + lua-rs API
  friction + many wasm compile cycles. Not one-shot.
- **PR submission is the user's call**, after a working POC + maintainer discussion on
  #166 (cold-dropping a new-VM-backend PR rarely merges). Build on a fork/branch first.
