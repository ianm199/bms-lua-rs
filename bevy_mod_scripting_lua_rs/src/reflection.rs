//! The reflection bridge: gives Lua scripts access to the Bevy ECS through bms's
//! language-agnostic reflection layer (`bevy_mod_scripting_bindings`).
//!
//! Each reflected value (`ReflectReference`) is wrapped in a real lua-rs **userdata**
//! ([`LuaRef`]) carrying its own reference — so references round-trip as function
//! arguments with no arena bookkeeping. Field/index access and `#`/`pairs` go through the
//! userdata's metamethods (`__index`/`__newindex`/`__len`/`__pairs`), which call bms's
//! `MagicFunctions` and registered functions. The `world` global is a [`WorldProxy`]
//! userdata whose `__index` resolves World-namespace functions (dot-call, no receiver).

use std::any::TypeId;

use bevy_mod_scripting_asset::Language;
use bevy_mod_scripting_bindings::{
    DynamicScriptFunction, DynamicScriptFunctionMut, FunctionCallContext, InteropError,
    ReflectReference, ScriptValue, ThreadWorldContainer,
};
use lua_rs_runtime::{
    Function, Lua, LuaError, MetaMethod, Result as LuaResult, UserData, UserDataMethods, Value,
    Variadic,
};

const LUA_CTX: FunctionCallContext = FunctionCallContext::new(Language::Lua);

fn interop_to_lua(e: InteropError) -> LuaError {
    LuaError::runtime(format_args!("bms: {e}"))
}

fn reflect_ref_tid() -> TypeId {
    TypeId::of::<ReflectReference>()
}

/// A reflected Bevy value, wrapped as Lua userdata.
struct LuaRef(ReflectReference);

impl UserData for LuaRef {
    fn add_meta_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // `obj.field` / `obj[i]` / `obj:method(..)`: a string key may name a typed method
        // (colon call passes the receiver as arg 0, which converts back via lua_to_script_value);
        // otherwise it's a field/index read through the magic getter.
        m.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            let reference = this.0.clone();
            let world = ThreadWorldContainer
                .try_get_context()
                .map_err(interop_to_lua)?
                .world;
            if let Value::String(ref s) = key {
                let name = s.to_str()?;
                let tid = reference.tail_type_id(world.clone()).ok().flatten();
                let lookup = match tid {
                    Some(t) => vec![t, reflect_ref_tid()],
                    None => vec![reflect_ref_tid()],
                };
                if let Ok(func) = world.lookup_function(lookup, name) {
                    return Ok(Value::Function(wrap_function(lua, func)?));
                }
            }
            let key_sv = lua_to_script_value(lua, key)?;
            let registry = world.script_function_registry();
            let registry = registry.read();
            let out = registry
                .magic_functions
                .get(LUA_CTX, reference, key_sv)
                .map_err(interop_to_lua)?;
            drop(registry);
            script_value_to_lua(lua, out)
        });

        // `obj.field = v` / `obj[i] = v` → the magic setter.
        m.add_meta_method(MetaMethod::NewIndex, |lua, this, (key, value): (Value, Value)| {
            let reference = this.0.clone();
            let key_sv = lua_to_script_value(lua, key)?;
            let value_sv = lua_to_script_value(lua, value)?;
            let world = ThreadWorldContainer
                .try_get_context()
                .map_err(interop_to_lua)?
                .world;
            let registry = world.script_function_registry();
            let registry = registry.read();
            registry
                .magic_functions
                .set(LUA_CTX, reference, key_sv, value_sv)
                .map_err(interop_to_lua)?;
            Ok(())
        });

        // `#obj` → bms's registered `len`.
        m.add_meta_method(MetaMethod::Len, |lua, this, ()| {
            let reference = this.0.clone();
            let world = ThreadWorldContainer
                .try_get_context()
                .map_err(interop_to_lua)?
                .world;
            let func = world
                .lookup_function([reflect_ref_tid()], "len")
                .map_err(|_| LuaError::runtime(format_args!("lua-rs bridge: no `len` function")))?;
            let out = func
                .call([ScriptValue::Reference(reference)], LUA_CTX)
                .map_err(interop_to_lua)?;
            script_value_to_lua(lua, out)
        });

        // `for v in pairs(obj)` → bms's registered `iter` (a stateful next-function).
        m.add_meta_method(MetaMethod::Pairs, |lua, this, ()| -> LuaResult<(Value, Value, Value)> {
            let reference = this.0.clone();
            let world = ThreadWorldContainer
                .try_get_context()
                .map_err(interop_to_lua)?
                .world;
            let tid = reference.tail_type_id(world.clone()).ok().flatten();
            let lookup = match tid {
                Some(t) => vec![t, reflect_ref_tid()],
                None => vec![reflect_ref_tid()],
            };
            let iter_fn = world
                .lookup_function(lookup, "iter")
                .map_err(|_| LuaError::runtime(format_args!("lua-rs bridge: no `iter` function")))?;
            let iter_val = iter_fn
                .call([ScriptValue::Reference(reference)], LUA_CTX)
                .map_err(interop_to_lua)?;
            let next_dyn = match iter_val {
                ScriptValue::FunctionMut(f) => f,
                other => {
                    return Err(LuaError::runtime(format_args!(
                        "lua-rs bridge: `iter` did not return an iterator: {other:?}"
                    )))
                }
            };
            // The generic-for passes (state, control) each step; bms's iterator takes none.
            let next_lua = lua.create_function(move |inner, _ignored: Variadic<Value>| {
                let out = next_dyn
                    .call(Vec::<ScriptValue>::new(), LUA_CTX)
                    .map_err(interop_to_lua)?;
                script_value_to_lua(inner, out)
            })?;
            Ok((Value::Function(next_lua), Value::Nil, Value::Nil))
        });
    }
}

