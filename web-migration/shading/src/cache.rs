//! Per-material results kept between frames, refreshed from blenkernel's change flags.

use std::collections::HashMap;

use blenkernel::{recalc, Change, IdType, Main, MaterialId, ObjectId};

use crate::eval::{evaluate, ShadedMaterial};

/// Evaluated materials of one `Main`. The render pipeline reads from here; the app crate calls
/// [`ShadingCache::apply_changes`] with the changes it took from `Main::take_changes()`.
#[derive(Default)]
pub struct ShadingCache {
    materials: HashMap<MaterialId, ShadedMaterial>,
}

impl ShadingCache {
    pub fn new() -> ShadingCache {
        ShadingCache::default()
    }

    /// Evaluate every material from scratch (first frame, or after undo/file load replaced
    /// `Main`).
    pub fn rebuild(&mut self, bmain: &Main) {
        self.materials = bmain.materials().map(|(id, material)| (id, evaluate_or_error(material))).collect();
    }

    /// Re-evaluate every material and drop removed ones, without needing `take_changes()`
    /// (which the depsgraph drains). Returns the materials whose result changed, so the renderer
    /// only re-uploads those. Cheap: a scene has few materials and each tree folds to constants.
    pub fn refresh(&mut self, bmain: &Main) -> Vec<MaterialId> {
        let mut updated = Vec::new();
        for (id, material) in bmain.materials() {
            let result = evaluate_or_error(material);
            if self.materials.get(&id) != Some(&result) {
                self.materials.insert(id, result);
                updated.push(id);
            }
        }
        self.materials.retain(|&id, _| bmain.material(id).is_ok());
        updated
    }

    /// Re-evaluate materials tagged with SHADING and drop removed ones. Returns the materials
    /// whose result changed, so the renderer only re-uploads those.
    pub fn apply_changes(&mut self, bmain: &Main, changes: &[Change]) -> Vec<MaterialId> {
        let mut updated = Vec::new();
        for change in changes {
            if change.id_type != IdType::Material || change.flags & recalc::SHADING == 0 {
                continue;
            }
            let id = MaterialId(change.handle);
            let Ok(material) = bmain.material(id) else { continue };
            let result = evaluate_or_error(material);
            if self.materials.get(&id) != Some(&result) {
                self.materials.insert(id, result);
                updated.push(id);
            }
        }
        // Removal isn't reported as a change; prune handles that no longer resolve.
        self.materials.retain(|&id, _| bmain.material(id).is_ok());
        updated
    }

    /// The result for a material. Unknown handles get the default surface.
    pub fn get(&self, material: MaterialId) -> &ShadedMaterial {
        self.materials.get(&material).unwrap_or(&ShadedMaterial::DEFAULT_SURFACE)
    }

    /// What to draw an object's material slot with: its material, or Blender's default surface
    /// for empty slots and meshes without materials.
    pub fn object_slot(&self, bmain: &Main, object: ObjectId, slot: usize) -> &ShadedMaterial {
        match bmain.object_material(object, slot) {
            Ok(Some(material)) => self.get(material),
            _ => &ShadedMaterial::DEFAULT_SURFACE,
        }
    }

    pub fn len(&self) -> usize {
        self.materials.len()
    }

    pub fn is_empty(&self) -> bool {
        self.materials.is_empty()
    }
}

fn evaluate_or_error(material: &blenkernel::Material) -> ShadedMaterial {
    evaluate(material).unwrap_or(ShadedMaterial::ERROR)
}
