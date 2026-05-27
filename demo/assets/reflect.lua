-- Milestone A: read+write a reflected Bevy resource from Lua via the reflection bridge.
-- pcall so the actual error message is observable through `log`.
function on_update()
  local ok, err = pcall(function()
    local t = world.get_type_by_name("Counter")
    local c = world.get_resource(t)
    c.value = c.value + 1
  end)
  if not ok then log(tostring(err)) end
end
