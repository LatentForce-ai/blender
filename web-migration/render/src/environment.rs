//! Image-based lighting for Material Preview: an equirectangular environment (Blender's
//! "studio light" HDRI, `blenkernel/intern/studiolight.cc`) turned into what the shader samples.
//!
//! - Diffuse: 9 spherical-harmonic irradiance coefficients (EEVEE stores the world's diffuse
//!   lighting as low-order SH too).
//! - Specular: [`LEVELS`] GGX-prefiltered copies of the environment, one per mip level, from
//!   roughness 0 to 1 (EEVEE's reflection probe mips), combined in the shader with an analytic
//!   split-sum BRDF instead of EEVEE's lookup table.
//!
//! Directions are Blender world space (Z up). Equirectangular mapping follows Blender's
//! `direction_to_equirectangular`, with texture row 0 at the top.

use std::f32::consts::{FRAC_1_PI, PI, TAU};

use glam::{Vec2, Vec3};

/// Mip levels of the prefiltered environment; level `i` holds roughness `i / (LEVELS - 1)`.
pub const LEVELS: usize = 5;
/// Size of mip level 0. Each further level halves both sides.
pub const BASE_SIZE: (usize, usize) = (256, 128);
/// Source resolution the rough levels are prefiltered from (cost is output × source texels).
const PREFILTER_SOURCE: (usize, usize) = (64, 32);

/// Real spherical-harmonic basis, bands 0–2 (`Y_lm` in Ramamoorthi and Hanrahan's order).
fn sh_basis(d: Vec3) -> [f32; 9] {
    [
        0.282_095,
        0.488_603 * d.y,
        0.488_603 * d.z,
        0.488_603 * d.x,
        1.092_548 * d.x * d.y,
        1.092_548 * d.y * d.z,
        0.315_392 * (3.0 * d.z * d.z - 1.0),
        1.092_548 * d.x * d.z,
        0.546_274 * (d.x * d.x - d.y * d.y),
    ]
}

/// Texture coordinates of a world direction: `u` around Z, `v` from the top (+Z) down.
pub fn direction_to_uv(d: Vec3) -> Vec2 {
    let u = -d.y.atan2(d.x) / TAU + 0.5;
    let v = 0.5 - d.z.atan2(Vec2::new(d.x, d.y).length()) * FRAC_1_PI;
    Vec2::new(u, v)
}

/// Inverse of [`direction_to_uv`]; returns a unit vector.
pub fn uv_to_direction(uv: Vec2) -> Vec3 {
    let phi = (0.5 - uv.x) * TAU;
    let latitude = (0.5 - uv.y) * PI;
    Vec3::new(phi.cos() * latitude.cos(), phi.sin() * latitude.cos(), latitude.sin())
}

/// An equirectangular image of scene-linear radiance.
#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    pub width: usize,
    pub height: usize,
    /// Row-major, row 0 at the top.
    pub pixels: Vec<Vec3>,
}

