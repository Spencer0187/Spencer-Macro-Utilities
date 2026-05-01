#pragma once

#include "imgui-files/imgui.h"

#include <optional>

namespace SmuImGui {

inline bool DisabledCheckbox(const char* label,
                             bool* value,
                             const char* disabledTooltip = nullptr,
                             std::optional<bool> forcedValue = std::nullopt)
{
    bool localValue = value ? *value : false;
    if (forcedValue.has_value()) {
        localValue = *forcedValue;
        if (value) {
            *value = localValue;
        }
    }

    ImGui::BeginDisabled(true);
    ImGui::Checkbox(label, &localValue);
    ImGui::EndDisabled();

    if (disabledTooltip && disabledTooltip[0] != '\0' &&
        ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled)) {
        ImGui::SetTooltip("%s", disabledTooltip);
    }

    if (forcedValue.has_value() && value) {
        *value = *forcedValue;
    }

    return false;
}

} // namespace SmuImGui
