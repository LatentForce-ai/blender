//! Errors returned by the data model. The C++ code mostly asserts or returns null; here every
//! failure is a value so the WASM layer can turn it into a JavaScript exception.

use std::fmt;

use crate::id::IdType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BkeError {
    /// The handle does not refer to a live data-block (never created, or removed).
    InvalidHandle { id_type: IdType, handle: u32 },
    /// The data-block still has users and cannot be removed.
    StillInUse { id_type: IdType, handle: u32, users: u32 },
    /// Setting this parent would make an object its own ancestor.
    ParentCycle,
    /// Linking this collection would make a collection contain itself.
    CollectionCycle,
    /// An array has the wrong length.
    LengthMismatch { what: &'static str, expected: usize, got: usize },
    /// Mesh topology arrays are inconsistent.
    InvalidTopology(String),
    /// An attribute with this name already exists, or the name is reserved.
    AttributeNameTaken(String),
}

impl fmt::Display for BkeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BkeError::InvalidHandle { id_type, handle } => {
                write!(f, "invalid {id_type:?} handle {handle}")
            }
            BkeError::StillInUse { id_type, handle, users } => {
                write!(f, "{id_type:?} {handle} still has {users} user(s)")
            }
            BkeError::ParentCycle => write!(f, "parenting would create a cycle"),
            BkeError::CollectionCycle => write!(f, "collection would contain itself"),
            BkeError::LengthMismatch { what, expected, got } => {
                write!(f, "{what}: expected length {expected}, got {got}")
            }
            BkeError::InvalidTopology(msg) => write!(f, "invalid mesh topology: {msg}"),
            BkeError::AttributeNameTaken(name) => {
                write!(f, "attribute name \"{name}\" is taken or reserved")
            }
        }
    }
}

impl std::error::Error for BkeError {}
