#define NOMINMAX
#include "Resource Files/theme_manager.h"
#include "Resource Files/globals.h"
#include <iostream>

using namespace Globals;

static const Theme default_dark_theme = {
    "Default Dark",
    ImVec4(21/255.0f, 22/255.0f, 23/255.0f, 255/255.0f), // bg_dark (WindowBg)
    ImVec4(21/255.0f, 22/255.0f, 23/255.0f, 255/255.0f), // bg_medium (ChildBg/PopupBg) - ImGui default is same as Window
    ImVec4(0.16f, 0.29f, 0.48f, 0.54f), // bg_light (FrameBg)
    ImVec4(0.26f, 0.59f, 0.98f, 0.40f), // accent_primary (Button)
    ImVec4(0.26f, 0.59f, 0.98f, 1.00f), // accent_secondary (ButtonHovered)
    ImVec4(1.00f, 1.00f, 1.00f, 1.00f), // text_primary
    ImVec4(0.50f, 0.50f, 0.50f, 1.00f), // text_secondary
	ImVec4(0.00f, 1.00f, 0.00f, 1.00f), // success_color (Checkmark/Active)
	ImVec4(0.90f, 0.60f, 0.20f, 1.00f), // warning_color
    ImVec4(1.00f, 0.00f, 0.00f, 1.00f), // error_color
    ImVec4(0.43f, 0.43f, 0.50f, 0.50f), // border_color
	ImVec4(0.45f, 0.29f, 0.15f, 1.00f), // disabled_color
    0.0f, // window_rounding
    0.0f, // frame_rounding
    0.0f  // button_rounding
};

