# shading (HACK-7)

Evaluates blenkernel's material node trees into the values the render pipeline draws with.
Plain Rust library, linked into the single `.wasm` by the top-level crate (`web-migration/app/`),
like depsgraph.

## Output

```rust
pub struct ShadedMaterial {
    pub base_color: [f32; 3],     // linear RGB
    pub alpha: f32,
    pub metallic: f32,
    pub roughness: f32,
    pub emission: [f32; 3],       // Emission Color × Strength
    pub base_color_texture: Option<String>, // image of an Image Texture linked into Base Color
}
```

`ShadedMaterial` is `Serialize`, so the app crate can hand it to JS as JSON.

## Use from the app / render side

```rust
let mut shading = shading::ShadingCache::new();

// Once per frame, after depsgraph.evaluate(). Needs no change flags (depsgraph drains
// take_changes()); returns only the materials whose result changed.
for material in shading.refresh(&bmain) {
    // re-upload uniforms for `material`
}

let m = shading.object_slot(&bmain, object, slot); // per draw (per material slot)
```

If the app crate ever passes the frame's `take_changes()` list along, `apply_changes(&bmain,
&changes)` re-evaluates only materials tagged SHADING. `rebuild(&bmain)` starts from scratch
(e.g. after undo / file load). One-off: `shading::evaluate(&material)` or
`shading::evaluate_tree(&tree)`.

## What is evaluated

| Node | Behaviour |
|------|-----------|
| Material Output | Pulls from the active output's Surface. No output / nothing linked → black |
| Principled BSDF | Base Color, Alpha, Metallic, Roughness (clamped 0..1), Emission Color × Strength |
| Diffuse BSDF | Color, roughness 1, not metallic |
| Emission | Color × Strength as emission, black base |
| Mix Shader | Linear blend of all fields by Fac; empty input = black |
| Mix (color) | Mix, Add, Multiply, Subtract, Screen, Divide, Difference, Exclusion, Darken, Lighten, Overlay; `clamp_result`. Not yet: Burn, Dodge, Soft/Linear Light, Hue, Saturation, Value, Color |
| Math | All Math node operations, using Blender's safe variants (divide by 0 → 0, etc.); `use_clamp` |
| RGB / Value | Output socket value |
| Image Texture | Not sampled: white plus `base_color_texture` when an image is set, magenta when not |

Matches Blender's implicit conversions: color → float is Rec. 709 luminance, vector → float is the
average, and a color or value linked into a shader input becomes an emission of that color. A
material without a node tree uses its viewport color / metallic / roughness. Unknown Math
operations or Mix blend modes give `ShadingError`; `ShadingCache` draws those materials magenta
(`ShadedMaterial::ERROR`), as Blender does for shaders that fail to compile.

## Limits

- Constant folding only: no per-pixel inputs (UVs, positions, normals), so procedural textures
  can't be added without moving to generated WGSL.
- One texture per material, and only when the Image Texture is linked straight into Base Color.
- Normal, Displacement, Volume, IOR, Subsurface, Specular, Transmission, Coat and Sheen are ignored.

## Build / test

```
cargo test                                     # 16 tests
cargo build --target wasm32-unknown-unknown
```
