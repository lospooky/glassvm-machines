-- script: lua
-- Glass Relay reference; original repository-authored specimen
shown = false
was_down = false
remaining = 0
function TIC()
  local down = btn(0)
  if down then
    if not was_down then remaining = 0 end
    if remaining > 0 then
      remaining = remaining - 1
    else
      shown = true
    end
  else
    shown = false
    remaining = 0
  end
  was_down = down
  cls(0)
  if shown then rect(8, 8, 4, 1, 15) end
end
