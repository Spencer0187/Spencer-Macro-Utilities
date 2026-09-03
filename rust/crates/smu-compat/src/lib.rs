//! Cross-language compatibility helpers for the C++ oracle and Rust rewrite.

use std::{fs, path::Path};

use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use serde::Deserialize;
use thiserror::Error;

pub mod ui_geometry;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct IgnoreRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl IgnoreRect {
    #[must_use]
    pub const fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(self.width)
            && y < self.y.saturating_add(self.height)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiffPolicy<'a> {
    pub channel_tolerance: u8,
    pub ignore: &'a [IgnoreRect],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiffStats {
    pub compared_pixels: u64,
    pub ignored_pixels: u64,
    pub different_pixels: u64,
    pub max_channel_delta: u8,
}

impl DiffStats {
    #[must_use]
    pub const fn exact_match(self) -> bool {
        self.different_pixels == 0
    }
}

#[derive(Debug, Error)]
pub enum DiffError {
    #[error("image dimensions differ: expected {expected:?}, actual {actual:?}")]
    DimensionMismatch { expected: (u32, u32), actual: (u32, u32) },
    #[error("failed to decode image: {0}")]
    Image(#[from] image::ImageError),
    #[error("failed to read UI case manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse UI case manifest: {0}")]
    Toml(#[from] toml::de::Error),
}

/// Compares two images. RGB/RGBA inputs are normalized to RGBA8 before comparison.
///
/// The default policy is strict pixel-for-pixel equality. Ignored rectangles are counted and never
/// influence the result.
///
/// # Errors
///
/// Returns [`DiffError::DimensionMismatch`] when the images have different dimensions.
pub fn diff_images(
    expected: &DynamicImage,
    actual: &DynamicImage,
    policy: DiffPolicy<'_>,
) -> Result<DiffStats, DiffError> {
    if expected.dimensions() != actual.dimensions() {
        return Err(DiffError::DimensionMismatch {
            expected: expected.dimensions(),
            actual: actual.dimensions(),
        });
    }

    let expected = expected.to_rgba8();
    let actual = actual.to_rgba8();
    let mut stats = DiffStats::default();

    for (x, y, expected_pixel) in expected.enumerate_pixels() {
        if policy.ignore.iter().any(|rect| rect.contains(x, y)) {
            stats.ignored_pixels += 1;
            continue;
        }
        stats.compared_pixels += 1;
        let actual_pixel = actual.get_pixel(x, y);
        let delta = max_channel_delta(*expected_pixel, *actual_pixel);
        stats.max_channel_delta = stats.max_channel_delta.max(delta);
        if delta > policy.channel_tolerance {
            stats.different_pixels += 1;
        }
    }

    Ok(stats)
}

/// Produces a diagnostic image where matching/ignored pixels are transparent and differing pixels
/// preserve the actual RGB value with opaque alpha.
///
/// # Errors
///
/// Returns [`DiffError::DimensionMismatch`] when the images have different dimensions.
pub fn difference_image(
    expected: &DynamicImage,
    actual: &DynamicImage,
    policy: DiffPolicy<'_>,
) -> Result<RgbaImage, DiffError> {
    if expected.dimensions() != actual.dimensions() {
        return Err(DiffError::DimensionMismatch {
            expected: expected.dimensions(),
            actual: actual.dimensions(),
        });
    }

    let expected = expected.to_rgba8();
    let actual = actual.to_rgba8();
    let mut output = RgbaImage::new(expected.width(), expected.height());
    for (x, y, expected_pixel) in expected.enumerate_pixels() {
        if policy.ignore.iter().any(|rect| rect.contains(x, y)) {
            continue;
        }
        let actual_pixel = actual.get_pixel(x, y);
        if max_channel_delta(*expected_pixel, *actual_pixel) > policy.channel_tolerance {
            output.put_pixel(
                x,
                y,
                Rgba([actual_pixel[0], actual_pixel[1], actual_pixel[2], u8::MAX]),
            );
        }
    }
    Ok(output)
}

fn max_channel_delta(left: Rgba<u8>, right: Rgba<u8>) -> u8 {
    left.0
        .into_iter()
        .zip(right.0)
        .map(|(left_channel, right_channel)| left_channel.abs_diff(right_channel))
        .max()
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
pub struct UiCaseManifest {
    #[serde(default, rename = "reference")]
    pub references: Vec<UiReference>,
    #[serde(default, rename = "case")]
    pub cases: Vec<UiCase>,
}

#[derive(Debug, Deserialize)]
pub struct UiReference {
    pub name: String,
    pub image_width: u32,
    pub image_height: u32,
    pub baseline: String,
}

#[derive(Debug, Deserialize)]
pub struct UiCase {
    pub name: String,
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale: f32,
    pub platform: Option<String>,
    pub parity_state: Option<String>,
    #[serde(default)]
    pub automated: bool,
    #[serde(default)]
    pub gate_tolerance: u8,
    #[serde(default)]
    pub ignore: Vec<IgnoreRect>,
}

/// Loads a checked-in UI capture matrix.
///
/// # Errors
///
/// Returns [`DiffError::Io`] when the file cannot be read and [`DiffError::Toml`] when it is not a
/// valid manifest.
pub fn load_ui_case_manifest(path: &Path) -> Result<UiCaseManifest, DiffError> {
    let text = fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::float_cmp)]
    use std::path::PathBuf;

    use image::{DynamicImage, Rgba, RgbaImage};

    use super::{DiffPolicy, IgnoreRect, diff_images, load_ui_case_manifest};

    #[test]
    fn exact_diff_detects_single_channel_change() {
        let expected = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])));
        let mut actual = expected.to_rgba8();
        actual.put_pixel(1, 1, Rgba([1, 2, 4, 255]));
        let stats =
            diff_images(&expected, &DynamicImage::ImageRgba8(actual), DiffPolicy::default())
                .expect("same-size in-memory images");
        assert_eq!(stats.different_pixels, 1);
        assert_eq!(stats.max_channel_delta, 1);
    }

    #[test]
    fn narrow_ignore_rect_masks_only_declared_pixels() {
        let expected = DynamicImage::ImageRgba8(RgbaImage::from_pixel(3, 1, Rgba([0, 0, 0, 255])));
        let actual = DynamicImage::ImageRgba8(RgbaImage::from_pixel(3, 1, Rgba([255, 0, 0, 255])));
        let ignored = [IgnoreRect { x: 1, y: 0, width: 1, height: 1 }];
        let stats =
            diff_images(&expected, &actual, DiffPolicy { channel_tolerance: 0, ignore: &ignored })
                .expect("same-size in-memory images");
        assert_eq!(stats.ignored_pixels, 1);
        assert_eq!(stats.different_pixels, 2);
    }

    #[test]
    fn user_reference_is_registered_with_exact_dimensions() {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let manifest = load_ui_case_manifest(&workspace.join("compat/ui/cases.toml"))
            .expect("checked-in manifest should parse");
        let reference = manifest
            .references
            .iter()
            .find(|reference| reference.name == "lag-switch-user-reference")
            .expect("reference image should be registered");
        assert_eq!((reference.image_width, reference.image_height), (872, 580));
        assert_eq!(reference.baseline, "../screenshots/lag-switch-user-reference-872x580.png");

        let canonical = manifest
            .cases
            .iter()
            .find(|case| case.name == "canonical-1280x800")
            .expect("canonical automated case should be registered");
        assert_eq!((canonical.logical_width, canonical.logical_height), (1280, 800));
        assert_eq!(canonical.scale, 1.0);

        let image_path = workspace.join("compat/screenshots/lag-switch-user-reference-872x580.png");
        let image = image::open(image_path).expect("checked-in user reference should decode");
        assert_eq!(image.width(), 872);
        assert_eq!(image.height(), 580);
    }
}
