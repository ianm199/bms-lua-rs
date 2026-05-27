//! Milestone A: prove the lua-rs reflection bridge round-trips. A Lua script reads and
//! writes a reflected Bevy resource via `world.get_resource(...)`, driven by bms on the
//! pure-Rust lua-rs backend. No mlua / C Lua. If `Counter.value` climbs, reflection works.

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

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct Counter {
    value: i32,
}

const ASSETS: &str =
    "/Users/ianmclaughlin/PycharmProjects/rustExperiments/bms-lua-rs/demo/assets";

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(2))))
        .add_plugins(AssetPlugin {
            file_path: ASSETS.to_string(),
            ..Default::default()
        })
        .register_type::<Counter>()
        .insert_resource(Counter { value: 0 })
        .add_plugins(BMSScriptingInfrastructurePlugin::default())
        .add_plugins(ScriptFunctionsPlugin)
        .add_plugins(LuaRsScriptingPlugin::default())
        .add_systems(Startup, load_script)
        .add_systems(
            Update,
            (
                fire_on_update,
                event_handler::<OnUpdate, LuaRsScriptingPlugin>,
                check,
            )
                .chain(),
        )
        .run();
}

fn load_script(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn(ScriptComponent::new(vec![
        assets.load::<ScriptAsset>("reflect.lua"),
    ]));
}

fn fire_on_update(mut events: MessageWriter<ScriptCallbackEvent>) {
    events.write(ScriptCallbackEvent::new_for_all_scripts(OnUpdate, vec![]));
}

fn check(mut n: Local<u32>, counter: Res<Counter>, mut exit: MessageWriter<AppExit>) {
    *n += 1;
    if *n % 20 == 0 {
        println!("[reflect_test] frame {} -> Counter.value = {}", *n, counter.value);
    }
    if *n > 80 {
        println!(
            "[reflect_test] FINAL Counter.value = {} (PASS if > 0)",
            counter.value
        );
        exit.write(AppExit::Success);
    }
}
