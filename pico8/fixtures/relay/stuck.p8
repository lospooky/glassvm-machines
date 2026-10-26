pico-8 cartridge // http://www.pico-8.com
version 41
__lua__
-- Glass Relay stuck; original repository-authored specimen
shown = false
was_down = false
remaining = 0
function _update60()
  local down = btn(0)
  if down then
    if not was_down then remaining = 0 end
    if remaining > 0 then
      remaining = remaining - 1
    else
      shown = true
    end
  else
    -- deliberate defect: leave shown unchanged
    remaining = 0
  end
  was_down = down
end
function _draw()
  cls(0)
  if shown then rectfill(8, 8, 11, 8, 7) end
end