// Default themes definition
static std::vector<Theme> defaultthemes = {
	default_dark_theme,
    {"Modern Dark", ImVec4(0.08f, 0.08f, 0.12f, 1.0f), ImVec4(0.12f, 0.12f, 0.16f, 1.0f),
	 ImVec4(0.16f, 0.16f, 0.20f, 1.0f), ImVec4(0.11f, 0.36f, 0.60f, 1.0f),
	 ImVec4(0.06f, 0.26f, 0.46f, 1.0f), ImVec4(0.95f, 0.95f, 0.95f, 1.0f),
     ImVec4(0.70f, 0.70f, 0.70f, 1.0f), ImVec4(0.20f, 0.80f, 0.40f, 1.0f),
     ImVec4(0.90f, 0.60f, 0.20f, 1.0f), ImVec4(0.90f, 0.30f, 0.30f, 1.0f),
	 ImVec4(0.20f, 0.20f, 0.25f, 1.0f), ImVec4(0.45f, 0.29f, 0.15f, 1.00f), 8.0f, 4.0f, 4.0f},
    {"Cyberpunk", ImVec4(0.05f, 0.05f, 0.10f, 1.0f), ImVec4(0.10f, 0.10f, 0.15f, 1.0f),
	 ImVec4(0.15f, 0.15f, 0.20f, 1.0f), ImVec4(0.0f, 0.49f, 0.62f, 1.0f),
     ImVec4(0.00f, 0.44f, 0.60f, 1.0f), ImVec4(0.90f, 0.90f, 1.0f, 1.0f),
     ImVec4(0.60f, 0.60f, 0.80f, 1.0f), ImVec4(0.00f, 1.0f, 0.50f, 1.0f),
     ImVec4(1.0f, 0.50f, 0.00f, 1.0f), ImVec4(1.0f, 0.20f, 0.40f, 1.0f),
	 ImVec4(0.00f, 0.40f, 0.60f, 1.0f), ImVec4(0.41f, 0.02f, 0.79f, 1.0f), 6.0f, 3.0f, 3.0f},
    {"Forest Green", ImVec4(0.05f, 0.12f, 0.08f, 1.0f), ImVec4(0.08f, 0.16f, 0.12f, 1.0f),
	 ImVec4(0.12f, 0.20f, 0.16f, 1.0f), ImVec4(0.11f, 0.55f, 0.26f, 1.0f),
	 ImVec4(0.11f, 0.43f, 0.21f, 1.0f), ImVec4(0.90f, 0.95f, 0.90f, 1.0f),
     ImVec4(0.70f, 0.80f, 0.70f, 1.0f), ImVec4(0.30f, 0.90f, 0.50f, 1.0f),
     ImVec4(0.90f, 0.70f, 0.20f, 1.0f), ImVec4(0.80f, 0.30f, 0.30f, 1.0f),
	 ImVec4(0.15f, 0.30f, 0.20f, 1.0f), ImVec4(0.45f, 0.50f, 0.00f, 1.0f), 10.0f, 5.0f, 5.0f},
    {"Sunset Orange", ImVec4(0.12f, 0.08f, 0.05f, 1.0f), ImVec4(0.16f, 0.12f, 0.08f, 1.0f),
     ImVec4(0.20f, 0.16f, 0.12f, 1.0f), ImVec4(1.0f, 0.50f, 0.20f, 1.0f),
     ImVec4(0.80f, 0.40f, 0.15f, 1.0f), ImVec4(1.0f, 0.95f, 0.90f, 1.0f),
     ImVec4(0.80f, 0.70f, 0.60f, 1.0f), ImVec4(0.40f, 0.80f, 0.30f, 1.0f),
     ImVec4(1.0f, 0.70f, 0.20f, 1.0f), ImVec4(0.90f, 0.30f, 0.30f, 1.0f),
	 ImVec4(0.30f, 0.20f, 0.15f, 1.0f), ImVec4(0.45f, 0.29f, 0.15f, 1.00f), 12.0f, 6.0f, 6.0f},
    {"Purple Haze", ImVec4(0.10f, 0.05f, 0.15f, 1.0f), ImVec4(0.15f, 0.10f, 0.20f, 1.0f),
	 ImVec4(0.20f, 0.15f, 0.25f, 1.0f), ImVec4(0.58f, 0.32f, 0.7f, 1.0f),
	 ImVec4(0.43f, 0.20f, 0.58f, 1.0f), ImVec4(0.95f, 0.90f, 1.0f, 1.0f),
     ImVec4(0.70f, 0.60f, 0.80f, 1.0f), ImVec4(0.40f, 0.80f, 0.60f, 1.0f),
     ImVec4(1.0f, 0.60f, 0.40f, 1.0f), ImVec4(0.90f, 0.30f, 0.50f, 1.0f),
	 ImVec4(0.25f, 0.15f, 0.30f, 1.0f), ImVec4(0.35f, 0.40f, 0.0f, 1.0f), 8.0f, 4.0f, 4.0f}};

void ThemeManager::Initialize() {
    // Populate the global themes vector
    Globals::themes = defaultthemes;
    // Initialize custom theme with the first default
    Globals::custom_theme = defaultthemes[0];
    Globals::custom_theme.name = "Custom Theme";
}

