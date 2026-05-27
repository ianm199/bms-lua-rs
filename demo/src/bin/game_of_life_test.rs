//! Milestone B: run bms's OWN `game_of_life.lua` UNMODIFIED on the pure-Rust lua-rs
//! backend. The script queries the `LifeState` component, reads/writes its `cells` Vec via
//! reflection, and steps Conway's Game of Life every `on_update`. Headless: we read the
//! component back from Rust and confirm the grid evolves. No mlua / C Lua.

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

const W: usize = 32;
const H: usize = 32;

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

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

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
            (
                fire_on_update,
                event_handler::<OnUpdate, LuaRsScriptingPlugin>,
                report,
            )
                .chain(),
        )
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((
        LifeState {
            cells: vec![0u8; W * H],
        },
        ScriptComponent::new(vec![assets.load::<ScriptAsset>("game_of_life.lua")]),
    ));
}

fn fire_on_update(mut events: MessageWriter<ScriptCallbackEvent>) {
    events.write(ScriptCallbackEvent::new_for_all_scripts(OnUpdate, vec![]));
}

fn report(
    mut n: Local<u32>,
    mut history: Local<Vec<usize>>,
    q: Query<&LifeState>,
    mut exit: MessageWriter<AppExit>,
) {
    *n += 1;
    let alive = q
        .iter()
        .next()
        .map(|s| s.cells.iter().filter(|&&c| c != 0).count())
        .unwrap_or(0);
    if *n % 10 == 0 {
        println!("[gol] tick {} -> {} live cells", *n, alive);
        history.push(alive);
    }
    if *n > 60 {
        let distinct: std::collections::HashSet<_> = history.iter().collect();
        let evolving = distinct.len() > 1 && history.iter().any(|&c| c > 0);
        println!(
            "[gol] history={:?} -> {}",
            history,
            if evolving {
                "PASS: grid evolves (game_of_life.lua runs unmodified via lua-rs)"
            } else {
                "FAIL: grid did not evolve"
            }
        );
        exit.write(AppExit::Success);
    }
}
