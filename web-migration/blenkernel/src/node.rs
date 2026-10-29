//! Shader node trees (`makesdna/DNA_node_types.h`: `bNodeTree`, `bNode`, `bNodeSocket`,
//! `bNodeLink`; `blenkernel/intern/node.cc`).
//!
//! blenkernel only *stores* the graph: nodes, socket default values and links. Evaluating it
//! (turning it into shading values or shader code) is the shading module's job.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::error::BkeError;
use crate::node_types;

/// Handle of a node inside one [`NodeTree`]. Never reused within that tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct NodeId(pub u32);

/// `eNodeSocketDatatype`, for the socket types shader nodes use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SocketType {
    Float,
    Vector,
    Color,
    Shader,
    Bool,
}

/// A socket's `default_value`: used when nothing is linked to an input, and the stored value
/// of RGB / Value nodes' outputs. Shader sockets have no value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SocketValue {
    None,
    Float(f32),
    Vector([f32; 3]),
    Color([f32; 4]),
    Bool(bool),
}

impl SocketValue {
    /// Build a value for `socket_type` from plain floats (1 for float/bool, 3 for vector,
    /// 4 for color). Used by the JavaScript API.
    pub fn from_floats(socket_type: SocketType, values: &[f32]) -> Option<SocketValue> {
        Some(match (socket_type, values) {
            (SocketType::Float, [v]) => SocketValue::Float(*v),
            (SocketType::Bool, [v]) => SocketValue::Bool(*v != 0.0),
            (SocketType::Vector, [x, y, z]) => SocketValue::Vector([*x, *y, *z]),
            (SocketType::Color, [r, g, b, a]) => SocketValue::Color([*r, *g, *b, *a]),
            _ => return None,
        })
    }

    pub fn to_floats(&self) -> Vec<f32> {
        match *self {
            SocketValue::None => Vec::new(),
            SocketValue::Float(v) => vec![v],
            SocketValue::Bool(v) => vec![if v { 1.0 } else { 0.0 }],
            SocketValue::Vector(v) => v.to_vec(),
            SocketValue::Color(v) => v.to_vec(),
        }
    }

