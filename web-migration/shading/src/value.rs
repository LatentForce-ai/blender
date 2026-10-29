//! Values flowing through links, and Blender's implicit socket conversions
//! (`nodes/intern/node_socket.cc`, GPU `convert_*` functions).

use blenkernel::{SocketType, SocketValue};

/// Rec. 709 luminance weights, Blender's default `IMB_colormanagement_get_luminance`.
const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// A non-shader value on a socket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Float(f32),
    Vector([f32; 3]),
    Color([f32; 4]),
}

impl Value {
    /// The value an unlinked socket holds. Shader sockets have none.
    pub fn from_socket(value: SocketValue) -> Option<Value> {
        Some(match value {
            SocketValue::Float(v) => Value::Float(v),
            SocketValue::Bool(v) => Value::Float(if v { 1.0 } else { 0.0 }),
            SocketValue::Vector(v) => Value::Vector(v),
            SocketValue::Color(v) => Value::Color(v),
            SocketValue::None => return None,
        })
    }

    pub fn to_float(self) -> f32 {
        match self {
            Value::Float(v) => v,
            // Vector → float is the average of the components.
            Value::Vector([x, y, z]) => (x + y + z) / 3.0,
            Value::Color([r, g, b, _]) => r * LUMINANCE[0] + g * LUMINANCE[1] + b * LUMINANCE[2],
        }
    }

    pub fn to_vector(self) -> [f32; 3] {
        match self {
            Value::Float(v) => [v; 3],
            Value::Vector(v) => v,
            Value::Color([r, g, b, _]) => [r, g, b],
        }
    }

    pub fn to_color(self) -> [f32; 4] {
        match self {
            Value::Float(v) => [v, v, v, 1.0],
            Value::Vector([x, y, z]) => [x, y, z, 1.0],
            Value::Color(c) => c,
        }
    }

    /// Convert to the type of the socket it is linked into.
    pub fn convert(self, to: SocketType) -> Value {
        match to {
            SocketType::Float | SocketType::Bool | SocketType::Shader => Value::Float(self.to_float()),
            SocketType::Vector => Value::Vector(self.to_vector()),
            SocketType::Color => Value::Color(self.to_color()),
        }
    }
}
