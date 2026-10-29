//! The single WebAssembly module of the Blender web migration. It links every ported module
//! (blenkernel, depsgraph, render, and shading as it lands) so they share one scene in one
//! memory, and it is the only `wasm-pack` build. JavaScript gets `BlenderMain` (blenkernel),
//! `Depsgraph` and `Viewport` (render) from here.

use blenkernel::SceneId;
use blenkernel::wasm::BlenderMain;
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

/// Advances one frame: re-evaluates what the scene's edits made out of date, then draws it.
#[wasm_bindgen]
pub fn tick(
    bmain: &mut BlenderMain,
    depsgraph: &mut SceneDepsgraph,
    viewport: &mut Viewport,
) -> Result<(), JsError> {
    let updates = depsgraph.inner.evaluate(bmain.main_mut())?;
    viewport.apply_updates(&updates);
    viewport.draw(bmain.main(), &depsgraph.inner)
}
