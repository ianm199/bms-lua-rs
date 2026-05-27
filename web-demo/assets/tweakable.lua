-- Game of Life, in Lua, on pure-Rust lua-rs via bevy_mod_scripting.
-- Each call to param(name, default, min, max) adds a slider in the page and returns its
-- current value. Drag a slider to change the birth/survival rules live, or change the
-- seed density and hit Run to reseed.

LifeState = world.get_type_by_name("LifeState")
Settings = world.get_type_by_name("Settings")

info("Lua: tweakable game of life loaded")
math.randomseed(os.time())

function fetch_life_state()
    local i, v = next(world.query():component(LifeState):build())
    return v:components()[1]
end

function on_script_loaded()
    local cells = fetch_life_state().cells
    local density = math.floor(param("seed density", 1000, 0, 3000))
    for _ = 1, density do
        local index = math.random(#cells)
        cells[index] = 255
    end
end

function on_update()
    local cells = fetch_life_state().cells
    local dimensions = world.get_resource(Settings).physical_grid_dimensions
    local dimension_x = dimensions[1]
    local dimension_y = dimensions[2]

    local birth = math.floor(param("birth on", 3, 1, 8))
    local birth2 = math.floor(param("also born on", 0, 0, 8)) -- 0 turns this off; HighLife uses 6
    local survive_lo = math.floor(param("survive min", 2, 0, 8))
    local survive_hi = math.floor(param("survive max", 3, 0, 8))

    local prev_state = {}
    for v in pairs(cells) do
        prev_state[#prev_state + 1] = (not (v == 0)) and 1 or 0
    end
    for i = 1, (dimension_x * dimension_y) do
        local north = prev_state[i - dimension_x] or prev_state[i + dimension_x * (dimension_y - 1)]
        local south = prev_state[i + dimension_x] or prev_state[i - dimension_x * (dimension_y - 1)]
        local east = prev_state[i + 1] or 0
        local west = prev_state[i - 1] or 0
        local northeast = prev_state[i - dimension_x + 1] or 0
        local southeast = prev_state[i + dimension_x + 1] or 0
        local northwest = prev_state[i - dimension_x - 1] or 0
        local southwest = prev_state[i + dimension_x - 1] or 0
        local neighbours = north + south + east + west
            + northeast + southeast + northwest + southwest

        if prev_state[i] == 0 and (neighbours == birth or (birth2 > 0 and neighbours == birth2)) then
            cells[i] = 255
        elseif prev_state[i] == 1 and ((neighbours < survive_lo) or (neighbours > survive_hi)) then
            cells[i] = 0
        end
    end
end
