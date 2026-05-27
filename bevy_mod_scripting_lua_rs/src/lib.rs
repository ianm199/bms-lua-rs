//! A pure-Rust Lua (lua-rs) backend for bevy_mod_scripting.
//!
//! Replaces the mlua (C Lua) backend so Lua scripting compiles to
//! `wasm32-unknown-unknown` with no C toolchain (bms issue #166).
//!
//! Scripts run on the high-level `lua-rs` embedding API (`Lua`, owned handles,
//! `create_function`). The [`reflection`] module bridges Lua to the Bevy ECS via bms's
//! reflection layer, so scripts can read/write reflected components and resources.

mod reflection;

use bevy_app::{App, Plugin};
use bevy_ecs::world::WorldId;
use bevy_log::{error, info};
use bevy_mod_scripting_asset::Language;
use bevy_mod_scripting_bindings::{InteropError, ScriptValue};
use bevy_mod_scripting_core::{
    config::{GetPluginThreadConfig, ScriptingPluginConfiguration},
    event::CallbackLabel,
    make_plugin_config_static,
    script::ContextPolicy,
    IntoScriptPluginParams, ScriptingPlugin,
};
use bevy_mod_scripting_script::ScriptAttachment;

use std::cell::RefCell;

use lua_rs_runtime::{Function, HostHooks, Lua, LuaError, Result as LuaResult, Value, Variadic};

/// `os.time`/`os.date` source for lua-rs. `Lua::new()` provides none, so on wasm `os.time()`
/// errors ("current time not available"); scripts like game_of_life call it to seed RNG.
fn host_unix_time() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        use std::sync::atomic::{AtomicI64, Ordering};
        static COUNTER: AtomicI64 = AtomicI64::new(1);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }
}

