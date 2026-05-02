#pragma once

#include "key_codes.h"

#include <array>
#include <string>

namespace smu::core {

inline constexpr int kMacroSectionCount = 16;

struct MacroSection {
    std::string title;
    std::string description;
    bool enabled = true;
    bool disableOutsideRoblox = true;
    KeyCode keybind = SMU_VK_NONE;
};

struct LagSwitchUiState {
    bool enabled = false;
    bool currentlyBlocking = false;
    bool inboundHardBlock = true;
    bool outboundHardBlock = true;
    bool fakeLagEnabled = false;
    bool inboundFakeLag = true;
    bool outboundFakeLag = true;
    int fakeLagDelayMs = 0;
    bool targetRobloxOnly = true;
    bool useTcp = false;
    bool preventDisconnect = true;
    bool autoUnblock = false;
    float maxDurationSeconds = 9.0f;
    int unblockDurationMs = 50;
};

struct MacroState {
    std::array<MacroSection, kMacroSectionCount> sections{};
    std::array<int, kMacroSectionCount> sectionOrder{};
    int selectedSection = -1;
    int draggedSection = -1;

    float maxFreezeTime = 9.0f;
    int maxFreezeOverrideMs = 50;
    bool freezeToggleMode = false;
    bool takeAllProcessIds = false;

    int wallhopDx = 300;
    int wallhopDy = -300;
    int wallhopVertical = 0;
    int wallhopDelayMs = 17;
    bool wallhopToggleJump = true;
    bool wallhopToggleFlick = true;

    int speedStrengthX = 959;
    int speedStrengthY = -959;
    bool speedHoldMode = false;

    int pressKeyDelayMs = 16;
    int pressKeyBonusDelayMs = 0;
    KeyCode pressKeyOutput = SMU_VK_D;

    int spamDelayMs = 20;
    KeyCode spamOutput = SMU_VK_LBUTTON;

    LagSwitchUiState lagSwitch;
};

MacroState& GetMacroState();
void InitializeMacroSections(bool shortDescriptions);
void ResetMacroState();

} // namespace smu::core
