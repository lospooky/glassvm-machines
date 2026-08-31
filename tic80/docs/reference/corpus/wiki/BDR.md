_This was added to the API in version 0.90. See [SCN](https://github.com/nesbox/TIC-80/wiki/SCN) for older carts._

The **BDR** callback allows you to execute code _between_ the rendering of each scan line. The primary reason to do this is to manipulate the [palette](palette).  Doing so makes it possible to use a different palette for each scan line, and therefore [more than 16 colors at a time](Palette#more-than-16-colors).

## Usage

``` lua
function BDR(scanline)
  -- your code here
end
```

## Parameters

* **scanline** : The scan line about to be drawn (0..143)


## Details

```
+-----------+-----------------
| scanline  | display        |
+-----------+----------------+
| 0 - 3     | TOP BORDER     |
| 4         | ROW 0          |  equiv SCN(0)
| ...       | ...            |  equiv SCN(n - 4)
| 139       | ROW 135        |  equiv SCN(135)
| 140 - 143 | BOTTOM BORDER  |
+-----------+----------------+
```

## Example Glitch

Glitch effect using BDR:

![BDR_glich_effect](https://github.com/nesbox/TIC-80/assets/26139286/e453e621-ef63-4049-ba69-4e44d9c3bc1b)

```lua
-- 'Change screen offset in every scanline' demo
-- author: Vadim
shake=0
d=4
function TIC()
	if btnp()~=0 then shake=30 end
	if shake>0 then
		poke(0x3FF9+1,math.random(-d,d))
		shake=shake-1
		if shake==0 then memset(0x3FF9,0,2) end
	end
	cls(12)
	print("PRESS ANY KEY TO GLITCH!",54,64)
end

function BDR(row)
	if shake>0 then
		poke(0x3FF9,math.random(-d,d))
	end
end
```
### Example 256 Colors
The following example displays all 256 possible shades of gray. This can be done with any color.

![screen10](https://github.com/nesbox/TIC-80/assets/26139286/8bb0e64b-f05b-48d3-a82a-354f854b6643)

``` lua
-- title:  256 shades of gray
-- author: Marcuss2, fixed by nesbox
-- desc:   Showoff of grayscale
-- script: lua
-- input:  mouse

ADDR = 0x3FC0
palette = 0

function addLight()
 for i=0, 15 do
  for j=0, 2 do
   poke(ADDR+(i*3)+j, palette)
  end
  palette = palette + 1
 end
end

function BDR(scnline)
 if scnline % 8 == 0 then
  addLight()
 end
end

function init()
 for i=0, 16 do
  rect(i*15, 0, 15, 240, i)
 end
end

init()

function TIC()
 palette = 0
end
```
### Example Maximum Colors
The following example displays 4224 colors on screen at once!

![color_test](https://github.com/user-attachments/assets/a90e197d-206b-4004-90bb-855e11d4bee1)

``` lua
-- title:   color test with BDR
-- author:  Bebop
-- desc:    4224 colors on-screen!
-- license: MIT License
-- version: 0.1
-- script:  lua

PAL_MAP=0X3FC0

function set_color(idx, col)
-- col is {red,green,blue}
 poke(PAL_MAP + 3 * idx + 0, col[1])
 poke(PAL_MAP + 3 * idx + 1, col[2])
 poke(PAL_MAP + 3 * idx + 2, col[3])
end

function BDR(scan_line)
 red=(scan_line//12)*21
 green=(scan_line%12)*21
 for j=0,1 do
  vbank(j)
  for i=0,15 do
   blue=i*8+j*128
   set_color(i,{red, green, blue})
  end
 end
end

function TIC()
 for j=0,1 do
  vbank(j)
  for i=0,14 do
   rect(i*16+8*j,0,8,136,i+1)
  end
 end
end
```