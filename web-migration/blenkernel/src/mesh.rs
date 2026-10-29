//! Port of `struct Mesh` (`makesdna/DNA_mesh_types.h`, `blenkernel/intern/mesh.cc`).
//!
//! Same struct-of-arrays layout as Blender:
//! - `positions`: one `float3` per vertex (attribute `"position"`)
//! - `edge_verts`: two vertex indices per edge (`".edge_verts"`)
//! - `face_offsets`: face *i* uses corners `face_offsets[i]..face_offsets[i + 1]`
//!   (`face_offset_indices`, length `faces + 1`)
//! - `corner_verts` / `corner_edges`: per face corner, its vertex and the edge to the next corner
//!   (`".corner_vert"`, `".corner_edge"`)
//!
//! Derived data (normals, triangles, bounds) is cached lazily and dropped on any write, like the
//! caches on `MeshRuntime` in C++.

use std::cell::OnceCell;
use std::ops::Range;

use glam::Vec3;

use crate::attribute::{AttrDomain, Attribute, AttributeData, AttributeStorage};
use crate::error::BkeError;
use crate::id::IdHeader;
use crate::material::MaterialId;
use crate::{mesh_calc_edges, mesh_normals, mesh_tessellate};

/// Built-in face attribute marking flat-shaded faces; absent means every face is smooth.
pub const SHARP_FACE: &str = "sharp_face";
/// Built-in edge attribute marking edges that split normals.
pub const SHARP_EDGE: &str = "sharp_edge";

/// Handle of a mesh in a [`crate::Main`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Clone, Debug, Default)]
struct MeshRuntime {
    face_normals: OnceCell<Vec<Vec3>>,
    vert_normals: OnceCell<Vec<Vec3>>,
    corner_tris: OnceCell<Vec<[u32; 3]>>,
    bounds: OnceCell<Option<Bounds>>,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub id: IdHeader,
    positions: Vec<Vec3>,
    edge_verts: Vec<[i32; 2]>,
    face_offsets: Vec<i32>,
    corner_verts: Vec<i32>,
    corner_edges: Vec<i32>,
    attributes: AttributeStorage,
    /// Material slots (`Mesh::mat`); `None` is an empty slot. Faces pick a slot with the
    /// `material_index` attribute.
    pub(crate) materials: Vec<Option<MaterialId>>,
    runtime: MeshRuntime,
}

/// Face attribute selecting the material slot (`"material_index"` in Blender).
pub const MATERIAL_INDEX: &str = "material_index";

impl Default for Mesh {
    fn default() -> Self {
        Mesh {
            id: IdHeader::default(),
            positions: Vec::new(),
            edge_verts: Vec::new(),
            face_offsets: vec![0],
            corner_verts: Vec::new(),
            corner_edges: Vec::new(),
            attributes: AttributeStorage::default(),
            materials: Vec::new(),
            runtime: MeshRuntime::default(),
        }
    }
}

impl Mesh {
    /// An empty mesh with no elements.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a mesh from vertex positions and faces; edges are calculated.
    ///
    /// `face_offsets` has `faces + 1` entries starting at 0 and ending at `corner_verts.len()`.
    pub fn from_faces(
        positions: Vec<Vec3>,
        face_offsets: Vec<i32>,
        corner_verts: Vec<i32>,
    ) -> Result<Mesh, BkeError> {
        validate_faces(positions.len(), &face_offsets, &corner_verts)?;
        let (edge_verts, corner_edges) = mesh_calc_edges::calc_edges(&face_offsets, &corner_verts);
        Ok(Mesh {
            positions,
            edge_verts,
            face_offsets,
            corner_verts,
            corner_edges,
            ..Mesh::default()
        })
    }

    pub fn verts_num(&self) -> usize {
        self.positions.len()
    }

    pub fn edges_num(&self) -> usize {
        self.edge_verts.len()
    }

    pub fn faces_num(&self) -> usize {
        self.face_offsets.len().saturating_sub(1)
    }

    pub fn corners_num(&self) -> usize {
        self.corner_verts.len()
    }

    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    pub fn edges(&self) -> &[[i32; 2]] {
        &self.edge_verts
    }

    pub fn face_offsets(&self) -> &[i32] {
        &self.face_offsets
    }

