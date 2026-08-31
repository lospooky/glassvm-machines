`ttri(x1, y1, x2, y2, x3, y3, u1, v1, u2, v2, u3, v3, [texsrc=0], [chromakey=-1], [z1=0], [z2=0], [z3=0])`

## Parameters

* **x1, y1** : The screen coordinates of the first vertex.
* **x2, y2** : The screen coordinates of the second vertex.
* **x3, y3** : The screen coordinates of the third vertex.
* **u1, v1** : The [UV coordinates](#a-note-on-uv-coordinates) for the first vertex.
* **u2, v2** : The UV coordinates for the second vertex.
* **u3, v3** : The UV coordinates for the third vertex.
* **texsrc** : If set to 0 (default), the triangle's texture is read from [`TILES`](https://github.com/nesbox/TIC-80/wiki/RAM#tilessprites) RAM.<br>
If set to 1, the texture comes from the [`MAP`](https://github.com/nesbox/TIC-80/wiki/RAM#map) RAM.<br>
If set to 2, the texture comes from the [screen](https://github.com/nesbox/TIC-80/wiki/RAM#vram) RAM in the [VBANK](https://github.com/nesbox/TIC-80/wiki/vbank) for which the function isn't being called – that is, a `ttri()` triangle that's drawn to `vbank(0)`'s screen with the `texsrc` value set to 2 will use `vbank(1)`'s screen as its texture and vice versa.
* **chromakey** : The palette index (or array of palette indices 0.80) that will be used as transparent.
* **z1, z2, z3** : Depth parameters for perspective correction and depth buffer

### A note on UV Coordinates

![UV_coordinates_montage](https://github.com/nesbox/TIC-80/assets/26139286/81bb264f-123f-4c6e-a60f-2bb2fbb123fd)

The letters "U" and "V" denote the axes of the 2D texture because "X", "Y" are already used.
These can be thought of as the window inside `TILES` or `MAP` RAM. Note that the sprite sheet or map in this case is treated as a single large image, with U and V addressing its pixels directly, rather than by sprite ID. So for example the top left corner of sprite #2 would be located at `u=16` (horizontal), `v=0` (vertical).

## Description

This function draws a triangle filled with texture from `TILES` RAM, `MAP` RAM, or the screen buffer of the VBANK for which the function isn't being called.

### Depth Buffer

A depth buffer is implemented in ttri when `z1, z2, z3` arguments are set. The depth buffer can be cleared using the [`cls()`](https://github.com/nesbox/TIC-80/wiki/cls) [function](https://github.com/nesbox/TIC-80/blob/main/src/core/draw.c#L397).

## Examples

``` lua
-- title:  triangle demo
-- author: MonstersGoBoom
-- desc:   wiki demo for ttri
-- script: lua
-- input:  gamepad

usize = 32
vsize = 32
function TIC()
  cls(1)
  if btn(0) then usize=usize-1 end
  if btn(1) then usize=usize+1 end
  if btn(2) then vsize=vsize-1 end
  if btn(3) then vsize=vsize+1 end

-- draw a scaling view into the map ram

  ttri(0,0,
         64,0,
         0,64,
         0,0,
         usize,0,
         0,vsize,
         true,
         14)
  ttri(64,0,
         0,64,
         64,64,
         usize,0,
         0,vsize,
         usize,vsize,
         true,
         14)
end
```
[UV show](https://tic80.com/play?cart=554)

![demo 3D](https://user-images.githubusercontent.com/1101448/160176923-acfbc5fe-2dbf-422b-b958-8667b5c19972.gif)

- Sprite x,y,z rotation with ttri [here](https://github.com/nesbox/TIC-80/wiki/Code-examples-and-snippets#ttri-xyz-rotation)

![ttri_rotation_demo](https://github.com/nesbox/TIC-80/assets/26139286/24464fa0-82ed-42de-a3c1-4433e5ff1b4e)
