-- Snake — the entire game is this Lua script, run by lua-rs as a
-- bevy_mod_scripting backend, in the browser. No C.
-- It plays itself (a greedy food-seeking autopilot); press arrow keys / WASD to take over.

local COLS, ROWS, CELL = 21, 21, 26
local STEP = 0.09  -- seconds per move

local function half(n) return (n - 1) / 2 end
local function world(cx, cy)  -- grid cell -> Bevy world coords (center origin)
  return (cx - half(COLS)) * CELL, (half(ROWS) - cy) * CELL
end

local seed = 987654321  -- tiny self-contained PRNG (no host RNG needed)
local function rnd(n)
  seed = (seed * 1103515245 + 12345) % 2147483648
  return seed % n
end

local snake, dir, want, food, acc, score

local function occupied(nx, ny, ignore_tail)
  local last = ignore_tail and (#snake - 1) or #snake
  for i = 1, last do
    if snake[i].cx == nx and snake[i].cy == ny then return true end
  end
  return false
end

local function place_food()
  repeat
    food = { cx = rnd(COLS), cy = rnd(ROWS) }
  until not occupied(food.cx, food.cy, false)
end

local function reset()
  snake = { {cx=10,cy=10}, {cx=9,cy=10}, {cx=8,cy=10} }
  dir, want, acc, score = {dx=1,dy=0}, {dx=1,dy=0}, 0, 0
  place_food()
end
reset()

-- A move is safe if it isn't a reversal, stays in bounds, and won't hit the body.
local function safe(d)
  if d.dx == -dir.dx and d.dy == -dir.dy then return false end
  local h = snake[1]
  local nx, ny = h.cx + d.dx, h.cy + d.dy
  if nx < 0 or nx >= COLS or ny < 0 or ny >= ROWS then return false end
  return not occupied(nx, ny, true)
end

-- Greedy autopilot: head toward the food, prefer the larger axis gap, fall back to any safe move.
local function autopilot()
  local h = snake[1]
  local toward = {}
  local dx, dy = food.cx - h.cx, food.cy - h.cy
  local hx = { dx = (dx > 0) and 1 or -1, dy = 0 }
  local hy = { dx = 0, dy = (dy > 0) and 1 or -1 }
  if math.abs(dx) >= math.abs(dy) then
    if dx ~= 0 then toward[#toward+1] = hx end
    if dy ~= 0 then toward[#toward+1] = hy end
  else
    if dy ~= 0 then toward[#toward+1] = hy end
    if dx ~= 0 then toward[#toward+1] = hx end
  end
  for _, d in ipairs(toward) do if safe(d) then return d end end
  for _, d in ipairs({{dx=1,dy=0},{dx=-1,dy=0},{dx=0,dy=1},{dx=0,dy=-1}}) do
    if safe(d) then return d end
  end
  return dir
end

local function step()
  dir = want
  local h = snake[1]
  local nx, ny = h.cx + dir.dx, h.cy + dir.dy
  if nx < 0 or nx >= COLS or ny < 0 or ny >= ROWS then reset(); return end
  if occupied(nx, ny, true) then reset(); return end
  table.insert(snake, 1, {cx=nx, cy=ny})
  if nx == food.cx and ny == food.cy then
    score = score + 1
    place_food()
  else
    table.remove(snake)
  end
end

function on_update(dt, left, right, up, down)
  local steered = false
  if left  and dir.dx ~= 1  then want = {dx=-1, dy=0}; steered = true end
  if right and dir.dx ~= -1 then want = {dx=1,  dy=0}; steered = true end
  if up    and dir.dy ~= 1  then want = {dx=0,  dy=-1}; steered = true end
  if down  and dir.dy ~= -1 then want = {dx=0,  dy=1}; steered = true end
  if not steered then want = autopilot() end

  acc = acc + math.min(dt, 0.1)  -- clamp: a throttled/long frame must not burst many steps
  while acc >= STEP do acc = acc - STEP; step() end

  clear()
  local bw, bh = COLS * CELL, ROWS * CELL
  rect(0, 0, bw + 12, bh + 12, 0.10, 0.11, 0.16)        -- playfield
  local fx, fy = world(food.cx, food.cy)
  rect(fx, fy, CELL - 4, CELL - 4, 0.95, 0.30, 0.35)    -- food (red)
  for i, s in ipairs(snake) do                           -- snake (head brighter)
    local x, y = world(s.cx, s.cy)
    local g = (i == 1) and 1.0 or 0.72
    rect(x, y, CELL - 4, CELL - 4, 0.25, g, 0.45)
  end
  for i = 1, score do                                    -- score pips along the top
    rect(-bw/2 + (i - 0.5) * 12, bh/2 + 22, 9, 9, 1.0, 0.85, 0.2)
  end
end