/// A draw command produced by the `rect` host fn, drained by a demo renderer.
#[derive(Clone, Copy)]
pub struct DrawCmd {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

thread_local! {
    static DRAW_BUF: RefCell<Vec<DrawCmd>> = const { RefCell::new(Vec::new()) };
}

/// Read the current frame's draw commands (for a demo renderer). Single-threaded.
pub fn draw_buffer() -> Vec<DrawCmd> {
    DRAW_BUF.with(|b| b.borrow().clone())
}

/// A tunable parameter a script declared via the `param(name, default, min, max)` host fn.
/// The host renders a control per param; the script reads the current value back.
struct ParamDef {
    name: String,
    value: f64,
    min: f64,
    max: f64,
}

thread_local! {
    static PARAMS: RefCell<Vec<ParamDef>> = const { RefCell::new(Vec::new()) };
}

/// The declared params as JSON (`[{"name","value","min","max"}, ...]`) for a host UI.
pub fn params_json() -> String {
    PARAMS.with(|p| {
        let items: Vec<String> = p
            .borrow()
            .iter()
            .map(|d| {
                format!(
                    "{{\"name\":{:?},\"value\":{},\"min\":{},\"max\":{}}}",
                    d.name, d.value, d.min, d.max
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    })
}

/// Set a declared param's value (from a host UI control). No-op for unknown names.
pub fn set_param(name: &str, value: f64) {
    PARAMS.with(|p| {
        if let Some(d) = p.borrow_mut().iter_mut().find(|d| d.name == name) {
            d.value = value;
        }
    });
}

/// A lua-rs runtime, used as a bms script context.
///
/// `Lua` holds GC raw pointers and is not `Send`. bms requires `Context: Send`, storing
/// contexts behind a `Mutex`. We assert `Send` because our target is single-threaded
/// wasm (and native access is serialized through the Mutex).
pub struct LuaRsContext {
    lua: Lua,
}

// SAFETY: contexts are accessed one-at-a-time behind bms's Mutex; the demo target is
// single-threaded wasm where cross-thread access cannot occur.
unsafe impl Send for LuaRsContext {}

make_plugin_config_static!(LuaRsScriptingPlugin);

/// A small owned error wrapper so dynamic lua-rs error messages can flow into
/// `InteropError::external` (which needs `'static`; `InteropError::str` wants `&'static str`).
#[derive(Debug)]
struct LuaRsError(String);

impl std::fmt::Display for LuaRsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LuaRsError {}

fn lua_err(e: LuaError) -> InteropError {
    InteropError::external(LuaRsError(format!("lua-rs: {}", lua_err_msg(&e))))
}

/// Render a `LuaError`'s message. `LuaError`'s `Display` only prints the GC pointer for
/// `Runtime`/`Syntax` string payloads, so extract the underlying string bytes.
fn lua_err_msg(e: &LuaError) -> String {
    use lua_types::value::LuaValue;
    match e {
        LuaError::Runtime(LuaValue::Str(s)) | LuaError::Syntax(LuaValue::Str(s)) => {
            String::from_utf8_lossy(s.as_bytes()).into_owned()
        }
        other => format!("{other:?}"),
    }
}

impl IntoScriptPluginParams for LuaRsScriptingPlugin {
    type C = LuaRsContext;
    type R = ();

    const LANGUAGE: Language = Language::Lua;

    fn build_runtime() -> Self::R {}

    fn handler() -> bevy_mod_scripting_core::handler::HandlerFn<Self> {
        lua_rs_handler
    }

    fn context_loader() -> bevy_mod_scripting_core::context::ContextLoadFn<Self> {
        lua_rs_context_load
    }

    fn context_reloader() -> bevy_mod_scripting_core::context::ContextReloadFn<Self> {
        lua_rs_context_reload
    }
}

/// Register the demo host functions (`log`, `clear`, `rect`) as capturing closures.
fn register_host_fns(lua: &Lua) -> LuaResult<()> {
    let g = lua.globals();

    let log = lua.create_function(|_l, v: Value| {
        match v {
            Value::Integer(i) => info!("[lua-rs<-bms] log: {i}"),
            Value::Number(f) => info!("[lua-rs<-bms] log: {f}"),
            Value::String(s) => {
                eprintln!("[lua log] {}", s.to_str().unwrap_or_default())
            }
            other => eprintln!("[lua log] {other:?}"),
        }
        Ok(())
    })?;
    g.set("log", log)?;

    let clear = lua.create_function(|_l, ()| {
        DRAW_BUF.with(|b| b.borrow_mut().clear());
        Ok(())
    })?;
    g.set("clear", clear)?;

    let rect = lua.create_function(|_l, args: Variadic<f64>| {
        let a = args.into_vec();
        if a.len() >= 7 {
            DRAW_BUF.with(|buf| {
                buf.borrow_mut().push(DrawCmd {
                    x: a[0] as f32,
                    y: a[1] as f32,
                    w: a[2] as f32,
                    h: a[3] as f32,
                    r: a[4] as f32,
                    g: a[5] as f32,
                    b: a[6] as f32,
                })
            });
        }
        Ok(())
    })?;
    g.set("rect", rect)?;

    // `param(name, default, min, max) -> number`: declare a tunable knob and read its current
    // value. New names register with the default; the host renders a control and feeds values
    // back via `set_param`, so the script keeps its value across hot-reloads.
    let param = lua.create_function(|_l, (name, rest): (String, Variadic<f64>)| {
        let r = rest.into_vec();
        let default = r.first().copied().unwrap_or(0.0);
        let min = r.get(1).copied().unwrap_or(0.0);
        let max = r.get(2).copied().unwrap_or(1.0);
        let value = PARAMS.with(|p| {
            let mut p = p.borrow_mut();
            match p.iter().find(|d| d.name == name) {
                Some(d) => d.value,
                None => {
                    p.push(ParamDef {
                        name,
                        value: default,
                        min,
                        max,
                    });
                    default
                }
            }
        });
        Ok(value)
    })?;
    g.set("param", param)?;

    Ok(())
}

fn value_to_display(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.to_str().unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

/// Install the standard script logging globals (`print`/`info`/`warn`/`error`/`debug`/
/// `trace`) that bms scripts expect from the language runtime. They route to `bevy_log`.
fn register_log_globals(lua: &Lua) -> LuaResult<()> {
    let g = lua.globals();
    for name in ["print", "info", "warn", "error", "debug", "trace"] {
        let f = lua.create_function(|_l, args: Variadic<Value>| {
            let msg = args
                .iter()
                .map(value_to_display)
                .collect::<Vec<_>>()
                .join("\t");
            info!("[lua] {msg}");
            Ok(())
        })?;
        g.set(name, f)?;
    }
    Ok(())
}

/// Load a lua-rs context from a script (runs the chunk to define callbacks/globals).
pub fn lua_rs_context_load(
    _context_key: &ScriptAttachment,
    content: &[u8],
    _world_id: WorldId,
) -> Result<LuaRsContext, InteropError> {
    let lua = Lua::with_hooks(HostHooks::default().unix_time(host_unix_time)).map_err(lua_err)?;
    register_host_fns(&lua).map_err(lua_err)?;
    register_log_globals(&lua).map_err(lua_err)?;
    reflection::install_world_global(&lua).map_err(lua_err)?;
    lua.load(content)
        .set_name(b"=script")
        .exec()
        .map_err(lua_err)?;
    Ok(LuaRsContext { lua })
}

/// Reload: re-run the chunk in the existing context.
pub fn lua_rs_context_reload(
    _context_key: &ScriptAttachment,
    content: &[u8],
    context: &mut LuaRsContext,
    _world_id: WorldId,
) -> Result<(), InteropError> {
    context
        .lua
        .load(content)
        .set_name(b"=script")
        .exec()
        .map_err(lua_err)
}

/// Invoke a Lua callback by name with the given args, marshalling through `ScriptValue`.
pub fn lua_rs_handler(
    args: Vec<ScriptValue>,
    _context_key: &ScriptAttachment,
    callback: &CallbackLabel,
    context: &mut LuaRsContext,
    _world_id: WorldId,
) -> Result<ScriptValue, InteropError> {
    run_callback(&context.lua, callback.as_ref(), args)
}

fn run_callback(lua: &Lua, name: &str, args: Vec<ScriptValue>) -> Result<ScriptValue, InteropError> {
    let callback: Value = lua.globals().get(name).map_err(lua_err)?;
    let func: Function = match callback {
        Value::Function(f) => f,
        // This script doesn't define this callback — skip cleanly.
        _ => return Ok(ScriptValue::Unit),
    };
    let lua_args = args
        .into_iter()
        .map(|a| reflection::script_value_to_lua(lua, a))
        .collect::<LuaResult<Vec<_>>>()
        .map_err(lua_err)?;
    match func.call::<Variadic<Value>, Value>(Variadic::from(lua_args)) {
        Ok(ret) => reflection::lua_to_script_value(lua, ret).map_err(lua_err),
        Err(e) => {
            error!("lua-rs callback '{name}': {}", lua_err_msg(&e));
            Err(lua_err(e))
        }
    }
}

/// The lua-rs scripting plugin for bms.
pub struct LuaRsScriptingPlugin {
    /// The internal bms scripting plugin.
    pub scripting_plugin: ScriptingPlugin<LuaRsScriptingPlugin>,
}

impl AsMut<ScriptingPlugin<Self>> for LuaRsScriptingPlugin {
    fn as_mut(&mut self) -> &mut ScriptingPlugin<Self> {
        &mut self.scripting_plugin
    }
}

impl Default for LuaRsScriptingPlugin {
    fn default() -> Self {
        LuaRsScriptingPlugin {
            scripting_plugin: ScriptingPlugin {
                supported_extensions: vec!["lua"],
                runtime_initializers: vec![],
                context_initializers: vec![],
                context_pre_handling_initializers: vec![],
                language: Language::Lua,
                context_policy: ContextPolicy::default(),
                emit_responses: false,
                processing_pipeline_plugin: Default::default(),
            },
        }
    }
}

impl Plugin for LuaRsScriptingPlugin {
    fn build(&self, app: &mut App) {
        self.scripting_plugin.build(app);
    }

    fn finish(&self, app: &mut App) {
        self.scripting_plugin.finish(app);
    }
}
