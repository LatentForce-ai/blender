//! Thin Rust port of Blender's `blenkernel` data model, compiled to WebAssembly.
//!
//! Covers `Main`, the `ID` header, `Mesh` (with edges, normals, triangulation and primitives),
//! `Object` transforms, `Collection` and `Scene`. See `PLAN.md` for the scope and for the mapping
//! of every module back to the C++ sources in `source/blender/`.

pub mod attribute;
pub mod collection;
pub mod error;
pub mod id;
pub mod main_db;
pub mod mesh;
pub mod mesh_calc_edges;
pub mod mesh_normals;
pub mod mesh_tessellate;
pub mod object;
pub mod primitives;
pub mod scene;
pub mod wasm;

pub use attribute::{AttrDomain, Attribute, AttributeData};
pub use collection::{Collection, CollectionId};
pub use error::BkeError;
pub use id::{recalc, IdHeader, IdType};
pub use main_db::{Change, CollectionRef, Main};
pub use mesh::{Bounds, Mesh, MeshId};
pub use object::{Object, ObjectData, ObjectId, RotationMode};
pub use scene::{Scene, SceneId};