    fn matches(&self, socket_type: SocketType) -> bool {
        matches!(
            (self, socket_type),
            (SocketValue::None, SocketType::Shader)
                | (SocketValue::Float(_), SocketType::Float)
                | (SocketValue::Bool(_), SocketType::Bool)
                | (SocketValue::Vector(_), SocketType::Vector)
                | (SocketValue::Color(_), SocketType::Color)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketInOut {
    Input,
    Output,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Socket {
    /// Unique per node and direction, e.g. "Base Color", "Value_001".
    pub identifier: String,
    /// Display name; may repeat (the Math node has three inputs named "Value").
    pub name: String,
    #[serde(rename = "type")]
    pub socket_type: SocketType,
    pub default_value: SocketValue,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Node {
    pub id: NodeId,
    /// Unique within the tree ("Principled BSDF", "Principled BSDF.001").
    pub name: String,
    /// Node type, e.g. "ShaderNodeBsdfPrincipled".
    pub idname: String,
    /// Position in the node editor.
    pub location: [f32; 2],
    pub inputs: Vec<Socket>,
    pub outputs: Vec<Socket>,
    /// Non-socket settings such as the Math node's "operation".
    pub properties: BTreeMap<String, String>,
}

impl Node {
    pub fn input(&self, identifier: &str) -> Option<&Socket> {
        self.inputs.iter().find(|s| s.identifier == identifier)
    }

    pub fn output(&self, identifier: &str) -> Option<&Socket> {
        self.outputs.iter().find(|s| s.identifier == identifier)
    }

    fn socket_mut(&mut self, in_out: SocketInOut, identifier: &str) -> Option<&mut Socket> {
        let sockets = match in_out {
            SocketInOut::Input => &mut self.inputs,
            SocketInOut::Output => &mut self.outputs,
        };
        sockets.iter_mut().find(|s| s.identifier == identifier)
    }
}

/// A connection from an output socket to an input socket (`bNodeLink`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Link {
    pub from_node: NodeId,
    pub from_socket: String,
    pub to_node: NodeId,
    pub to_socket: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeTree {
    nodes: Vec<Option<Node>>,
    links: Vec<Link>,
    active_output: Option<NodeId>,
}

fn invalid_node(node: NodeId) -> BkeError {
    BkeError::InvalidNode(node.0)
}

fn unknown_socket(node: &Node, in_out: SocketInOut, identifier: &str) -> BkeError {
    let side = match in_out {
        SocketInOut::Input => "input",
        SocketInOut::Output => "output",
    };
    BkeError::UnknownSocket(format!("{} has no {side} \"{identifier}\"", node.name))
}

impl NodeTree {
    pub fn new() -> NodeTree {
        NodeTree::default()
    }

    /// `bke::node_add_static_node`: add a node of a supported type with its declared sockets
    /// and default values. The first Material Output added becomes the active output.
    pub fn add_node(&mut self, idname: &str) -> Result<NodeId, BkeError> {
        let decl = node_types::declaration(idname)
            .ok_or_else(|| BkeError::UnknownNodeType(idname.to_string()))?;
        let id = NodeId(self.nodes.len() as u32);
        let to_socket = |d: &node_types::SocketDecl| Socket {
            identifier: d.identifier.to_string(),
            name: d.name.to_string(),
            socket_type: d.socket_type,
            default_value: d.default_value,
        };
        let node = Node {
            id,
            name: self.unique_node_name(decl.label),
            idname: decl.idname.to_string(),
            location: [0.0, 0.0],
            inputs: decl.inputs.iter().map(to_socket).collect(),
            outputs: decl.outputs.iter().map(to_socket).collect(),
            properties: decl
                .properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        self.nodes.push(Some(node));
        if idname == "ShaderNodeOutputMaterial" && self.active_output.is_none() {
            self.active_output = Some(id);
        }
        Ok(id)
    }

    fn unique_node_name(&self, label: &str) -> String {
        let taken = |name: &str| self.nodes().any(|(_, n)| n.name == name);
        if !taken(label) {
            return label.to_string();
        }
        (1u32..)
            .map(|i| format!("{label}.{i:03}"))
            .find(|name| !taken(name))
            .expect("unbounded range")
    }

    /// Remove a node and every link touching it.
    pub fn remove_node(&mut self, node: NodeId) -> Result<(), BkeError> {
        self.nodes
            .get_mut(node.0 as usize)
            .and_then(Option::take)
            .ok_or(invalid_node(node))?;
        self.links.retain(|l| l.from_node != node && l.to_node != node);
        if self.active_output == Some(node) {
            let next_output = self
                .nodes()
                .find(|(_, n)| n.idname == "ShaderNodeOutputMaterial")
                .map(|(id, _)| id);
            self.active_output = next_output;
        }
        Ok(())
    }

    pub fn node(&self, node: NodeId) -> Result<&Node, BkeError> {
        self.nodes.get(node.0 as usize).and_then(Option::as_ref).ok_or(invalid_node(node))
    }

    fn node_mut(&mut self, node: NodeId) -> Result<&mut Node, BkeError> {
        self.nodes.get_mut(node.0 as usize).and_then(Option::as_mut).ok_or(invalid_node(node))
    }

    /// Live nodes in creation order.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().filter_map(|n| n.as_ref().map(|n| (n.id, n)))
    }

    pub fn node_by_name(&self, name: &str) -> Option<NodeId> {
        self.nodes().find(|(_, n)| n.name == name).map(|(id, _)| id)
    }

    pub fn set_location(&mut self, node: NodeId, x: f32, y: f32) -> Result<(), BkeError> {
        self.node_mut(node)?.location = [x, y];
        Ok(())
    }

    /// Set a socket's default value. The value must match the socket type.
    pub fn set_socket_value(
        &mut self,
        node: NodeId,
        in_out: SocketInOut,
        identifier: &str,
        value: SocketValue,
    ) -> Result<(), BkeError> {
        let found = self.node_mut(node)?;
        let error = unknown_socket(found, in_out, identifier);
        let socket = found.socket_mut(in_out, identifier).ok_or(error)?;
        if socket.socket_type == SocketType::Shader || !value.matches(socket.socket_type) {
            return Err(BkeError::SocketTypeMismatch(format!(
                "\"{identifier}\" is {:?}, got {value:?}",
                socket.socket_type
            )));
        }
        socket.default_value = value;
        Ok(())
    }

    /// Set a node setting such as Math's "operation". Only declared settings exist.
    pub fn set_property(&mut self, node: NodeId, key: &str, value: &str) -> Result<(), BkeError> {
        let found = self.node_mut(node)?;
        let slot = found
            .properties
            .get_mut(key)
            .ok_or_else(|| BkeError::UnknownProperty(key.to_string()))?;
        *slot = value.to_string();
        Ok(())
    }

    /// `bke::node_add_link`. An input takes one link, so an existing link into `to_socket` is
    /// replaced, as in Blender. Refuses shader → non-shader links and cycles.
    pub fn add_link(
        &mut self,
        from_node: NodeId,
        from_socket: &str,
        to_node: NodeId,
        to_socket: &str,
    ) -> Result<(), BkeError> {
        let from = self.node(from_node)?;
        let from_type = from
            .output(from_socket)
            .ok_or_else(|| unknown_socket(from, SocketInOut::Output, from_socket))?
            .socket_type;
        let to = self.node(to_node)?;
        let to_type = to
            .input(to_socket)
            .ok_or_else(|| unknown_socket(to, SocketInOut::Input, to_socket))?
            .socket_type;
        if from_type == SocketType::Shader && to_type != SocketType::Shader {
            return Err(BkeError::SocketTypeMismatch(format!(
                "cannot link shader output \"{from_socket}\" into {to_type:?} input \"{to_socket}\""
            )));
        }
        if from_node == to_node || self.is_downstream(to_node, from_node) {
            return Err(BkeError::NodeCycle);
        }
        self.remove_link(to_node, to_socket);
        self.links.push(Link {
            from_node,
            from_socket: from_socket.to_string(),
            to_node,
            to_socket: to_socket.to_string(),
        });
        Ok(())
    }

    /// Is `target` reachable by following links forward from `start`?
    fn is_downstream(&self, start: NodeId, target: NodeId) -> bool {
        let mut stack = vec![start];
        let mut seen = std::collections::HashSet::new();
        while let Some(current) = stack.pop() {
            if current == target {
                return true;
            }
            if seen.insert(current) {
                stack.extend(self.links.iter().filter(|l| l.from_node == current).map(|l| l.to_node));
            }
        }
        false
    }

    /// Remove the link into an input. Returns false if there was none.
    pub fn remove_link(&mut self, to_node: NodeId, to_socket: &str) -> bool {
        let len_before = self.links.len();
        self.links.retain(|l| !(l.to_node == to_node && l.to_socket == to_socket));
        self.links.len() != len_before
    }

    pub fn links(&self) -> &[Link] {
        &self.links
    }

    /// The link feeding an input, if any.
    pub fn input_link(&self, to_node: NodeId, to_socket: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.to_node == to_node && l.to_socket == to_socket)
    }

    /// The Material Output node used for rendering (`ntreeShaderOutputNode`).
    pub fn output_node(&self) -> Option<NodeId> {
        self.active_output
    }

    pub fn set_active_output(&mut self, node: NodeId) -> Result<(), BkeError> {
        if self.node(node)?.idname != "ShaderNodeOutputMaterial" {
            return Err(BkeError::UnknownNodeType("active output must be a Material Output".into()));
        }
        self.active_output = Some(node);
        Ok(())
    }

    /// All nodes ordered so that every node comes after the nodes linked into it
    /// (the order an evaluator processes them in). Links never form cycles.
    pub fn topological_order(&self) -> Vec<NodeId> {
        let mut pending: BTreeMap<NodeId, usize> = self.nodes().map(|(id, _)| (id, 0)).collect();
        for link in &self.links {
            *pending.get_mut(&link.to_node).expect("links only touch live nodes") += 1;
        }
        let mut ready: Vec<NodeId> =
            pending.iter().filter(|(_, &n)| n == 0).map(|(&id, _)| id).rev().collect();
        let mut order = Vec::with_capacity(pending.len());
        while let Some(node) = ready.pop() {
            order.push(node);
            for link in self.links.iter().filter(|l| l.from_node == node) {
                let count = pending.get_mut(&link.to_node).expect("live node");
                *count -= 1;
                if *count == 0 {
                    ready.push(link.to_node);
                }
            }
        }
        order
    }

    /// The whole tree as JSON: `{"output_node": id|null, "nodes": [...], "links": [...]}`.
    pub fn to_json(&self) -> String {
        let nodes: Vec<&Node> = self.nodes().map(|(_, n)| n).collect();
        serde_json::json!({
            "output_node": self.active_output,
            "nodes": nodes,
            "links": self.links,
        })
        .to_string()
    }
}
