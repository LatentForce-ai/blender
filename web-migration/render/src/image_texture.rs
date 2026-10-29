//! CPU side of image textures (HACK-14): the mip chain uploaded for a blenkernel `Image`
//! (`gpu/intern/gpu_texture.cc` and `imbuf` mipmap generation, simplified).

use crate::theme::srgb_to_linear;

/// Largest texture side WebGL2 guarantees (`downlevel_webgl2_defaults`); bigger images start
/// from the first mip level that fits.
pub const MAX_SIZE: u32 = 2048;

/// One mip level: RGBA8, sRGB colour, straight alpha, row 0 at the top.
#[derive(Clone, Debug, PartialEq)]
pub struct MipLevel {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Halves a level with a 2×2 box filter, averaging colour in linear space so dark and bright
/// texels blend as light does. Odd sides repeat their last row or column.
fn downsample(level: &MipLevel) -> MipLevel {
    let (width, height) = ((level.width / 2).max(1), (level.height / 2).max(1));
    let texel = |x: u32, y: u32, c: usize| {
        let (x, y) = (x.min(level.width - 1), y.min(level.height - 1));
        level.pixels[((y * level.width + x) * 4) as usize + c]
    };
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let block = [(2 * x, 2 * y), (2 * x + 1, 2 * y), (2 * x, 2 * y + 1), (2 * x + 1, 2 * y + 1)];
            for c in 0..4 {
                let average = if c == 3 {
                    block.iter().map(|&(bx, by)| f32::from(texel(bx, by, c)) / 255.0).sum::<f32>() / 4.0
                } else {
                    let linear: f32 = block
                        .iter()
                        .map(|&(bx, by)| srgb_to_linear(f32::from(texel(bx, by, c)) / 255.0))
                        .sum::<f32>()
                        / 4.0;
                    linear_to_srgb(linear)
                };
                pixels.push((average * 255.0).round() as u8);
            }
        }
    }
    MipLevel { width, height, pixels }
}

/// The full mip chain down to 1×1, starting from the largest level within [`MAX_SIZE`].
pub fn mip_chain(width: u32, height: u32, pixels: &[u8]) -> Vec<MipLevel> {
    let mut level = MipLevel { width, height, pixels: pixels.to_vec() };
    while level.width > MAX_SIZE || level.height > MAX_SIZE {
        level = downsample(&level);
    }
    let mut chain = vec![level];
    while chain.last().is_some_and(|l| l.width > 1 || l.height > 1) {
        chain.push(downsample(chain.last().unwrap()));
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_halves_down_to_one_texel() {
        let chain = mip_chain(8, 2, &[200; 8 * 2 * 4]);
        let sizes: Vec<_> = chain.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(sizes, [(8, 2), (4, 1), (2, 1), (1, 1)]);
        assert!(chain.iter().all(|l| l.pixels.len() == (l.width * l.height * 4) as usize));
        assert!(chain.iter().all(|l| l.pixels.iter().all(|&p| p.abs_diff(200) <= 1)));
    }

    #[test]
    fn black_and_white_average_in_linear_light() {
        // A 2×1 black/white pair: linear average 0.5 is sRGB ~188, not the naive 128.
        let chain = mip_chain(2, 1, &[0, 0, 0, 255, 255, 255, 255, 255]);
        assert_eq!(chain[1].pixels[..3], [188, 188, 188]);
        assert_eq!(chain[1].pixels[3], 255);
    }

    #[test]
    fn oversized_images_start_within_the_webgl2_limit() {
        let chain = mip_chain(4096, 16, &vec![0; 4096 * 16 * 4]);
        assert_eq!((chain[0].width, chain[0].height), (2048, 8));
    }
}
