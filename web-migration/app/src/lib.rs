//! The single WebAssembly module of the Blender web migration. It links every ported module
//! (blenkernel, render, and depsgraph and shading as they land) so they share one scene in
//! one memory, and it is the only `wasm-pack` build. JavaScript gets `BlenderMain`
//! (blenkernel) and `Viewport` (render) from here.

use blenkernel::SceneId;
use blenkernel::wasm::BlenderMain;
use render::Viewport;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

/// Advances one frame: hands the scene's pending changes to each module, then draws `scene`.
#[wasm_bindgen]
pub fn tick(bmain: &mut BlenderMain, viewport: &mut Viewport, scene: u32) -> Result<(), JsError> {
    let changes = bmain.main_mut().take_changes();
    viewport.apply_changes(&changes);
    viewport.draw(bmain.main(), SceneId(scene))
}