void ThemeManager::ApplyTheme() {
    Theme& theme = Globals::GetCurrentTheme();
    ImGuiStyle &style = ImGui::GetStyle();

    // 1. Apply Rounding (Themes control this)
    style.WindowRounding = theme.window_rounding;
    style.ChildRounding = theme.window_rounding * 0.75f;
    style.FrameRounding = theme.frame_rounding;
    style.PopupRounding = theme.window_rounding * 0.75f;
    style.ScrollbarRounding = theme.frame_rounding;
    style.GrabRounding = theme.button_rounding;
    style.TabRounding = theme.frame_rounding;

    // 2. Apply Colors
    style.Colors[ImGuiCol_WindowBg] = theme.bg_dark;
    style.Colors[ImGuiCol_ChildBg] = theme.bg_medium;
    style.Colors[ImGuiCol_PopupBg] = theme.bg_medium;
    style.Colors[ImGuiCol_Border] = theme.border_color;
    style.Colors[ImGuiCol_BorderShadow] = ImVec4(0.00f, 0.00f, 0.00f, 0.00f);
        
    style.Colors[ImGuiCol_FrameBg] = theme.bg_light;
    // Make hover slightly lighter
    style.Colors[ImGuiCol_FrameBgHovered] = ImVec4(
        std::min(theme.bg_light.x + 0.1f, 1.0f), 
        std::min(theme.bg_light.y + 0.1f, 1.0f), 
        std::min(theme.bg_light.z + 0.1f, 1.0f), 
        theme.bg_light.w);
    style.Colors[ImGuiCol_FrameBgActive] = theme.accent_primary;
        
    style.Colors[ImGuiCol_TitleBg] = theme.bg_dark;
    style.Colors[ImGuiCol_TitleBgActive] = theme.bg_dark;
    style.Colors[ImGuiCol_TitleBgCollapsed] = theme.bg_dark;
        
    style.Colors[ImGuiCol_MenuBarBg] = theme.bg_medium;
    style.Colors[ImGuiCol_ScrollbarBg] = theme.bg_medium;
    style.Colors[ImGuiCol_ScrollbarGrab] = theme.bg_light; 
    style.Colors[ImGuiCol_ScrollbarGrabHovered] = theme.accent_primary;
    style.Colors[ImGuiCol_ScrollbarGrabActive] = theme.accent_secondary;
        
    style.Colors[ImGuiCol_CheckMark] = theme.success_color; 
        
    style.Colors[ImGuiCol_SliderGrab] = theme.accent_primary;
    style.Colors[ImGuiCol_SliderGrabActive] = theme.accent_secondary;
        
    // Base button colors (Main UI buttons, not the sidebar ones which are handled manually)
    style.Colors[ImGuiCol_Button] = theme.accent_primary;
    style.Colors[ImGuiCol_ButtonHovered] = theme.accent_secondary;
    style.Colors[ImGuiCol_ButtonActive] = theme.accent_secondary;
        
    style.Colors[ImGuiCol_Header] = theme.accent_primary;
    style.Colors[ImGuiCol_HeaderHovered] = theme.accent_secondary;
    style.Colors[ImGuiCol_HeaderActive] = theme.accent_secondary;
        
    style.Colors[ImGuiCol_Separator] = theme.border_color;
    style.Colors[ImGuiCol_SeparatorHovered] = theme.accent_primary;
    style.Colors[ImGuiCol_SeparatorActive] = theme.accent_secondary;
        
    style.Colors[ImGuiCol_ResizeGrip] = theme.accent_primary;
    style.Colors[ImGuiCol_ResizeGripHovered] = theme.accent_secondary;
    style.Colors[ImGuiCol_ResizeGripActive] = theme.accent_secondary;
        
    style.Colors[ImGuiCol_Tab] = theme.bg_light;
    style.Colors[ImGuiCol_TabHovered] = theme.accent_primary;
    style.Colors[ImGuiCol_TabActive] = theme.accent_primary;
    style.Colors[ImGuiCol_TabUnfocused] = theme.bg_light;
    style.Colors[ImGuiCol_TabUnfocusedActive] = theme.accent_primary;
        
    style.Colors[ImGuiCol_Text] = theme.text_primary;
    style.Colors[ImGuiCol_TextDisabled] = theme.text_secondary;
    style.Colors[ImGuiCol_NavHighlight] = theme.accent_primary;
    style.Colors[ImGuiCol_ModalWindowDimBg] = ImVec4(0.00f, 0.00f, 0.00f, 0.60f);
}