impl Environment {
    /// Fills each texel from its centre direction.
    pub fn from_fn(width: usize, height: usize, radiance: impl Fn(Vec3) -> Vec3) -> Environment {
        let mut pixels = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                pixels.push(radiance(texel_direction(width, height, x, y)));
            }
        }
        Environment { width, height, pixels }
    }

    /// The default Material Preview environment: a soft studio with a warm key light at the
    /// upper left of the default view, a cool fill and a rim light behind, over a sky-to-floor
    /// gradient. Stands in for Blender's bundled `forest.exr` (EXR isn't decoded here).
    pub fn studio() -> Environment {
        let key = Vec3::new(0.3, -1.0, 0.9).normalize();
        let fill = Vec3::new(-1.0, -0.2, 0.3).normalize();
        let rim = Vec3::new(-0.4, 1.0, 0.6).normalize();
        // A soft disc around `toward`; `spread` is 1 - cos of its angular size.
        let lobe = |d: Vec3, toward: Vec3, spread: f32| ((d.dot(toward) - 1.0) / spread).exp();
        Environment::from_fn(BASE_SIZE.0, BASE_SIZE.1, |d| {
            let sky = if d.z >= 0.0 {
                Vec3::splat(0.9).lerp(Vec3::new(0.5, 0.58, 0.72), d.z)
            } else {
                Vec3::new(0.32, 0.29, 0.26).lerp(Vec3::splat(0.15), -d.z)
            };
            sky + Vec3::new(1.0, 0.95, 0.88) * (12.0 * lobe(d, key, 0.02))
                + Vec3::new(0.8, 0.88, 1.0) * (1.5 * lobe(d, fill, 0.08))
                + Vec3::splat(6.0 * lobe(d, rim, 0.03))
        })
    }

    pub fn texel(&self, x: usize, y: usize) -> Vec3 {
        self.pixels[y * self.width + x]
    }

    /// Box-filtered (or nearest, when growing) copy at another size.
    pub fn resized(&self, width: usize, height: usize) -> Environment {
        let mut sums = vec![Vec3::ZERO; width * height];
        let mut counts = vec![0u32; width * height];
        for y in 0..self.height {
            for x in 0..self.width {
                let i = (y * height / self.height) * width + x * width / self.width;
                sums[i] += self.texel(x, y);
                counts[i] += 1;
            }
        }
        let pixels = (0..width * height)
            .map(|i| {
                if counts[i] > 0 {
                    sums[i] / counts[i] as f32
                } else {
                    // Growing: take the source texel under this one.
                    let (x, y) = (i % width, i / width);
                    self.texel(x * self.width / width, y * self.height / height)
                }
            })
            .collect();
        Environment { width, height, pixels }
    }

    /// Texel directions and solid angles, for integrating over the sphere.
    fn samples(&self) -> Vec<(Vec3, f32, Vec3)> {
        let (w, h) = (self.width, self.height);
        let texel_area = (TAU / w as f32) * (PI / h as f32);
        let mut samples = Vec::with_capacity(w * h);
        for y in 0..h {
            let latitude = (0.5 - (y as f32 + 0.5) / h as f32) * PI;
            let solid_angle = texel_area * latitude.cos();
            for x in 0..w {
                samples.push((texel_direction(w, h, x, y), solid_angle, self.texel(x, y)));
            }
        }
        samples
    }
}

fn texel_direction(width: usize, height: usize, x: usize, y: usize) -> Vec3 {
    uv_to_direction(Vec2::new(
        (x as f32 + 0.5) / width as f32,
        (y as f32 + 0.5) / height as f32,
    ))
}

/// Diffuse lighting as 9 SH coefficients, already convolved with the clamped cosine and divided
/// by π, so `eval(n)` is the outgoing radiance of a white Lambertian surface facing `n`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Irradiance(pub [Vec3; 9]);

impl Irradiance {
    pub fn from_environment(env: &Environment) -> Irradiance {
        let mut coefficients = [Vec3::ZERO; 9];
        for (direction, solid_angle, radiance) in env.samples() {
            for (c, y) in coefficients.iter_mut().zip(sh_basis(direction)) {
                *c += radiance * (y * solid_angle);
            }
        }
        // Cosine-lobe convolution per band (π, 2π/3, π/4), then the Lambertian 1/π.
        const BAND: [f32; 9] = [1.0, 2.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 0.25, 0.25, 0.25, 0.25, 0.25];
        for (c, band) in coefficients.iter_mut().zip(BAND) {
            *c *= band;
        }
        Irradiance(coefficients)
    }

    pub fn eval(&self, n: Vec3) -> Vec3 {
        self.0.iter().zip(sh_basis(n)).map(|(c, y)| *c * y).sum()
    }
}

/// GGX normal distribution for `alpha = roughness²`.
fn ggx(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    a2 / (PI * d * d)
}

