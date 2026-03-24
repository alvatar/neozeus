# vello orange fix

## Folder name
`vendor-patches/vello-orange-fix`

## What is in here

Minimal patch set needed for the renderer-side orange fix:

1. `01-bevy_vello-vello_rendertarget.wgsl.patch`
2. `02-vello_shaders-fine.wgsl.patch`
3. `03-vello_shaders-cpu-fine.rs.patch`

## Rationale

There were two required fixes.

### A. `vello_shaders/fine.wgsl`
Vello fine pass was effectively mixing packed/sampled colors with the wrong color contract for this path.
The fix is:
- decode packed/sampled colors as premultiplied sRGB inputs
- composite in linear light
- encode final straight output back to sRGB

Without this, the source canvas can already drift from the requested orange under partial coverage.

### B. `bevy_vello/vello_rendertarget.wgsl`
The final canvas present pass was sampling an already-rasterized texture with `textureSample(...)`.
That allows filtered neighbor bleed on thin edges/text.
The fix is:
- replace filtered sampling with exact texel fetch via `textureLoad(...)`

Without this, even a correct source canvas can shift visibly at presentation time.

### C. `vello_shaders/src/cpu/fine.rs`
This is not the GPU runtime fix itself.
It keeps the CPU analog/tests aligned with the shader contract and preserves regression coverage.

## Target versions

These patches were generated against:
- `bevy_vello 0.13.1`
- `vello_shaders 0.7.0`

## How to apply

Apply each patch inside the corresponding crate root.

### `bevy_vello`
From the `bevy_vello` repo/crate root:

```bash
git apply /path/to/vendor-patches/vello-orange-fix/01-bevy_vello-vello_rendertarget.wgsl.patch
```

### `vello_shaders`
From the `vello_shaders` repo/crate root:

```bash
git apply /path/to/vendor-patches/vello-orange-fix/02-vello_shaders-fine.wgsl.patch
git apply /path/to/vendor-patches/vello-orange-fix/03-vello_shaders-cpu-fine.rs.patch
```

If `git apply` fails due to version drift, inspect and port manually. The diffs are intentionally narrow.

## What is intentionally not included

Not part of the fix:
- the `COPY_SRC` change in `bevy_vello/src/render/systems.rs`
- any NeoZeus-side compositor/capture/debug code
- any experiment/repro assets

Those were support/debug changes, not the minimal renderer fix.

## Sanity check after apply

- `bevy_vello/shaders/vello_rendertarget.wgsl` should contain `textureLoad(` in the fragment path
- `vello_shaders/shader/fine.wgsl` should contain:
  - `premul_srgb_to_linear`
  - `linear_rgba_to_srgba`
- `vello_shaders/src/cpu/fine.rs` should contain the same CPU-side helpers/tests
