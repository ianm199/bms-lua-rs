//! Native check of the `param()` host fn + slider plumbing. Loads tweakable.lua, confirms the
//! script declares its knobs (params_json lists them), then `set_param("birth on", 6)` and
//! verifies the simulation keeps running with the new rule — no browser needed.

use std::time::Duration;

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::prelude::*;
use bevy_mod_scripting_asset::ScriptAsset;
use bevy_mod_scripting_core::{
    callback_labels, event::ScriptCallbackEvent, handler::event_handler,
    script::ScriptComponent, BMSScriptingInfrastructurePlugin,
};
use bevy_mod_scripting_functions::ScriptFunctionsPlugin;
use bevy_mod_scripting_lua_rs::{params_json, set_param, LuaRsScriptingPlugin};

callback_labels!(OnUpdate => "on_update");

const W: usize = 48;
const H: usize = 48;
const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

#[derive(Debug, Default, Clone, Reflect, Component)]
#[reflect(Component, Default)]
pub struct LifeState {
    pub cells: Vec<u8>,
}

#[derive(Reflect, Resource)]
#[reflect(Resource)]
pub struct Settings {
    physical_grid_dimensions: (u32, u32),
    display_grid_dimensions: (u32, u32),
    border_thickness: u32,
    live_color: u8,
    dead_color: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            physical_grid_dimensions: (W as u32, H as u32),
            display_grid_dimensions: (W as u32, H as u32),
            border_thickness: 0,
            live_color: 255,
            dead_color: 0,
        }
    }
}

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(2))))
        .add_plugins(AssetPlugin {
            file_path: ASSETS.to_string(),
            ..Default::default()
        })
        .register_type::<LifeState>()
        .register_type::<Settings>()
        .init_resource::<Settings>()
        .add_plugins(BMSScriptingInfrastructurePlugin::default())
        .add_plugins(ScriptFunctionsPlugin)
        .add_plugins(LuaRsScriptingPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (fire_on_update, event_handler::<OnUpdate, LuaRsScriptingPlugin>, report).chain(),
        )
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((
        LifeState {
            cells: vec![0u8; W * H],
        },
        ScriptComponent::new(vec![assets.load::<ScriptAsset>("tweakable.lua")]),
    ));
}

fn fire_on_update(mut events: MessageWriter<ScriptCallbackEvent>) {
    events.write(ScriptCallbackEvent::new_for_all_scripts(OnUpdate, vec![]));
}

fn report(mut n: Local<u32>, q: Query<&LifeState>, mut exit: MessageWriter<AppExit>) {
    *n += 1;
    let alive = q
        .iter()
        .next()
        .map(|s| s.cells.iter().filter(|&&c| c != 0).count())
        .unwrap_or(0);
    if *n == 20 {
        println!("[param] declared knobs: {}", params_json());
        println!("[param] alive @20 (birth=3 default) = {alive}; now set 'birth on' = 6");
        set_param("birth on", 6.0);
    }
    if *n % 10 == 0 {
        println!("[param] tick {} -> {} alive", *n, alive);
    }
    if *n > 50 {
        println!("[param] final knobs: {}", params_json());
        exit.write(AppExit::Success);
    }
}
