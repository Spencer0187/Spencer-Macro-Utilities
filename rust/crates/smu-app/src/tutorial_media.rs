use std::{
    env,
    path::{Path, PathBuf},
};

use dear_imgui_rs::texture::{ManagedTextureId, OwnedTextureData, TextureFormat};
use dear_imgui_rs::{Context, Ui};

const PARITY_CASE: &str = "tutorial-media-user-reference";
const PARITY_CASES: [&str; 5] = [
    PARITY_CASE,
    "tutorial-wallhop-user-reference",
    "tutorial-item-clip-user-reference",
    "tutorial-wall-walk-user-reference",
    "tutorial-spam-key-user-reference",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TutorialImage {
    LaughClip,
    GearClip,
    Wallhop,
    WallWalk,
}

impl TutorialImage {
    const ALL: [Self; 4] = [Self::LaughClip, Self::GearClip, Self::Wallhop, Self::WallWalk];

    const fn index(self) -> usize {
        match self {
            Self::LaughClip => 0,
            Self::GearClip => 1,
            Self::Wallhop => 2,
            Self::WallWalk => 3,
        }
    }

    const fn asset_name(self) -> &'static str {
        match self {
            Self::LaughClip => "macro_tutorials/laugh.jpg",
            Self::GearClip => "macro_tutorials/gear-clip.jpg",
            Self::Wallhop => "macro_tutorials/wallhop.jpg",
            Self::WallWalk => "macro_tutorials/wallwalk.jpg",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TutorialTexture {
    id: ManagedTextureId,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TutorialMedia {
    textures: [Option<TutorialTexture>; 4],
    parity_placeholder: bool,
}

impl TutorialMedia {
    pub fn load(context: &mut Context, parity_case: Option<&str>) -> (Self, Vec<String>) {
        if parity_case.is_some() {
            return (
                Self {
                    parity_placeholder: parity_case
                        .is_some_and(|case| PARITY_CASES.contains(&case)),
                    ..Self::default()
                },
                Vec::new(),
            );
        }

        let mut media = Self::default();
        let mut warnings = Vec::new();
        for image in TutorialImage::ALL {
            match load_texture(context, image) {
                Ok(texture) => media.textures[image.index()] = Some(texture),
                Err(message) => warnings.push(message),
            }
        }
        (media, warnings)
    }

    pub fn render_centered(
        self,
        ui: &Ui,
        image: TutorialImage,
        display_width: f32,
        display_height: f32,
    ) {
        if self.parity_placeholder {
            render_centered_placeholder(ui, display_width, display_height);
            return;
        }
        let Some(texture) = self.textures[image.index()] else {
            return;
        };
        render_centered_texture(ui, texture, display_width, display_height);
    }
}

fn load_texture(context: &mut Context, image: TutorialImage) -> Result<TutorialTexture, String> {
    let asset_name = image.asset_name();
    let path = resolve_asset_path(asset_name)
        .ok_or_else(|| format!("Macro tutorial image is missing: assets/{asset_name}"))?;
    let decoded = image::open(&path)
        .map_err(|error| {
            format!("Macro tutorial image could not be loaded: {}: {error}", path.display())
        })?
        .into_rgba8();
    let (width, height) = decoded.dimensions();
    let owned =
        OwnedTextureData::from_pixels(TextureFormat::RGBA32, width, height, decoded.as_raw())
            .map_err(|error| {
                format!("Macro tutorial image texture is invalid: {}: {error}", path.display())
            })?;
    let id = context.register_texture(owned);
    Ok(TutorialTexture { id, width, height })
}

#[allow(clippy::cast_precision_loss)] // Tutorial assets are small image dimensions well below f32's exact integer range.
fn render_centered_texture(
    ui: &Ui,
    texture: TutorialTexture,
    display_width: f32,
    display_height: f32,
) {
    let native_width = texture.width as f32;
    let native_height = texture.height as f32;
    let aspect = native_width / native_height;
    let mut width = if display_width > 0.0 { display_width } else { native_width };
    let mut height = if display_height > 0.0 { display_height } else { native_height };
    if display_width > 0.0 && display_height <= 0.0 {
        height = width / aspect;
    } else if display_height > 0.0 && display_width <= 0.0 {
        width = height * aspect;
    }
    let available_width = ui.content_region_avail()[0];
    if available_width > 0.0 && width > available_width {
        let scale = available_width / width;
        width *= scale;
        height *= scale;
    }
    let offset = ((available_width - width) * 0.5).max(0.0);
    ui.set_cursor_pos_x(ui.cursor_pos_x() + offset);
    ui.image(texture.id, [width, height]);
}

fn render_centered_placeholder(ui: &Ui, display_width: f32, display_height: f32) {
    let available_width = ui.content_region_avail()[0];
    let mut width = display_width.max(1.0);
    let mut height = display_height.max(1.0);
    if available_width > 0.0 && width > available_width {
        let scale = available_width / width;
        width *= scale;
        height *= scale;
    }
    let offset = ((available_width - width) * 0.5).max(0.0);
    ui.set_cursor_pos_x(ui.cursor_pos_x() + offset);
    let top_left = ui.cursor_screen_pos();
    let bottom_right = [top_left[0] + width, top_left[1] + height];
    let label = "Tutorial image";
    let label_size = ui.calc_text_size(label);
    let label_pos = [
        (width - label_size[0]).mul_add(0.5, top_left[0]),
        (height - label_size[1]).mul_add(0.5, top_left[1]),
    ];
    let draw = ui.get_window_draw_list();
    draw.add_rect(top_left, bottom_right, ui.style_color(dear_imgui_rs::StyleColor::Border))
        .build();
    draw.add_text(label_pos, ui.style_color(dear_imgui_rs::StyleColor::TextDisabled), label);
    ui.dummy([width, height]);
}

fn resolve_asset_path(asset_name: &str) -> Option<PathBuf> {
    let executable_directory =
        env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf));
    let app_directories = ["SMU_APPDIR", "APPDIR"]
        .into_iter()
        .filter_map(env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    resolve_asset_path_from(
        asset_name,
        executable_directory.as_deref(),
        &app_directories,
        &source_root(),
        &env::current_dir().unwrap_or_default(),
    )
}

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn resolve_asset_path_from(
    asset_name: &str,
    executable_directory: Option<&Path>,
    app_directories: &[PathBuf],
    source_root: &Path,
    current_directory: &Path,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(executable_directory) = executable_directory {
        candidates.push(executable_directory.join("assets").join(asset_name));
    }
    for app_directory in app_directories {
        candidates.push(app_directory.join("usr/bin/assets").join(asset_name));
        candidates.push(app_directory.join("assets").join(asset_name));
    }
    candidates.push(source_root.join("assets").join(asset_name));
    candidates.push(current_directory.join("assets").join(asset_name));
    candidates.into_iter().find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::{TutorialImage, resolve_asset_path_from};
    use std::{fs, path::PathBuf};

    #[test]
    fn tutorial_asset_names_match_cpp_catalog() {
        assert_eq!(TutorialImage::LaughClip.asset_name(), "macro_tutorials/laugh.jpg");
        assert_eq!(TutorialImage::GearClip.asset_name(), "macro_tutorials/gear-clip.jpg");
        assert_eq!(TutorialImage::Wallhop.asset_name(), "macro_tutorials/wallhop.jpg");
        assert_eq!(TutorialImage::WallWalk.asset_name(), "macro_tutorials/wallwalk.jpg");
    }

    #[test]
    fn source_tutorial_assets_decode_with_nonzero_dimensions()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = super::source_root();
        for image in TutorialImage::ALL {
            let decoded = image::open(root.join("assets").join(image.asset_name()))?;
            assert!(decoded.width() > 0);
            assert!(decoded.height() > 0);
        }
        Ok(())
    }

    #[test]
    fn packaged_tutorial_asset_wins_over_source_fallback() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = std::env::temp_dir().join(format!("smu-tutorial-media-{}", std::process::id()));
        let executable = root.join("exe");
        let source = root.join("source");
        let current = root.join("current");
        let appdirs: Vec<PathBuf> = Vec::new();
        fs::create_dir_all(executable.join("assets/macro_tutorials"))?;
        fs::create_dir_all(source.join("assets/macro_tutorials"))?;
        fs::create_dir_all(&current)?;
        fs::write(executable.join("assets/macro_tutorials/wallhop.jpg"), b"packaged")?;
        fs::write(source.join("assets/macro_tutorials/wallhop.jpg"), b"source")?;
        assert_eq!(
            resolve_asset_path_from(
                "macro_tutorials/wallhop.jpg",
                Some(&executable),
                &appdirs,
                &source,
                &current,
            ),
            Some(executable.join("assets/macro_tutorials/wallhop.jpg"))
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
