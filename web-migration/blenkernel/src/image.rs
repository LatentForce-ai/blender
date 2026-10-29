//! Port of `struct Image` (`makesdna/DNA_image_types.h`, `blenkernel/intern/image.cc`), for
//! image textures (HACK-14).
//!
//! Blender loads pixels lazily from a file path into `ImBuf`s; here the caller (the browser)
//! decodes the file and hands over 8-bit RGBA, stored with the image as a packed image would be.
//! Colour space is always sRGB, Blender's default for byte images.

use crate::error::BkeError;
use crate::id::IdHeader;

/// Handle of an image in a [`crate::Main`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub id: IdHeader,
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8, sRGB-encoded colour with straight alpha, row 0 at the top (as browsers
    /// decode images; Blender's `ImBuf` starts at the bottom, which UV lookups account for).
    pixels: Vec<u8>,
}

impl Image {
    /// An image from decoded RGBA8 pixels; `pixels` must hold `width * height * 4` bytes.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Image, BkeError> {
        if width == 0 || height == 0 {
            return Err(BkeError::InvalidValue("image size must be at least 1×1".into()));
        }
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            return Err(BkeError::LengthMismatch { what: "image pixels", expected, got: pixels.len() });
        }
        Ok(Image { id: IdHeader::default(), width, height, pixels })
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}