/// The `world` global: `world.method(..)` resolves a World-namespace function (no receiver).
struct WorldProxy;

impl UserData for WorldProxy {
    fn add_meta_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, _this, name: String| {
            let world = ThreadWorldContainer
                .try_get_context()
                .map_err(interop_to_lua)?
                .world;
            let world_tid = TypeId::of::<bevy_ecs::world::World>();
            match world.lookup_function([world_tid], name.clone()) {
                Ok(func) => Ok(Value::Function(wrap_function(lua, func)?)),
                Err(_) => Err(LuaError::runtime(format_args!(
                    "lua-rs bridge: world has no function '{name}'"
                ))),
            }
        });
    }
}

/// Convert a bms `ScriptValue` into a Lua value.
pub fn script_value_to_lua(lua: &Lua, v: ScriptValue) -> LuaResult<Value> {
    Ok(match v {
        ScriptValue::Unit => Value::Nil,
        ScriptValue::Bool(b) => Value::Boolean(b),
        ScriptValue::Integer(i) => Value::Integer(i),
        ScriptValue::Float(f) => Value::Number(f),
        ScriptValue::String(s) => Value::String(lua.create_string(s.as_bytes())?),
        ScriptValue::Reference(r) => Value::UserData(lua.create_userdata(LuaRef(r))?),
        ScriptValue::List(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.into_iter().enumerate() {
                let lv = script_value_to_lua(lua, item)?;
                t.set((i + 1) as i64, lv)?;
            }
            Value::Table(t)
        }
        ScriptValue::Map(map) => {
            let t = lua.create_table()?;
            for (k, item) in map {
                let lv = script_value_to_lua(lua, item)?;
                t.set(k, lv)?;
            }
            Value::Table(t)
        }
        ScriptValue::Function(f) => Value::Function(wrap_function(lua, f)?),
        ScriptValue::FunctionMut(f) => Value::Function(wrap_function_mut(lua, f)?),
        ScriptValue::Error(e) => return Err(interop_to_lua(e)),
    })
}

/// Convert a Lua value into a bms `ScriptValue`. Userdata recovers its reflect reference.
pub fn lua_to_script_value(_lua: &Lua, v: Value) -> LuaResult<ScriptValue> {
    Ok(match v {
        Value::Nil => ScriptValue::Unit,
        Value::Boolean(b) => ScriptValue::Bool(b),
        Value::Integer(i) => ScriptValue::Integer(i),
        Value::Number(f) => ScriptValue::Float(f),
        Value::String(s) => ScriptValue::String(s.to_str()?.into()),
        Value::UserData(ud) => {
            let lref = ud.borrow::<LuaRef>().map_err(|_| {
                LuaError::runtime(format_args!(
                    "lua-rs bridge: userdata is not a reflect reference"
                ))
            })?;
            ScriptValue::Reference(lref.0.clone())
        }
        _ => {
            return Err(LuaError::runtime(format_args!(
                "lua-rs bridge: unsupported Lua value as a ScriptValue argument"
            )))
        }
    })
}

/// Wrap a bms `DynamicScriptFunction` as a Lua function (args convert via `lua_to_script_value`).
fn wrap_function(lua: &Lua, func: DynamicScriptFunction) -> LuaResult<Function> {
    lua.create_function(move |inner, args: Variadic<Value>| {
        let sv_args = args
            .into_iter()
            .map(|v| lua_to_script_value(inner, v))
            .collect::<LuaResult<Vec<_>>>()?;
        let out = func.call(sv_args, LUA_CTX).map_err(interop_to_lua)?;
        script_value_to_lua(inner, out)
    })
}

/// Wrap a bms `DynamicScriptFunctionMut` (e.g. an iterator) as a Lua function.
fn wrap_function_mut(lua: &Lua, func: DynamicScriptFunctionMut) -> LuaResult<Function> {
    lua.create_function(move |inner, args: Variadic<Value>| {
        let sv_args = args
            .into_iter()
            .map(|v| lua_to_script_value(inner, v))
            .collect::<LuaResult<Vec<_>>>()?;
        let out = func.call(sv_args, LUA_CTX).map_err(interop_to_lua)?;
        script_value_to_lua(inner, out)
    })
}

/// Install the `world` global.
pub fn install_world_global(lua: &Lua) -> LuaResult<()> {
    let world = lua.create_userdata(WorldProxy)?;
    lua.globals().set("world", Value::UserData(world))?;
    Ok(())
}