void ThemeManager::RenderThemeMenu(bool* p_open) {
    if (!*p_open) return;

    ImVec2 main_window_size = ImGui::GetIO().DisplaySize;
    float child_width = main_window_size.x * 0.5f;
    float child_height = main_window_size.y * 0.7f; // Slightly taller for color pickers

    ImVec2 child_pos = ImVec2(
        (main_window_size.x * 0.3f),
        (main_window_size.y - child_height - 90)
    );

    ImGui::SetNextWindowPos(child_pos, ImGuiCond_Once);
    ImGui::SetNextWindowSize(ImVec2(child_width, child_height), ImGuiCond_Always);

    ImGuiWindowFlags window_flags = ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoCollapse;

    if (ImGui::Begin("Theme Editor", p_open, window_flags)) {
        
        // Theme selector
        ImGui::TextColored(Globals::GetCurrentTheme().text_primary, "SELECT THEME");
        ImGui::Separator();
        
        std::vector<const char *> theme_names;
        for (const auto &theme : Globals::themes)
            theme_names.push_back(theme.name.c_str());
        theme_names.push_back(Globals::custom_theme.name.c_str());
        
        if (ImGui::Combo("Theme", &Globals::current_theme_index, theme_names.data(), theme_names.size())) {
            Globals::theme_modified = true;
        }

        ImGui::Spacing();
        ImGui::TextColored(Globals::GetCurrentTheme().text_primary, "CUSTOMIZE COLORS");
        ImGui::Separator();

        Theme *editing_theme = (Globals::current_theme_index < Globals::themes.size())
                ? &Globals::themes[Globals::current_theme_index]
                : &Globals::custom_theme;

        if (editing_theme) {
            ImGui::BeginChild("ColorEditor", ImVec2(0, -35), true); // Leave room for reset button
            
            if (ImGui::CollapsingHeader("Background Colors", ImGuiTreeNodeFlags_DefaultOpen)) {
                ImGui::ColorEdit4("Dark Background", (float *)&editing_theme->bg_dark);
                ImGui::ColorEdit4("Medium Background", (float *)&editing_theme->bg_medium);
                ImGui::ColorEdit4("Light Background", (float *)&editing_theme->bg_light);
            }
            
            if (ImGui::CollapsingHeader("Accent Colors", ImGuiTreeNodeFlags_DefaultOpen)) {
                ImGui::ColorEdit4("Primary Accent", (float *)&editing_theme->accent_primary);
                ImGui::ColorEdit4("Secondary Accent", (float *)&editing_theme->accent_secondary);
            }
            
            if (ImGui::CollapsingHeader("Text & Status")) {
                ImGui::ColorEdit4("Primary Text", (float *)&editing_theme->text_primary);
                ImGui::ColorEdit4("Secondary Text", (float *)&editing_theme->text_secondary);
                ImGui::ColorEdit4("Success", (float *)&editing_theme->success_color);
                ImGui::ColorEdit4("Warning", (float *)&editing_theme->warning_color);
                ImGui::ColorEdit4("Error", (float *)&editing_theme->error_color);
                ImGui::ColorEdit4("Border", (float *)&editing_theme->border_color);
                ImGui::ColorEdit4("Disabled", (float *)&editing_theme->disabled_color);
            }

            if (ImGui::CollapsingHeader("Rounding", ImGuiTreeNodeFlags_DefaultOpen)) {
                ImGui::SliderFloat("Window Rounding", &editing_theme->window_rounding, 0.0f, 20.0f, "%.1f");
                ImGui::SliderFloat("Frame Rounding", &editing_theme->frame_rounding, 0.0f, 20.0f, "%.1f");
                ImGui::SliderFloat("Button Rounding", &editing_theme->button_rounding, 0.0f, 20.0f, "%.1f");
            }

            ImGui::Spacing();
            if (Globals::current_theme_index == Globals::themes.size()) {
                ImGui::Text("Custom Theme Name");
                static char theme_name[256];
                strncpy(theme_name, editing_theme->name.c_str(), sizeof(theme_name) - 1);
                if (ImGui::InputText("##ThemeName", theme_name, sizeof(theme_name)))
                    editing_theme->name = theme_name;
            }
            
            ImGui::EndChild();

            if (ImGui::Button("Reset to Default", ImVec2(150, 30))) {
                if (Globals::current_theme_index < Globals::themes.size())
                    Globals::themes[Globals::current_theme_index] = defaultthemes[Globals::current_theme_index];
                else
                    Globals::custom_theme = defaultthemes[0];

                if (Globals::current_theme_index == Globals::themes.size()) {
                    Globals::custom_theme.name = "Custom Theme";
		        }

                Globals::theme_modified = true;
            }
        }
    }
    ImGui::End();
}