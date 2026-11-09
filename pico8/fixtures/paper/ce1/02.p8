pico-8 cartridge // http://www.pico-8.com
version 42
__lua__
-- sapbot's flame 2025
-- by sapbot

fire_width = 128  -- pico-8 screen width
fire_height = 128 -- pico-8 screen height
firepixels = {}
palette = {}

-- define the color palette (pico-8 uses a fixed palette)
function init_palette()
    palette = {
        0,  -- 0: black
        8,  -- 1: red
        9,  -- 2: orange
        10, -- 3: yellow
        7   -- 4: white
    }
end

function _init()
    init_palette()
    
    -- initialize firepixels
    for i = 1, fire_width * fire_height do
        firepixels[i] = 0
    end

    -- set bottom line to the brightest color (index 4)
    for i = 0, fire_width - 1 do
        firepixels[(fire_height - 1) * fire_width + i + 1] = 4
    end
end

function spreadfire(src)
    local pixel = firepixels[src]
    if pixel == 0 then
        firepixels[src - fire_width] = 0
    else
        local randidx = flr(rnd(4))
        local dst = src - randidx + 1
        firepixels[dst - fire_width] = max(0, pixel - (randidx % 2))
    end
end

function dofire()
    for x = 0, fire_width - 1 do
        for y = 1, fire_height - 1 do
            spreadfire(y * fire_width + x + 1)
        end
    end
end

function _update()
    dofire()  -- continuously update the fire effect
end

function _draw()
    cls()  -- clear the screen
    -- draw the fire effect
    for y = 0, fire_height - 1 do
        for x = 0, fire_width - 1 do
            local index = firepixels[y * fire_width + x + 1]
            if index > 0 then
                pset(x, y, palette[index])
            end
        end
    end
    print("sapbot's flame 2025. doom based.", 0, 0, 7)  -- print in white
end

__gfx__
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
