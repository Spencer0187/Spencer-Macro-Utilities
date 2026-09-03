use dear_imgui_rs::{StyleColor, Ui};

const TEXT_PRIMARY: [f32; 4] = [0.90, 0.90, 1.0, 1.0];
const ACCENT_PRIMARY: [f32; 4] = [0.0, 0.49, 0.62, 1.0];
const BG_LIGHT: [f32; 4] = [0.15, 0.15, 0.20, 1.0];
const DISABLED_COLOR: [f32; 4] = [0.41, 0.02, 0.79, 1.0];
const DEFAULT_SECTION_ORDER: [usize; 16] = [0, 1, 2, 15, 3, 4, 5, 6, 13, 7, 8, 9, 10, 11, 12, 14];

#[derive(Clone, Copy)]
struct SectionConfig {
    title: &'static str,
    description: &'static str,
}

impl SectionConfig {
    const fn new(title: &'static str, description: &'static str) -> Self {
        Self { title, description }
    }
}

const SECTION_CONFIGS: [SectionConfig; 16] = [
    SectionConfig::new("Freeze", "Freeze the Roblox process."),
    SectionConfig::new("Item Desync", "Desynchronize a held item from the client and server."),
    SectionConfig::new(
        "Wall Helicopter High Jump",
        "Use COM offset to launch yourself high into the air.",
    ),
    SectionConfig::new("Speedglitch", "Use COM offset to gain extreme midair speed."),
    SectionConfig::new(
        "Item Unequip COM Offset",
        "Create a COM offset by using an emote and unequipping an item.",
    ),
    SectionConfig::new("Press a Button", "Press another key for a short moment."),
    SectionConfig::new("Wallhop/Rotation", "Flick and jump automatically to perform wallhops."),
    SectionConfig::new("Walless LHJ", "Perform a lag high jump without using a wall."),
    SectionConfig::new("Item Clip", "Clip through thin walls using item equip timing."),
    SectionConfig::new("Laugh Clip", "Perform a laugh clip automatically."),
    SectionConfig::new("Wall-Walk", "Walk along wall seams without jumping."),
    SectionConfig::new("Spam a Key", "Spam a selected key while the macro is active."),
    SectionConfig::new(
        "Ledge Bounce",
        "Drop from a ledge and bounce back with timed camera movement.",
    ),
    SectionConfig::new(
        "Smart Bunnyhop",
        "Automatically bunnyhop while avoiding chat/input conflicts.",
    ),
    SectionConfig::new("Floor Bounce", "Perform a high jump from flat ground."),
    SectionConfig::new("Lag Switch", "Temporarily block or delay Roblox network traffic."),
];

#[allow(clippy::suboptimal_flops)] // Preserve C++ sidebar layout arithmetic for pixel parity.
pub fn render(ui: &Ui, left_panel_width: f32) {
    let style = ui.clone_style();
    let button_width = left_panel_width - style.frame_padding()[0] * 2.0;

    for section_index in DEFAULT_SECTION_ORDER {
        let config = SECTION_CONFIGS[section_index];
        let section_active = section_index == 15;
        let selected = section_index == 15;
        let base = if section_active { ACCENT_PRIMARY } else { DISABLED_COLOR };
        let button =
            if selected { brighten(base, if section_active { 1.4 } else { 1.6 }) } else { base };
        let hovered = if selected {
            brighten(base, if section_active { 1.6 } else { 1.9 })
        } else {
            brighten(base, 1.3)
        };
        let active = if selected {
            brighten(base, if section_active { 1.7 } else { 2.1 })
        } else if section_active {
            brighten(base, 0.8)
        } else {
            brighten(base, 1.1)
        };

        let button_color = ui.push_style_color(StyleColor::Button, button);
        let hovered_color = ui.push_style_color(StyleColor::ButtonHovered, hovered);
        let active_color = ui.push_style_color(StyleColor::ButtonActive, active);

        let title_size = ui.calc_text_size_with_opts(config.title, true, -1.0);
        let description_size =
            ui.calc_text_size_with_opts(config.description, true, button_width - 20.0);
        let button_height = title_size[1] + description_size[1] + style.frame_padding()[1] * 2.0;
        let scroll_adjustment = if ui.scroll_max_y() == 0.0 { 7.0 } else { 18.0 };
        let _ = ui.button_with_size(
            format!("##SectionButton{section_index}"),
            [button_width - scroll_adjustment, button_height],
        );

        let button_position = ui.item_rect_min();
        let mut text_position = [
            button_position[0] + style.frame_padding()[0],
            button_position[1] + style.frame_padding()[1],
        ];
        let draw_list = ui.get_window_draw_list();
        draw_list.add_text(text_position, TEXT_PRIMARY, config.title);
        text_position[1] += title_size[1];

        let wrap_width = button_width - if ui.scroll_max_y() == 0.0 { 11.0 } else { 22.0 };
        for line in wrap_words(ui, config.description, wrap_width) {
            draw_list.add_text(text_position, TEXT_PRIMARY, line);
            text_position[1] += ui.text_line_height();
        }

        drop(active_color);
        drop(hovered_color);
        drop(button_color);

        if matches!(section_index, 5 | 6 | 11) {
            render_add_instance_row(ui, button_width, section_index);
        }
        if section_index == 14 {
            ui.separator();
        } else {
            crate::parity::separator(ui);
        }
    }
}

fn render_add_instance_row(ui: &Ui, button_width: f32, section_index: usize) {
    let width = if ui.scroll_max_y() == 0.0 { button_width - 7.0 } else { button_width - 18.0 };
    let button = ui.push_style_color(StyleColor::Button, BG_LIGHT);
    let hovered = ui.push_style_color(StyleColor::ButtonHovered, brighten(BG_LIGHT, 1.3));
    let active = ui.push_style_color(StyleColor::ButtonActive, brighten(BG_LIGHT, 1.1));
    let _ = ui.button_with_size(format!("+##AddInstance{section_index}"), [width, 0.0]);
    drop(active);
    drop(hovered);
    drop(button);
}

fn wrap_words(ui: &Ui, text: &str, wrap_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let potential =
            if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
        if ui.calc_text_size(&potential)[0] > wrap_width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            word.clone_into(&mut current);
        } else {
            current = potential;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn brighten(color: [f32; 4], factor: f32) -> [f32; 4] {
    [
        (color[0] * factor).min(1.0),
        (color[1] * factor).min(1.0),
        (color[2] * factor).min(1.0),
        color[3],
    ]
}