/// The environment prefiltered for each roughness level, sizes halving from [`BASE_SIZE`].
/// Level 0 (mirror) is the environment itself; rougher levels integrate the GGX lobe around
/// each texel's direction (the usual N = V = R assumption).
pub fn prefiltered_levels(env: &Environment) -> Vec<Environment> {
    let source = env.resized(PREFILTER_SOURCE.0, PREFILTER_SOURCE.1).samples();
    let mut levels = vec![env.resized(BASE_SIZE.0, BASE_SIZE.1)];
    for level in 1..LEVELS {
        let roughness = level as f32 / (LEVELS - 1) as f32;
        let alpha = roughness * roughness;
        let (w, h) = (BASE_SIZE.0 >> level, BASE_SIZE.1 >> level);
        levels.push(Environment::from_fn(w, h, |r| {
            let mut sum = Vec3::ZERO;
            let mut weight = 0.0;
            for &(l, solid_angle, radiance) in &source {
                let n_dot_l = r.dot(l);
                if n_dot_l <= 0.0 {
                    continue;
                }
                let n_dot_h = r.dot((r + l).normalize());
                let w = ggx(n_dot_h, alpha) * n_dot_l * solid_angle;
                sum += radiance * w;
                weight += w;
            }
            if weight > 0.0 { sum / weight } else { Vec3::ZERO }
        }));
    }
    levels
}

/// Parses a Radiance `.hdr` (RGBE) image: flat or new-style run-length-encoded scanlines, in
/// the standard `-Y height +X width` orientation.
pub fn parse_radiance_hdr(bytes: &[u8]) -> Result<Environment, String> {
    let mut pos = 0;
    let mut next_line = || -> Result<&[u8], String> {
        let start = pos;
        let end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| start + i)
            .ok_or("truncated .hdr header")?;
        pos = end + 1;
        Ok(&bytes[start..end])
    };
    let magic = next_line()?;
    if !(magic.starts_with(b"#?RADIANCE") || magic.starts_with(b"#?RGBE")) {
        return Err("not a Radiance .hdr file".into());
    }
    loop {
        let line = next_line()?;
        if line.is_empty() {
            break;
        }
        if let Some(format) = line.strip_prefix(b"FORMAT=") {
            if format != b"32-bit_rle_rgbe" {
                return Err(format!("unsupported .hdr format {}", String::from_utf8_lossy(format)));
            }
        }
    }
    let resolution = String::from_utf8_lossy(next_line()?).into_owned();
    let parts: Vec<&str> = resolution.split_whitespace().collect();
    let (height, width) = match parts.as_slice() {
        ["-Y", h, "+X", w] => (
            h.parse::<usize>().map_err(|e| e.to_string())?,
            w.parse::<usize>().map_err(|e| e.to_string())?,
        ),
        _ => return Err(format!("unsupported .hdr orientation \"{resolution}\"")),
    };
    if width == 0 || height == 0 {
        return Err("empty .hdr image".into());
    }

    let mut data = Cursor { bytes: &bytes[pos..], at: 0 };
    let mut pixels = Vec::with_capacity(width * height);
    let mut scanline = vec![[0u8; 4]; width];
    for _ in 0..height {
        let rle = (8..=0x7fff).contains(&width) && data.peek(2) == Some(&[2, 2]);
        if rle {
            let header = data.take(4)?;
            if usize::from(header[2]) << 8 | usize::from(header[3]) != width {
                return Err("bad .hdr scanline width".into());
            }
            for channel in 0..4 {
                let mut x = 0;
                while x < width {
                    let count = usize::from(data.take(1)?[0]);
                    if count > 128 {
                        let run = count - 128;
                        let value = data.take(1)?[0];
                        for pixel in scanline.get_mut(x..x + run).ok_or("bad .hdr run length")? {
                            pixel[channel] = value;
                        }
                        x += run;
                    } else {
                        let values = data.take(count)?;
                        for (pixel, &value) in scanline.get_mut(x..x + count).ok_or("bad .hdr run length")?.iter_mut().zip(values) {
                            pixel[channel] = value;
                        }
                        x += count;
                    }
                }
            }
        } else {
            for pixel in scanline.iter_mut() {
                pixel.copy_from_slice(data.take(4)?);
            }
        }
        pixels.extend(scanline.iter().map(|&[r, g, b, e]| rgbe_to_linear(r, g, b, e)));
    }
    Ok(Environment { width, height, pixels })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self, n: usize) -> Option<&'a [u8]> {
        self.bytes.get(self.at..self.at + n)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let slice = self.peek(n).ok_or("truncated .hdr pixel data")?;
        self.at += n;
        Ok(slice)
    }
}

