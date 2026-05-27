//! Local repro/fix harness for the editor hot-swap. At tick SWAP_AT it applies an edited
//! script by MUTATING the existing ScriptAsset (→ AssetEvent::Modified → bms reload →
//! re-exec → new on_update) and reseeds the grid from Rust (on_script_loaded won't re-fire
//! on a reload). Prints alive-counts; alive should stay > 0 after the swap when fixed.

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

const W: usize = 48;
const H: usize = 48;
const SWAP_AT: u32 = 40;
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

#[derive(Resource)]
struct ScriptHandle(Handle<ScriptAsset>);

fn reseed(cells: &mut [u8]) {
    for c in cells.iter_mut() {
        *c = 0;
    }
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..1000 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let idx = (seed >> 33) as usize % cells.len();
        cells[idx] = 255;
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
            (
                maybe_reload,
                fire_on_update,
                event_handler::<OnUpdate, LuaRsScriptingPlugin>,
                report,
            )
                .chain(),
        )
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let h = assets.load::<ScriptAsset>("game_of_life.lua");
    commands.spawn((
        LifeState {
            cells: vec![0u8; W * H],
        },
        ScriptComponent::new(vec![h.clone()]),
    ));
    commands.insert_resource(ScriptHandle(h));
}

fn fire_on_update(mut events: MessageWriter<ScriptCallbackEvent>) {
    events.write(ScriptCallbackEvent::new_for_all_scripts(OnUpdate, vec![]));
}

fn maybe_reload(
    mut n: Local<u32>,
    sh: Res<ScriptHandle>,
    mut assets: ResMut<Assets<ScriptAsset>>,
    mut life: Query<&mut LifeState>,
) {
    *n += 1;
    if *n == SWAP_AT {
        println!("[reload] >>> editor Run: mutate asset (reload) + reseed at tick {SWAP_AT}");
        let src = std::fs::read_to_string(format!("{ASSETS}/game_of_life.lua")).unwrap();
        if let Some(a) = assets.get_mut(&sh.0) {
            a.content = src.into_bytes().into_boxed_slice();
        }
        let _ = (&mut life, reseed); // minimal fix: rely on on_script_loaded firing on reload
    }
}

fn report(mut n: Local<u32>, q: Query<&LifeState>, mut exit: MessageWriter<AppExit>) {
    *n += 1;
    let alive = q
        .iter()
        .next()
        .map(|s| s.cells.iter().filter(|&&c| c != 0).count())
        .unwrap_or(0);
    if *n % 5 == 0 {
        let marker = if *n > SWAP_AT { " (post-reload)" } else { "" };
        println!("[reload] tick {} -> {} alive{}", *n, alive, marker);
    }
    if *n > 90 {
        exit.write(AppExit::Success);
    }
}
