-- Gameplay logic in Lua, executed by lua-rs as a bevy_mod_scripting backend.
-- bms fires `on_update` each frame; lua-rs runs it; `log` (a host fn registered
-- by the backend) prints — proving bms -> lua-rs callbacks work.
local frame = 0
function on_update()
  frame = frame + 1
  log(frame)
end
