use std::{error::Error, io, path::Path};

use image::RgbaImage;
use sdl3::video::Window;

/// Captures the current OpenGL back buffer as a top-left-origin RGBA PNG.
///
/// # Errors
///
/// Returns an error when the framebuffer is too large to address safely, when an RGBA image cannot
/// be constructed from the captured bytes, or when the PNG cannot be written.
pub fn save_back_buffer(window: &Window, path: &Path) -> Result<(), Box<dyn Error>> {
    let (width, height) = window.size_in_pixels();
    let width_usize = usize::try_from(width)?;
    let height_usize = usize::try_from(height)?;
    let byte_len = width_usize
        .checked_mul(height_usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| io::Error::other("framebuffer byte count overflow"))?;
    let width_i32 = i32::try_from(width)?;
    let height_i32 = i32::try_from(height)?;
    let mut bytes = vec![0_u8; byte_len];

    // SAFETY: a current OpenGL context is guaranteed by the caller. `bytes` is sized for exactly
    // width*height RGBA8 pixels, PACK_ALIGNMENT=1 removes row padding, and the pointer remains valid
    // for the duration of the synchronous `ReadPixels` call.
    unsafe {
        gl::PixelStorei(gl::PACK_ALIGNMENT, 1);
        gl::ReadPixels(
            0,
            0,
            width_i32,
            height_i32,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            bytes.as_mut_ptr().cast(),
        );
    }

    let mut image = RgbaImage::from_raw(width, height, bytes)
        .ok_or_else(|| io::Error::other("OpenGL capture did not form a valid RGBA image"))?;
    image::imageops::flip_vertical_in_place(&mut image);
    image.save(path)?;
    Ok(())
}
