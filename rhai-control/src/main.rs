//! Control: bms + Rhai (pure-Rust, NOT lua-rs) in wasm.
//!
//! The heartbeat logs every 30 frames. If it climbs, bms's machine `tick` runs to
//! completion in wasm with the Rhai backend → the lua-rs freeze is lua-rs-specific.
//! If it stalls at ~1, a single bms `tick` hangs in wasm regardless of backend →
//! the bms pipeline itself doesn't run in wasm (an upstream issue beyond the Instant fix).

use bevy::prelude::*;
use bevy_mod_scripting_asset::ScriptAsset;
use bevy_mod_scripting_bindings::ScriptValue;
use bevy_mod_scripting_core::{
    callback_labels, event::ScriptCallbackEvent, handler::event_handler,
    script::ScriptComponent, BMSScriptingInfrastructurePlugin,
};
use bevy_mod_scripting_rhai::RhaiScriptingPlugin;

callback_labels!(OnUpdate => "on_update");

/// A pure-Bevy moving box (no scripting). If a bms `tick` hangs the Update schedule,
/// this stops sweeping — a screenshot-visible freeze detector.
#[derive(Component)]
struct Indicator;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            meta_check: bevy::asset::AssetMetaCheck::Never,
            ..default()
        }))
        .add_plugins(BMSScriptingInfrastructurePlugin::default())
        .add_plugins(RhaiScriptingPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, heartbeat)
        .add_systems(
            Update,
            (fire_on_update, event_handler::<OnUpdate, RhaiScriptingPlugin>).chain(),
        )
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn(Camera2d);
    commands.spawn((
        Sprite::from_color(Color::srgb(1.0, 0.55, 0.15), Vec2::new(80.0, 80.0)),
        Transform::from_xyz(0.0, 0.0, 0.0),
        Indicator,
    ));
    commands.spawn(ScriptComponent::new(vec![
        assets.load::<ScriptAsset>("script.rhai"),
    ]));
}

fn fire_on_update(mut events: MessageWriter<ScriptCallbackEvent>, time: Res<Time>) {
    events.write(ScriptCallbackEvent::new_for_all_scripts(
        OnUpdate,
        vec![ScriptValue::Float(time.elapsed_secs() as f64)],
    ));
}

fn heartbeat(mut n: Local<u32>, mut q: Query<&mut Transform, With<Indicator>>) {
    *n += 1;
    // sweep horizontally so a screenshot shows whether frames are advancing
    let x = (((*n as f32) * 5.0) % 600.0) - 300.0;
    for mut t in &mut q {
        t.translation.x = x;
    }
}
