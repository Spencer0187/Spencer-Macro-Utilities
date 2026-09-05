use crate::theme::ThemeDefinition;
use dear_imgui_rs::{StyleColor, Ui};

pub const DEFAULT_SECTION_ORDER: [usize; 16] =
    [0, 1, 2, 15, 3, 4, 5, 6, 13, 7, 8, 9, 10, 11, 12, 14];

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

#[derive(Clone, Debug)]
pub struct MultiInstanceSummary {
    pub section_index: usize,
    pub enabled: Vec<bool>,
    pub selected_instance: usize,
}

#[derive(Clone, Copy)]
pub struct SidebarRenderConfig<'a> {
    pub section_order: &'a [usize; 16],
    pub section_enabled: &'a [bool; 16],
    pub multi_instances: &'a [MultiInstanceSummary],
    pub short_descriptions: bool,
    pub theme: &'a ThemeDefinition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidebarAction {
    SelectSection { section_index: usize },
    SelectInstance { section_index: usize, instance_index: usize },
    AddInstance { section_index: usize },
}

#[must_use]
pub fn section_title(index: usize) -> Option<&'static str> {
    SECTION_CONFIGS.get(index).map(|config| config.title)
}

#[allow(clippy::suboptimal_flops)] // Preserve C++ sidebar layout arithmetic for pixel parity.
pub fn render(
    ui: &Ui,
    left_panel_width: f32,
    selected_section: &mut i32,
    config: SidebarRenderConfig<'_>,
) -> Option<SidebarAction> {
    let SidebarRenderConfig {
        section_order,
        section_enabled,
        multi_instances,
        short_descriptions,
        theme,
    } = config;
    let style = ui.clone_style();
    let button_width = left_panel_width - style.frame_padding()[0] * 2.0;
    let mut action = None;

    for &section_index in section_order {
        let config = SECTION_CONFIGS[section_index];
        let section_active = section_enabled[section_index];
        let selected = usize::try_from(*selected_section).ok() == Some(section_index);
        let base = if section_active { theme.accent_primary } else { theme.disabled_color };
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
        let description_size = if short_descriptions {
            // C++ caches an empty description string, then still runs ImGui::CalcTextSize("").
            // Dear ImGui returns (0, font_size) for empty text, so the hidden description keeps
            // one text-line worth of button height even though no description is drawn.
            ui.calc_text_size_with_opts("", true, button_width - 20.0)
        } else {
            ui.calc_text_size_with_opts(config.description, true, button_width - 20.0)
        };
        let button_height = title_size[1] + description_size[1] + style.frame_padding()[1] * 2.0;
        let scroll_adjustment = if ui.scroll_max_y() == 0.0 { 7.0 } else { 18.0 };
        if ui.button_with_size(
            format!("##SectionButton{section_index}"),
            [button_width - scroll_adjustment, button_height],
        ) {
            *selected_section = i32::try_from(section_index).unwrap_or(-1);
            action = Some(SidebarAction::SelectSection { section_index });
            if multi_instances.iter().any(|summary| summary.section_index == section_index) {
                action = Some(SidebarAction::SelectInstance { section_index, instance_index: 0 });
            }
        }

        let button_position = ui.item_rect_min();
        let mut text_position = [
            button_position[0] + style.frame_padding()[0],
            button_position[1] + style.frame_padding()[1],
        ];
        let draw_list = ui.get_window_draw_list();
        draw_list.add_text(text_position, theme.text_primary, config.title);
        text_position[1] += title_size[1];

        if !short_descriptions {
            let wrap_width = button_width - if ui.scroll_max_y() == 0.0 { 11.0 } else { 22.0 };
            for line in wrap_words(ui, config.description, wrap_width) {
                draw_list.add_text(text_position, theme.text_primary, line);
                text_position[1] += ui.text_line_height();
            }
        }

        drop(active_color);
        drop(hovered_color);
        drop(button_color);

        if let Some(summary) =
            multi_instances.iter().find(|summary| summary.section_index == section_index)
        {
            if let Some(instance_action) =
                render_multi_instance_rows(ui, button_width, config.title, summary, theme)
            {
                if let SidebarAction::SelectInstance { .. } = instance_action {
                    *selected_section = i32::try_from(section_index).unwrap_or(-1);
                }
                action = Some(instance_action);
            }
        } else if matches!(section_index, 5 | 11) {
            render_add_instance_row(ui, button_width, section_index, theme);
        }
        crate::parity::separator(ui);
    }
    action
}

fn render_multi_instance_rows(
    ui: &Ui,
    button_width: f32,
    title: &str,
    summary: &MultiInstanceSummary,
    theme: &ThemeDefinition,
) -> Option<SidebarAction> {
    let width = if ui.scroll_max_y() == 0.0 { button_width - 7.0 } else { button_width - 18.0 };
    let mut action = None;
    for index in 1..summary.enabled.len() {
        let enabled = summary.enabled[index];
        let selected = summary.selected_instance == index;
        let base = if enabled { theme.accent_primary } else { theme.disabled_color };
        let button = if selected { brighten(base, if enabled { 1.4 } else { 1.6 }) } else { base };
        let button_color = ui.push_style_color(StyleColor::Button, button);
        let hovered =
            ui.push_style_color(StyleColor::ButtonHovered, brighten(theme.accent_primary, 1.3));
        let active =
            ui.push_style_color(StyleColor::ButtonActive, brighten(theme.accent_primary, 0.8));
        if ui.button_with_size(
            format!("{title} {}##SubInstance{}_{}", index + 1, summary.section_index, index),
            [width, 0.0],
        ) {
            action = Some(SidebarAction::SelectInstance {
                section_index: summary.section_index,
                instance_index: index,
            });
        }
        drop(active);
        drop(hovered);
        drop(button_color);
    }
    let button = ui.push_style_color(StyleColor::Button, theme.bg_light);
    let hovered = ui.push_style_color(StyleColor::ButtonHovered, brighten(theme.bg_light, 1.3));
    let active = ui.push_style_color(StyleColor::ButtonActive, brighten(theme.bg_light, 1.1));
    if ui.button_with_size(format!("+##AddInstance{}", summary.section_index), [width, 0.0]) {
        action = Some(SidebarAction::AddInstance { section_index: summary.section_index });
    }
    drop(active);
    drop(hovered);
    drop(button);
    action
}

fn render_add_instance_row(
    ui: &Ui,
    button_width: f32,
    section_index: usize,
    theme: &ThemeDefinition,
) {
    let width = if ui.scroll_max_y() == 0.0 { button_width - 7.0 } else { button_width - 18.0 };
    let button = ui.push_style_color(StyleColor::Button, theme.bg_light);
    let hovered = ui.push_style_color(StyleColor::ButtonHovered, brighten(theme.bg_light, 1.3));
    let active = ui.push_style_color(StyleColor::ButtonActive, brighten(theme.bg_light, 1.1));
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
