//! The single WebAssembly module of the Blender web migration. It links every ported module
//! (blenkernel, depsgraph, render and shading) so they share one scene in one memory, and it
//! is the only `wasm-pack` build. JavaScript gets `BlenderMain` (blenkernel), `Depsgraph`,
//! `Shading` and `Viewport` (render) from here.

use blenkernel::wasm::BlenderMain;
use blenkernel::{MaterialId, SceneId};
use render::Viewport;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

/// The dependency graph of one scene, kept across frames.
#[wasm_bindgen(js_name = Depsgraph)]
pub struct SceneDepsgraph {
    inner: depsgraph::Depsgraph,
}

#[wasm_bindgen(js_class = Depsgraph)]
impl SceneDepsgraph {
    #[wasm_bindgen(constructor)]
    pub fn new(scene: u32) -> SceneDepsgraph {
        SceneDepsgraph {
            inner: depsgraph::Depsgraph::new(SceneId(scene)),
        }
    }
}

/// Evaluated node-tree results of every material (HACK-7), kept across frames. The viewport
/// draws them in Material Preview (`viewport.set_shading_type("MATERIAL")`); Solid mode draws
/// viewport values.
#[wasm_bindgen]
pub struct Shading {
    inner: shading::ShadingCache,
    /// Materials whose result changed in the last `tick`.
    updated: Vec<u32>,
}

#[wasm_bindgen]
impl Shading {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Shading {
        Shading {
            inner: shading::ShadingCache::new(),
            updated: Vec::new(),
        }
    }

    /// Material handles whose evaluated result changed in the last `tick`.
    pub fn updated(&self) -> Vec<u32> {
        self.updated.clone()
    }

    /// `[r, g, b, alpha, metallic, roughness, emission_r, emission_g, emission_b]` of a
    /// material's node tree as of the last `tick`. Unknown handles get the default surface.
    pub fn material(&self, material: u32) -> Vec<f32> {
        let m = self.inner.get(MaterialId(material));
        let [r, g, b] = m.base_color;
        let [er, eg, eb] = m.emission;
        vec![r, g, b, m.alpha, m.metallic, m.roughness, er, eg, eb]
    }

    /// Image name of the Image Texture feeding the material's base color, if any.
    pub fn material_texture(&self, material: u32) -> Option<String> {
        self.inner.get(MaterialId(material)).base_color_texture.clone()
    }
}

impl Default for Shading {
    fn default() -> Self {
        Shading::new()
    }
}

/// Advances one frame: re-evaluates what the scene's edits made out of date, re-evaluates
/// changed materials' node trees, then draws the scene.
#[wasm_bindgen]
pub fn tick(
    bmain: &mut BlenderMain,
    depsgraph: &mut SceneDepsgraph,
    shading: &mut Shading,
    viewport: &mut Viewport,
) -> Result<(), JsError> {
    let updates = depsgraph.inner.evaluate(bmain.main_mut())?;
    // The depsgraph drained the change list, so shading checks every material itself.
    shading.updated = shading.inner.refresh(bmain.main()).into_iter().map(|m| m.0).collect();
    viewport.apply_updates(&updates);
    viewport.draw(bmain.main(), &depsgraph.inner, &shading.inner)
}