/// Radiance's `colr_color`: mantissas scaled by 2^(e - 136), sampled at the bucket centre.
fn rgbe_to_linear(r: u8, g: u8, b: u8, e: u8) -> Vec3 {
    if e == 0 {
        return Vec3::ZERO;
    }
    let scale = 2f32.powi(i32::from(e) - 136);
    Vec3::new(f32::from(r) + 0.5, f32::from(g) + 0.5, f32::from(b) + 0.5) * scale
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3, eps: f32) -> bool {
        (a - b).abs().max_element() < eps
    }

    #[test]
    fn equirect_mapping_matches_blender_and_round_trips() {
        // Blender: +X at the centre of the image, +Z at the top.
        assert!(direction_to_uv(Vec3::X).abs_diff_eq(Vec2::new(0.5, 0.5), 1e-6));
        assert!(direction_to_uv(Vec3::Z).y.abs() < 1e-6);
        assert!((direction_to_uv(-Vec3::Z).y - 1.0).abs() < 1e-6);
        for d in [Vec3::new(0.3, -0.8, 0.5), Vec3::new(-0.7, 0.2, -0.6), Vec3::Y] {
            let d = d.normalize();
            assert!(uv_to_direction(direction_to_uv(d)).abs_diff_eq(d, 1e-5));
        }
    }

    #[test]
    fn constant_environment_lights_a_white_surface_to_its_radiance() {
        let env = Environment::from_fn(64, 32, |_| Vec3::splat(2.0));
        let irradiance = Irradiance::from_environment(&env);
        for n in [Vec3::X, Vec3::Z, -Vec3::Z, Vec3::new(0.5, -0.5, 0.7).normalize()] {
            assert!(close(irradiance.eval(n), Vec3::splat(2.0), 0.02), "{:?}", irradiance.eval(n));
        }
        for level in prefiltered_levels(&env) {
            assert!(level.pixels.iter().all(|p| close(*p, Vec3::splat(2.0), 1e-3)));
        }
    }

    #[test]
    fn sky_lights_upward_faces_more_than_downward_ones() {
        let env = Environment::from_fn(64, 32, |d| Vec3::splat(d.z.max(0.0)));
        let irradiance = Irradiance::from_environment(&env);
        assert!(irradiance.eval(Vec3::Z).x > 0.5);
        assert!(irradiance.eval(-Vec3::Z).x < 0.05);
    }

    #[test]
    fn prefiltered_levels_halve_and_blur_more_with_roughness() {
        let levels = prefiltered_levels(&Environment::studio());
        assert_eq!(levels.len(), LEVELS);
        for (i, level) in levels.iter().enumerate() {
            assert_eq!((level.width, level.height), (BASE_SIZE.0 >> i, BASE_SIZE.1 >> i));
        }
        // The key light's peak spreads out as roughness rises.
        let peak = |env: &Environment| env.pixels.iter().map(|p| p.x).fold(0.0, f32::max);
        let peaks: Vec<f32> = levels.iter().map(peak).collect();
        assert!(peaks.windows(2).all(|w| w[0] > w[1]), "{peaks:?}");
    }

    #[test]
    fn parses_flat_and_rle_radiance_files() {
        // 2×1 flat: (128,64,32,e=129) is 1.0, 0.5, 0.25 at bucket centres; e=0 is black.
        let mut flat = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 2\n".to_vec();
        flat.extend([128, 64, 32, 129, 0, 0, 0, 0]);
        let env = parse_radiance_hdr(&flat).unwrap();
        assert_eq!((env.width, env.height), (2, 1));
        assert!(close(env.pixels[0], Vec3::new(1.0, 0.5, 0.25), 0.01));
        assert_eq!(env.pixels[1], Vec3::ZERO);

        // 8×1 RLE: each channel one run of 8.
        let mut rle = b"#?RGBE\n\n-Y 1 +X 8\n".to_vec();
        rle.extend([2, 2, 0, 8]);
        for value in [128, 128, 128, 130] {
            rle.extend([128 + 8, value]);
        }
        let env = parse_radiance_hdr(&rle).unwrap();
        assert!(env.pixels.iter().all(|p| close(*p, Vec3::splat(2.0), 0.02)));

        assert!(parse_radiance_hdr(b"P6\n").is_err());
        assert!(parse_radiance_hdr(b"#?RADIANCE\n\n-Y 2 +X 2\n\x01").is_err());
    }
}
