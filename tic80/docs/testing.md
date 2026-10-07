# TIC-80 bundle testing

Core tests parse and execute the real upstream Gecko `.tic` cartridge, compare
deterministic native runs, and prove state-only snapshot continuation.
Verifier tests analyze the real cartridge and reject syntactically invalid Lua
without executing it. Adversarial cases cover quoted and long-bracket strings,
short and equals-delimited long comments, local shadowing, table/nested
near-matches, malformed declarations, and accepted top-level declarations and
assignments. Bundle tests cover contract/body resolution, strict native config
and directly deserialized stimuli, real-cart native-event normalization and
causality, one-shot lifecycle closure, bounded runtime/sink failures, atomic
snapshot rejection, scheduled-input replay, reset, and exact session resume.
Snapshot tests additionally cover the version-2 state schema, explicit
incompatible-version rejection, absence of trace and input-history vectors,
and continuation in a fresh session without persisted observational history.

## Gamepad API conformance

The `gamepad_queries_match_held_pressed_and_repeat_semantics` core test is
checked against the pinned official `btn` and `btnp` references in
`docs/reference/corpus/wiki/`. It verifies that `btn(id)` reports held state,
`btn()` returns the current 32-bit mask, `btnp(id)` reports a press edge,
`btnp()` returns the newly-pressed mask, and `btnp(id, hold, period)` repeats
at the configured frame interval. The input sequence includes simultaneous
buttons, a held press, release, and a later re-press.

The smoke artifact rebuilds offline from its byte-identical upstream source and
both fixture and reference checksum manifests cover their corpora exactly.

## Graphics primitive conformance

The `graphics_primitives_match_pixel_coordinates_clipping_and_default_clear`
core test asserts exact RGBA pixels for the pinned documented behavior of
`cls`, `pix`, `line`, and `rect`. It covers default and explicit clear colors,
pixel reads and writes, inclusive line endpoints, draw ordering, negative and
screen-edge clipping, and the bottom-right display coordinate.

The `sprites_maps_and_video_banks_match_memory_and_composition_semantics` core
test checks the pinned RAM sprite layout and map-cell addressing, color-key
transparency, bank-local VRAM reads/writes (including nibble and block memory
operations), the previous-bank return value, and bank-1 palette/overlay
composition over bank 0.

The `sprite_scale_flip_rotation_and_composite_layout_match_reference` core
test pins exact pixels for scaled, flipped, rotated, combined-transform, and
two-tile composite sprites. It also verifies that nonpositive sprite scales
draw nothing and that rotation is applied before flipping, as specified by the
pinned `spr` reference.

The `map_remap_receives_cell_coordinates_and_applies_tile_transforms` core test
checks callback invocation order and map-cell coordinates, single-value tile
replacement, flip/rotation tuple results, unchanged map storage, and exact
rendered pixels. It also verifies hiding a tile by remapping to the transparent
tile selected by `colorkey`, without inventing a special callback sentinel.
`map_wraps_lookup_and_callback_coordinates_at_native_boundaries` additionally
checks that negative map positions wrap in both tile lookup and callback
arguments, using the pinned upstream `drawMap` behavior.

`clipped_clear_maps_color_once_and_negative_colorkey_keeps_color_fifteen`
checks that `cls` respects the intersected clip rectangle, including negative
clip origins, applies palette remapping only once, and leaves pixels outside
the clip unchanged. It also checks that explicit `-1` sprite/map colorkeys do
not incorrectly hide color 15.

## Palette-map conformance

The `palette_map_swaps_drawn_colors_and_stays_local_to_each_vram_bank` core
test checks the documented 16-entry nibble map at `0x3FF0`: primitive and clear
writes store mapped indices, sprite colors are remapped while colorkey
transparency still checks the original sprite index, and bank 1 uses its own
map and palette. Palette-map reset is the identity mapping in each bank. TIC-80
does not expose a built-in `pal()` API in the pinned API reference; the wiki's
`pal()` example is a Lua helper implemented using `poke4`.

## Border callback conformance

The `bdr_runs_after_tic_and_selects_palette_per_display_scanline` core test
checks that `TIC()` runs before `BDR(0..143)`, that callback rows are delivered
in order, and that rows 4 through 139 select display rows 0 through 135. RGB
palette values are captured after each corresponding callback, so later
palette writes do not recolor earlier scanlines. The four top and bottom
border callbacks run, but border pixels are outside the framebuffer contract.
`OVR()` and border rendering remain unsupported.
This is palette-only raster conformance: screen-RAM changes, scrolling offsets,
and changing overlay composition during BDR are not captured per scanline and
must not be claimed as full upstream raster-effect support. All callbacks share
the frame instruction budget rather than receiving independent row budgets.

## Textured-triangle conformance

The `textured_triangles_sample_image_map_and_other_vbank_with_clip_and_colorkey`
core test pins pixel-center affine sampling from the sprite/tile image, map
tiles, and the opposite VRAM bank. It asserts exact pixels at sample points,
clipping bounds, and transparent color-key behavior. It also
compares sprite and triangle reads of the same native tile, and uses the native
eight-pixel intra-tile row stride rather than treating tile RAM as a flat image.
The rasterizer uses the
reference pixel-center barycentric coverage rule and wraps texture coordinates
within each source's documented dimensions. `z1/z2/z3` perspective correction
and depth-buffer behavior are not approximated: nonzero depth values fail
explicitly until that behavior is implemented.

The `ttri_rejects_depth_parameters_until_depth_buffer_semantics_are_implemented`
test protects that explicit unsupported boundary.

The source review uses the pinned upstream revision
`4aba09c98f1e5028b82765be1647677b08d35942`:
[draw.c](https://github.com/nesbox/TIC-80/blob/4aba09c98f1e5028b82765be1647677b08d35942/src/core/draw.c)
for map wrapping, clear/clipping, and colorkey behavior;
[tilesheet.h](https://github.com/nesbox/TIC-80/blob/4aba09c98f1e5028b82765be1647677b08d35942/src/tilesheet.h)
for sheet-to-tile addressing. Default 4-bpp sampling is tested; other blit
segments/BPP modes are not part of this conformance claim.
