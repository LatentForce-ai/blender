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
    /// The shader node type is not supported.
    UnknownNodeType(String),
    /// The node handle does not refer to a live node in this tree.
    InvalidNode(u32),
    /// The node has no socket with this identifier.
    UnknownSocket(String),
    /// A value or link does not fit the socket type.
    SocketTypeMismatch(String),
    /// Linking would make the node graph cyclic.
    NodeCycle,
    /// The node has no setting with this name.
    UnknownProperty(String),
    /// A material has no node tree.
    NoNodeTree(u32),
    /// Invalid value passed in.
    InvalidValue(String),
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
            BkeError::UnknownNodeType(idname) => write!(f, "unsupported node type \"{idname}\""),
            BkeError::InvalidNode(node) => write!(f, "invalid node handle {node}"),
            BkeError::UnknownSocket(msg) => write!(f, "unknown socket: {msg}"),
            BkeError::SocketTypeMismatch(msg) => write!(f, "socket type mismatch: {msg}"),
            BkeError::NodeCycle => write!(f, "link would create a cycle in the node tree"),
            BkeError::UnknownProperty(key) => write!(f, "unknown node property \"{key}\""),
            BkeError::NoNodeTree(material) => write!(f, "material {material} has no node tree"),
            BkeError::InvalidValue(msg) => write!(f, "invalid value: {msg}"),
        }
    }
}

impl std::error::Error for BkeError {}
