//! Conway's Game of Life — bms's OWN `game_of_life.lua`, UNMODIFIED, running in the
//! browser on the pure-Rust lua-rs backend. The script queries the `LifeState` component
//! and reads/writes its `cells` via reflection; a Bevy system renders the grid. The Lua
//! is **editable live**: edit it in the page, hit Run, and the running game hot-reloads
//! through bms — no rebuild, no C, no server.

use std::cell::RefCell;

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
const CELL: f32 = 13.0;

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

#[derive(Component)]
struct Cell(usize);

/// Handle to the game's `ScriptAsset`, so the editor can hot-reload it by mutating its content.
#[derive(Resource)]
struct ScriptHandle(Handle<ScriptAsset>);

thread_local! {
    /// A Lua source edited in the page, awaiting application on the next frame.
    static PENDING_SCRIPT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Called from JS (the editor's Run button) with the edited Lua source.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn set_script(src: String) {
    PENDING_SCRIPT.with(|p| *p.borrow_mut() = Some(src));
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    meta_check: bevy::asset::AssetMetaCheck::Never,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        canvas: Some("#bevy".to_string()),
                        fit_canvas_to_parent: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
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
                apply_pending_script,
                fire_on_update,
                event_handler::<OnUpdate, LuaRsScriptingPlugin>,
                render_cells,
            )
                .chain(),
        )
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn(Camera2d);
    let handle = assets.load::<ScriptAsset>("game_of_life.lua");
    commands.spawn((
        LifeState {
            cells: vec![0u8; W * H],
        },
        ScriptComponent::new(vec![handle.clone()]),
    ));
    commands.insert_resource(ScriptHandle(handle));

    let origin_x = -(W as f32) * CELL / 2.0 + CELL / 2.0;
    let origin_y = (H as f32) * CELL / 2.0 - CELL / 2.0;
    for i in 0..(W * H) {
        let cx = (i % W) as f32;
        let cy = (i / W) as f32;
        commands.spawn((
            Sprite::from_color(Color::srgb(0.10, 0.11, 0.16), Vec2::splat(CELL - 1.5)),
            Transform::from_xyz(origin_x + cx * CELL, origin_y - cy * CELL, 0.0),
            Cell(i),
        ));
    }
}

/// Hot-reload when the editor submitted new Lua: mutate the existing `ScriptAsset`'s content,
/// which fires `AssetEvent::Modified` → bms reloads the context (re-execs the chunk, so the new
/// `on_update` rule takes effect, and `on_script_loaded` re-fires to reseed the grid).
fn apply_pending_script(sh: Res<ScriptHandle>, mut assets: ResMut<Assets<ScriptAsset>>) {
    let Some(src) = PENDING_SCRIPT.with(|p| p.borrow_mut().take()) else {
        return;
    };
    if let Some(asset) = assets.get_mut(&sh.0) {
        asset.content = src.into_bytes().into_boxed_slice();
    }
}

fn fire_on_update(mut events: MessageWriter<ScriptCallbackEvent>) {
    events.write(ScriptCallbackEvent::new_for_all_scripts(OnUpdate, vec![]));
}

fn render_cells(life: Query<&LifeState>, mut cells: Query<(&Cell, &mut Sprite)>) {
    let Some(state) = life.iter().next() else {
        return;
    };
    for (cell, mut sprite) in &mut cells {
        let alive = state.cells.get(cell.0).copied().unwrap_or(0) != 0;
        sprite.color = if alive {
            Color::srgb(0.25, 1.0, 0.45)
        } else {
            Color::srgb(0.10, 0.11, 0.16)
        };
    }
}
