//! Math node operations and Mix node color blend modes, matching Blender's GPU code
//! (`gpu_shader_material_math.glsl`, `gpu_shader_material_mix_color.glsl`) including its
//! "safe" variants that return 0 instead of NaN/inf.

/// `ShaderNodeMath` with `operation` as its RNA enum identifier. `None` for unknown names.
pub fn math(operation: &str, a: f32, b: f32, c: f32) -> Option<f32> {
    let safe_divide = |a: f32, b: f32| if b != 0.0 { a / b } else { 0.0 };
    Some(match operation {
        "ADD" => a + b,
        "SUBTRACT" => a - b,
        "MULTIPLY" => a * b,
        "DIVIDE" => safe_divide(a, b),
        "MULTIPLY_ADD" => a * b + c,
        "POWER" => {
            // compatible_pow: negative base only with an integer exponent.
            if a >= 0.0 || b == b.floor() {
                a.powf(b)
            } else {
                0.0
            }
        }
        "LOGARITHM" => {
            if a > 0.0 && b > 0.0 {
                safe_divide(a.ln(), b.ln())
            } else {
                0.0
            }
        }
        "SQRT" => {
            if a > 0.0 {
                a.sqrt()
            } else {
                0.0
            }
        }
        "INVERSE_SQRT" => {
            if a > 0.0 {
                1.0 / a.sqrt()
            } else {
                0.0
            }
        }
        "ABSOLUTE" => a.abs(),
        "EXPONENT" => a.exp(),
        "MINIMUM" => a.min(b),
        "MAXIMUM" => a.max(b),
        "LESS_THAN" => (a < b) as u8 as f32,
        "GREATER_THAN" => (a > b) as u8 as f32,
        "SIGN" => {
            if a == 0.0 {
                0.0
            } else {
                a.signum()
            }
        }
        "COMPARE" => ((a - b).abs() <= c.max(1e-5)) as u8 as f32,
        "ROUND" => (a + 0.5).floor(),
        "FLOOR" => a.floor(),
        "CEIL" => a.ceil(),
        "TRUNC" => a.trunc(),
        "FRACT" => a - a.floor(),
        // Blender's MODULO is C fmod (sign of a); FLOORED_MODULO has the sign of b.
        "MODULO" => {
            if b != 0.0 {
                a % b
            } else {
                0.0
            }
        }
        "FLOORED_MODULO" => {
            if b != 0.0 {
                a - (a / b).floor() * b
            } else {
                0.0
            }
        }
        // wrapf(value, max, min)
        "WRAP" => {
            let range = b - c;
            if range != 0.0 {
                a - range * ((a - c) / range).floor()
            } else {
                c
            }
        }
        "SNAP" => {
            if b != 0.0 {
                (a / b).floor() * b
            } else {
                0.0
            }
        }
        "PINGPONG" => {
            if b != 0.0 {
                let t = (a - b) / (b * 2.0);
                ((t - t.floor()) * b * 2.0 - b).abs()
            } else {
                0.0
            }
        }
        "SINE" => a.sin(),
        "COSINE" => a.cos(),
        "TANGENT" => a.tan(),
        "ARCSINE" => {
            if (-1.0..=1.0).contains(&a) {
                a.asin()
            } else {
                0.0
            }
        }
        "ARCCOSINE" => {
            if (-1.0..=1.0).contains(&a) {
                a.acos()
            } else {
                0.0
            }
        }
        "ARCTANGENT" => a.atan(),
        "ARCTAN2" => a.atan2(b),
        "SINH" => a.sinh(),
        "COSH" => a.cosh(),
        "TANH" => a.tanh(),
        "RADIANS" => a.to_radians(),
        "DEGREES" => a.to_degrees(),
        "SMOOTH_MIN" => smooth_min(a, b, c),
        "SMOOTH_MAX" => -smooth_min(-a, -b, c),
        _ => return None,
    })
}

fn smooth_min(a: f32, b: f32, c: f32) -> f32 {
    if c != 0.0 {
        let h = (c - (a - b).abs()).max(0.0) / c;
        a.min(b) - h * h * h * c * (1.0 / 6.0)
    } else {
        a.min(b)
    }
}

/// `ShaderNodeMix` in RGBA mode. `fac` is already clamped to 0..1 (the node's default
/// `clamp_factor`). Alpha is taken from `a`, as in Blender.
pub fn mix_color(blend_type: &str, fac: f32, a: [f32; 4], b: [f32; 4]) -> Option<[f32; 4]> {
    let facm = 1.0 - fac;
    let lerp = |x: f32, y: f32| x * facm + y * fac;
    let per_channel = |f: &dyn Fn(f32, f32) -> f32| [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2]), a[3]];
    Some(match blend_type {
        "MIX" => per_channel(&|x, y| lerp(x, y)),
        "ADD" => per_channel(&|x, y| lerp(x, x + y)),
        "MULTIPLY" => per_channel(&|x, y| lerp(x, x * y)),
        "SUBTRACT" => per_channel(&|x, y| lerp(x, x - y)),
        "SCREEN" => per_channel(&|x, y| 1.0 - (facm + fac * (1.0 - y)) * (1.0 - x)),
        "DIVIDE" => per_channel(&|x, y| facm * x + fac * if y != 0.0 { x / y } else { 0.0 }),
        "DIFFERENCE" => per_channel(&|x, y| lerp(x, (x - y).abs())),
        "EXCLUSION" => per_channel(&|x, y| lerp(x, x + y - 2.0 * x * y)),
        "DARKEN" => per_channel(&|x, y| lerp(x, x.min(y))),
        "LIGHTEN" => per_channel(&|x, y| lerp(x, x.max(y))),
        "OVERLAY" => per_channel(&|x, y| {
            if x < 0.5 {
                x * (facm + 2.0 * fac * y)
            } else {
                1.0 - (facm + 2.0 * fac * (1.0 - y)) * (1.0 - x)
            }
        }),
        _ => return None,
    })
}
