//! Port of the data-block header `ID` (`makesdna/DNA_ID.h`, `blenkernel/intern/lib_id.cc`).

/// Data-block types supported by this port (a subset of `ID_Type` in `DNA_ID_enums.h`).
/// The numeric values are this port's own and are what the WASM API reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum IdType {
    Scene = 0,
    Object = 1,
    Mesh = 2,
    Collection = 3,
    Material = 4,
}

impl IdType {
    /// Two-character code from `DNA_ID_enums.h` (`ID_SCE`, `ID_OB`, ...).
    /// Blender also uses it as the name prefix inside `.blend` files ("OBCube").
    pub fn code(self) -> &'static str {
        match self {
            IdType::Scene => "SC",
            IdType::Object => "OB",
            IdType::Mesh => "ME",
            IdType::Collection => "GR",
            IdType::Material => "MA",
        }
    }
}

/// Update flags stored in [`IdHeader::recalc`] and consumed by the depsgraph
/// (see `Main::take_changes`).
pub mod recalc {
    /// Same bit as `ID_RECALC_TRANSFORM`.
    pub const TRANSFORM: u32 = 1 << 0;
    /// Same bit as `ID_RECALC_GEOMETRY`.
    pub const GEOMETRY: u32 = 1 << 1;
    /// Same bit as `ID_RECALC_SHADING`: material values, node trees or material assignment.
    pub const SHADING: u32 = 1 << 7;
    /// Port-specific: links, parenting or collection membership changed.
    /// In Blender this is `DEG_relations_tag_update` rather than an ID flag.
    pub const RELATIONS: u32 = 1 << 31;
    /// Everything; set on newly created data-blocks.
    pub const ALL: u32 = TRANSFORM | GEOMETRY | SHADING | RELATIONS;
}

/// Maximum name length in bytes: `MAX_ID_NAME` (258) minus the 2-byte type prefix and the
/// terminating null.
pub const MAX_NAME_BYTES: usize = 255;

/// The part of `struct ID` this port needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdHeader {
    /// Name without the 2-character type prefix, unique per type within a `Main`.
    pub name: String,
    /// User count (`ID::us`).
    pub users: u32,
    /// Pending update flags (`ID::recalc`), see [`recalc`].
    pub recalc: u32,
    /// Unique per `Main` session, never reused (`ID::session_uid`).
    pub session_uid: u32,
}

/// Truncate `name` to at most `max` bytes without splitting a UTF-8 character.
pub(crate) fn truncate_name(name: &str, max: usize) -> &str {
    if name.len() <= max {
        return name;
    }
    let mut end = max;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}