    pub fn corner_verts(&self) -> &[i32] {
        &self.corner_verts
    }

    pub fn corner_edges(&self) -> &[i32] {
        &self.corner_edges
    }

    /// Corner range of one face.
    pub fn face(&self, face: usize) -> Range<usize> {
        self.face_offsets[face] as usize..self.face_offsets[face + 1] as usize
    }

    /// Vertex indices of one face, in winding order.
    pub fn face_verts(&self, face: usize) -> &[i32] {
        &self.corner_verts[self.face(face)]
    }

    /// Mutable positions; drops position-dependent caches.
    pub fn positions_mut(&mut self) -> &mut [Vec3] {
        self.tag_positions_changed();
        &mut self.positions
    }

    /// Replace all positions (same vertex count).
    pub fn set_positions(&mut self, positions: Vec<Vec3>) -> Result<(), BkeError> {
        if positions.len() != self.positions.len() {
            return Err(BkeError::LengthMismatch {
                what: "positions",
                expected: self.positions.len(),
                got: positions.len(),
            });
        }
        self.positions = positions;
        self.tag_positions_changed();
        Ok(())
    }

    /// Shade every face smooth or flat (`bke::mesh_smooth_set` without `keep_sharp_edges`):
    /// clears sharp edges and faces, then marks all faces sharp when `use_smooth` is false.
    pub fn set_smooth(&mut self, use_smooth: bool) {
        self.attributes.remove(SHARP_EDGE);
        self.attributes.remove(SHARP_FACE);
        if !use_smooth {
            let sharp_faces = Attribute {
                name: SHARP_FACE.to_owned(),
                domain: AttrDomain::Face,
                data: AttributeData::Bool(vec![true; self.faces_num()]),
            };
            self.add_attribute(sharp_faces).expect("one value per face, name just removed");
        }
    }

    /// Call after editing positions directly (`Mesh::tag_positions_changed`).
    /// Triangulation only depends on topology, so it is kept.
    pub fn tag_positions_changed(&mut self) {
        self.runtime.face_normals = OnceCell::new();
        self.runtime.vert_normals = OnceCell::new();
        self.runtime.bounds = OnceCell::new();
    }

    pub fn face_normals(&self) -> &[Vec3] {
        self.runtime.face_normals.get_or_init(|| {
            mesh_normals::calc_face_normals(&self.positions, &self.face_offsets, &self.corner_verts)
        })
    }

    pub fn vert_normals(&self) -> &[Vec3] {
        self.runtime.vert_normals.get_or_init(|| {
            mesh_normals::calc_vert_normals(
                &self.positions,
                &self.face_offsets,
                &self.corner_verts,
                self.face_normals(),
            )
        })
    }

    /// Triangles as corner indices (`Mesh::corner_tris`).
    pub fn corner_tris(&self) -> &[[u32; 3]] {
        self.runtime
            .corner_tris
            .get_or_init(|| mesh_tessellate::calc_corner_tris(&self.face_offsets))
    }

    /// Triangles as vertex indices, ready for a GPU index buffer.
    pub fn tri_vert_indices(&self) -> Vec<u32> {
        self.corner_tris()
            .iter()
            .flat_map(|tri| tri.map(|corner| self.corner_verts[corner as usize] as u32))
            .collect()
    }

    /// Axis-aligned bounds of all vertices, `None` for a mesh without vertices.
    pub fn bounds(&self) -> Option<Bounds> {
        *self.runtime.bounds.get_or_init(|| {
            let first = *self.positions.first()?;
            let (min, max) = self
                .positions
                .iter()
                .fold((first, first), |(min, max), p| (min.min(*p), max.max(*p)));
            Some(Bounds { min, max })
        })
    }

    /// Number of elements in a domain.
    pub fn domain_size(&self, domain: AttrDomain) -> usize {
        match domain {
            AttrDomain::Point => self.verts_num(),
            AttrDomain::Edge => self.edges_num(),
            AttrDomain::Face => self.faces_num(),
            AttrDomain::Corner => self.corners_num(),
        }
    }

    pub fn attributes(&self) -> &AttributeStorage {
        &self.attributes
    }

