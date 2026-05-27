//! Gate 0: does the pure-Rust bms Rhai backend compile to wasm32-unknown-unknown?
//! Constructing the plugin forces bms-core + bindings + rhai + their bevy deps to
//! compile for the target. If `cargo build --target wasm32-unknown-unknown` succeeds,
//! the pure-Rust bms stack reaches the web and the only Lua-specific blocker is the VM.

fn main() {
    let _plugin = bevy_mod_scripting_rhai::RhaiScriptingPlugin::default();
    println!("bms rhai backend constructed");
}
