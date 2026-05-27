//! Gate 1c: a headless bevy_mod_scripting app whose Lua scripting runs on the
//! pure-Rust lua-rs backend. bms fires `on_update` each frame → the lua-rs backend
//! runs the Lua callback → the script calls `log(frame)` → prints. Proves the full
//! bms → lua-rs path with no mlua / C Lua.

use std::time::Duration;

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::prelude::*;
use bevy_mod_scripting_asset::ScriptAsset;
use bevy_mod_scripting_core::{
    callback_labels, event::ScriptCallbackEvent, handler::event_handler,
    script::ScriptComponent, BMSScriptingInfrastructurePlugin,
};
use bevy_mod_scripting_functions::ScriptFunctionsPlugin;
use bevy_mod_scripting_lua_rs::LuaRsScriptingPlugin;

callback_labels!(OnUpdate => "on_update");

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

fn main() {
    App::new()
        .add_plugins(
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(5))),
        )
        .add_plugins(AssetPlugin {
            file_path: ASSETS.to_string(),
            ..Default::default()
        })
        .add_plugins(ScriptFunctionsPlugin)
        .add_plugins(BMSScriptingInfrastructurePlugin::default())
        .add_plugins(LuaRsScriptingPlugin::default())
        .add_systems(Startup, load_script)
        .add_systems(
            Update,
            (
                fire_on_update,
                event_handler::<OnUpdate, LuaRsScriptingPlugin>,
                maybe_exit,
            )
                .chain(),
        )
        .run();
}

fn load_script(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn(ScriptComponent::new(vec![
        assets.load::<ScriptAsset>("script.lua"),
    ]));
}

fn fire_on_update(mut events: MessageWriter<ScriptCallbackEvent>) {
    events.write(ScriptCallbackEvent::new_for_all_scripts(OnUpdate, vec![]));
}

fn maybe_exit(mut n: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *n += 1;
    if *n > 200 {
        println!("[demo] ran {} frames via lua-rs backend; exiting", *n - 1);
        exit.write(AppExit::Success);
    }
}