    /// Add a generic attribute; its length must match the domain size.
    pub fn add_attribute(&mut self, attribute: Attribute) -> Result<(), BkeError> {
        let expected = self.domain_size(attribute.domain);
        if attribute.data.len() != expected {
            return Err(BkeError::LengthMismatch {
                what: "attribute",
                expected,
                got: attribute.data.len(),
            });
        }
        self.attributes.add(attribute)
    }

    pub fn remove_attribute(&mut self, name: &str) -> bool {
        self.attributes.remove(name)
    }

    /// Material slots. Change them through `Main::mesh_set_materials` so user counts stay right.
    pub fn materials(&self) -> &[Option<MaterialId>] {
        &self.materials
    }

    /// Material slot index per face; all 0 when the attribute doesn't exist.
    pub fn material_indices(&self) -> Vec<i32> {
        match self.attributes.lookup(MATERIAL_INDEX).map(|a| &a.data) {
            Some(AttributeData::Int32(values)) => values.clone(),
            _ => vec![0; self.faces_num()],
        }
    }

    /// Set the material slot index of every face (one non-negative value per face).
    /// Indices past the last slot are allowed; drawing clamps them like Blender does.
    pub fn set_material_indices(&mut self, indices: Vec<i32>) -> Result<(), BkeError> {
        if indices.len() != self.faces_num() {
            return Err(BkeError::LengthMismatch {
                what: "material_index",
                expected: self.faces_num(),
                got: indices.len(),
            });
        }
        if indices.iter().any(|&i| i < 0) {
            return Err(BkeError::InvalidValue("material_index must be >= 0".into()));
        }
        self.attributes.remove(MATERIAL_INDEX);
        self.add_attribute(Attribute {
            name: MATERIAL_INDEX.into(),
            domain: AttrDomain::Face,
            data: AttributeData::Int32(indices),
        })
    }

    /// Material slot per triangle of [`Mesh::corner_tris`], clamped to the last slot
    /// (0 when there are no slots). Lets a renderer split its index buffer by material.
    pub fn tri_material_indices(&self) -> Vec<u32> {
        let max_slot = self.materials.len().saturating_sub(1) as i32;
        let face_indices = self.material_indices();
        let mut result = Vec::with_capacity(self.corner_tris().len());
        for (face, f) in self.face_offsets.windows(2).enumerate() {
            let tris = (f[1] - f[0] - 2).max(0) as usize;
            let slot = face_indices[face].clamp(0, max_slot) as u32;
            result.extend(std::iter::repeat(slot).take(tris));
        }
        result
    }

    /// Check that all topology arrays are consistent (a small `BKE_mesh_validate`).
    pub fn validate(&self) -> Result<(), BkeError> {
        validate_faces(self.positions.len(), &self.face_offsets, &self.corner_verts)?;
        let verts = self.positions.len() as i32;
        if let Some(edge) = self
            .edge_verts
            .iter()
            .find(|e| e.iter().any(|&v| v < 0 || v >= verts) || e[0] == e[1])
        {
            return Err(BkeError::InvalidTopology(format!("bad edge {edge:?}")));
        }
        if self.corner_edges.len() != self.corner_verts.len() {
            return Err(BkeError::LengthMismatch {
                what: "corner_edges",
                expected: self.corner_verts.len(),
                got: self.corner_edges.len(),
            });
        }
        let edges = self.edge_verts.len() as i32;
        if self.corner_edges.iter().any(|&e| e < 0 || e >= edges) {
            return Err(BkeError::InvalidTopology("corner edge out of range".into()));
        }
        Ok(())
    }
}

fn validate_faces(verts_num: usize, face_offsets: &[i32], corner_verts: &[i32]) -> Result<(), BkeError> {
    if face_offsets.first() != Some(&0) {
        return Err(BkeError::InvalidTopology("face_offsets must start with 0".into()));
    }
    if *face_offsets.last().unwrap() as usize != corner_verts.len() {
        return Err(BkeError::InvalidTopology(
            "last face offset must equal the number of corners".into(),
        ));
    }
    if let Some(i) = face_offsets.windows(2).position(|f| f[1] - f[0] < 3) {
        return Err(BkeError::InvalidTopology(format!("face {i} has fewer than 3 corners")));
    }
    if corner_verts.iter().any(|&v| v < 0 || v as usize >= verts_num) {
        return Err(BkeError::InvalidTopology("corner vertex out of range".into()));
    }
    Ok(())
}
