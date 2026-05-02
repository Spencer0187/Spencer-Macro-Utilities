#include "app_ui.h"

#include "input_actions.h"
#include "../core/app_state.h"
#include "../core/key_codes.h"
#include "../core/macro_state.h"
#include "../platform/input_backend.h"
#include "../platform/logging.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"

#include "imgui.h"

#include <algorithm>
#include <array>
#include <cstdio>
#include <cstring>
#include <sstream>
#include <string>
#include <vector>

namespace smu::app {
namespace {

struct BindingState {
    bool bindingMode = false;
    std::string buttonText = "Click to Bind Key";
    char keyBuffer[64] = "0x00";
    char keyBufferHuman[64] = "None";
};

std::array<BindingState, smu::core::kMacroSectionCount> g_sectionBindingStates;
BindingState g_pressKeyOutputBindingState;
BindingState g_spamOutputBindingState;

const char* kForegroundFallbackTooltip =
    "Foreground Roblox window detection is unavailable on this display server. This macro has been forced to always-active mode.";

void CopyString(char* destination, std::size_t size, const std::string& source)
{
    if (!destination || size == 0) {
        return;
    }
    std::snprintf(destination, size, "%s", source.c_str());
}

std::string FormatHexKeyString(smu::core::KeyCode combinedCode)
{
    const smu::core::KeyCode vk = combinedCode & smu::core::HOTKEY_KEY_MASK;
    std::string text;

    if (combinedCode & smu::core::HOTKEY_MASK_WIN) text += "0x5B + ";
    if (combinedCode & smu::core::HOTKEY_MASK_CTRL) text += "0x11 + ";
    if (combinedCode & smu::core::HOTKEY_MASK_ALT) text += "0x12 + ";
    if (combinedCode & smu::core::HOTKEY_MASK_SHIFT) text += "0x10 + ";

    char keyText[32] = {};
    std::snprintf(keyText, sizeof(keyText), "0x%X", vk);
    text += keyText;
    return text;
}

std::string FormatKeyName(smu::core::KeyCode combinedCode)
{
    const smu::core::KeyCode vk = combinedCode & smu::core::HOTKEY_KEY_MASK;
    std::string text;

    if (combinedCode & smu::core::HOTKEY_MASK_WIN) text += "Win + ";
    if (combinedCode & smu::core::HOTKEY_MASK_CTRL) text += "Ctrl + ";
    if (combinedCode & smu::core::HOTKEY_MASK_ALT) text += "Alt + ";
    if (combinedCode & smu::core::HOTKEY_MASK_SHIFT) text += "Shift + ";

    if (auto backend = smu::platform::GetInputBackend()) {
        text += backend->formatKeyName(vk);
    } else {
        text += smu::core::FormatKeyCodeFallback(vk);
    }

    return text;
}

smu::core::KeyCode CurrentModifierMask()
{
    smu::core::KeyCode modifiers = 0;
    if (IsKeyPressed(smu::core::SMU_VK_LWIN) || IsKeyPressed(smu::core::SMU_VK_RWIN)) {
        modifiers |= smu::core::HOTKEY_MASK_WIN;
    }
    if (IsKeyPressed(smu::core::SMU_VK_CONTROL) || IsKeyPressed(smu::core::SMU_VK_LCONTROL) || IsKeyPressed(smu::core::SMU_VK_RCONTROL)) {
        modifiers |= smu::core::HOTKEY_MASK_CTRL;
    }
    if (IsKeyPressed(smu::core::SMU_VK_MENU) || IsKeyPressed(smu::core::SMU_VK_LMENU) || IsKeyPressed(smu::core::SMU_VK_RMENU)) {
        modifiers |= smu::core::HOTKEY_MASK_ALT;
    }
    if (IsKeyPressed(smu::core::SMU_VK_SHIFT) || IsKeyPressed(smu::core::SMU_VK_LSHIFT) || IsKeyPressed(smu::core::SMU_VK_RSHIFT)) {
        modifiers |= smu::core::HOTKEY_MASK_SHIFT;
    }
    return modifiers;
}

void RefreshBindingBuffers(BindingState& state, smu::core::KeyCode key)
{
    CopyString(state.keyBuffer, sizeof(state.keyBuffer), FormatHexKeyString(key));
    CopyString(state.keyBufferHuman, sizeof(state.keyBufferHuman), FormatKeyName(key));
}

smu::core::KeyCode BindKeyMode(BindingState& state, smu::core::KeyCode currentKey)
{
    auto& appState = smu::core::GetAppState();

    if (!state.bindingMode) {
        appState.notBinding = true;
        state.buttonText = "Click to Bind Key";
        RefreshBindingBuffers(state, currentKey);
        return currentKey;
    }

    appState.notBinding = false;
    state.buttonText = "Press a Key...";

    const smu::core::KeyCode modifiers = CurrentModifierMask();
    if (modifiers == 0) {
        CopyString(state.keyBuffer, sizeof(state.keyBuffer), "Waiting...");
        CopyString(state.keyBufferHuman, sizeof(state.keyBufferHuman), "Waiting...");
    } else {
        CopyString(state.keyBuffer, sizeof(state.keyBuffer), FormatHexKeyString(modifiers));
        CopyString(state.keyBufferHuman, sizeof(state.keyBufferHuman), FormatKeyName(modifiers));
    }

    auto backend = smu::platform::GetInputBackend();
    if (!backend) {
        return currentKey;
    }

    const auto pressedKey = backend->getCurrentPressedKey();
    if (!pressedKey || smu::core::IsModifierKey(*pressedKey)) {
        return currentKey;
    }

    const smu::core::KeyCode finalCombo = (*pressedKey & smu::core::HOTKEY_KEY_MASK) | modifiers;
    state.bindingMode = false;
    appState.notBinding = true;
    state.buttonText = "Click to Bind Key";
    RefreshBindingBuffers(state, finalCombo);
    return finalCombo;
}

void RenderCapabilities(const AppContext& context)
{
    if (ImGui::BeginTable("CapabilityTable", 2, ImGuiTableFlags_SizingStretchProp)) {
        auto row = [](const char* label, bool value) {
            ImGui::TableNextRow();
            ImGui::TableSetColumnIndex(0);
            ImGui::TextUnformatted(label);
            ImGui::TableSetColumnIndex(1);
            ImGui::TextColored(value ? ImVec4(0.25f, 0.85f, 0.45f, 1.0f) : ImVec4(0.95f, 0.45f, 0.35f, 1.0f),
                "%s", value ? "available" : "unavailable");
        };

        row("Global input injection", context.capabilities.canInjectGlobalInput && context.inputBackendAvailable);
        row("Global input reading", context.capabilities.canReadGlobalInput && context.inputBackendAvailable);
        row("Foreground process detection", context.capabilities.canDetectForegroundProcess);
        row("Process suspension", context.capabilities.canSuspendProcesses && context.processBackendAvailable);
        row("Network lag switch", context.capabilities.canUseNetworkLagSwitch && context.networkBackendAvailable);
        ImGui::EndTable();
    }
}

void RenderGlobalSettings(AppContext& context)
{
    auto& appState = smu::core::GetAppState();

    ImGui::TextUnformatted("Global Settings");
    ImGui::SameLine();
    ImGui::TextDisabled("Display server: %s", context.capabilities.displayServer.c_str());
    ImGui::SameLine(ImGui::GetWindowWidth() - 130.0f);
    ImGui::Text("VERSION %s", appState.localVersion.c_str());

    ImGui::Checkbox("Macro Toggle", &appState.macroToggled);
    ImGui::SameLine();
    ImGui::Checkbox("Anti-AFK", &appState.antiAfkToggle);
    ImGui::SameLine();
    ImGui::Checkbox("Cam-Fix", &appState.camFixToggle);

    ImGui::SetNextItemWidth(260.0f);
    ImGui::InputText("Roblox Executable Name", appState.settingsBuffer, sizeof(appState.settingsBuffer));
    ImGui::SameLine();
    if (ImGui::Button("Refresh Process")) {
        if (auto backend = smu::platform::GetProcessBackend()) {
            auto pids = backend->findAllProcesses(appState.settingsBuffer);
            context.detectedProcessCount = pids.size();
            appState.processFound = !pids.empty();
        }
    }
    ImGui::SameLine();
    ImGui::TextColored(appState.processFound ? ImVec4(0.25f, 0.85f, 0.45f, 1.0f) : ImVec4(0.95f, 0.45f, 0.35f, 1.0f),
        "%s", appState.processFound ? "Process Found" : "Process Not Found");

    ImGui::SetNextItemWidth(80.0f);
    if (ImGui::InputText("Roblox FPS", appState.robloxFpsBuffer, sizeof(appState.robloxFpsBuffer), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
        try {
            appState.robloxFps.store(static_cast<unsigned int>(std::stoul(appState.robloxFpsBuffer)), std::memory_order_release);
        } catch (...) {
        }
    }
    ImGui::SameLine();
    ImGui::SetNextItemWidth(90.0f);
    ImGui::InputText("Sensitivity", appState.robloxSensitivityBuffer, sizeof(appState.robloxSensitivityBuffer));
}

void RenderSectionList()
{
    auto& macroState = smu::core::GetMacroState();

    for (std::size_t displayIndex = 0; displayIndex < macroState.sectionOrder.size(); ++displayIndex) {
        const int sectionIndex = macroState.sectionOrder[displayIndex];
        auto& section = macroState.sections[sectionIndex];
        ImGui::PushID(sectionIndex);

        const bool selected = macroState.selectedSection == sectionIndex;
        const ImVec4 active = section.enabled ? ImVec4(0.20f, 0.47f, 0.79f, 1.0f) : ImVec4(0.30f, 0.33f, 0.38f, 1.0f);
        const ImVec4 selectedColor = section.enabled ? ImVec4(0.30f, 0.62f, 0.94f, 1.0f) : ImVec4(0.42f, 0.45f, 0.50f, 1.0f);

        ImGui::PushStyleColor(ImGuiCol_Button, selected ? selectedColor : active);
        ImGui::PushStyleColor(ImGuiCol_ButtonHovered, selectedColor);
        ImGui::PushStyleColor(ImGuiCol_ButtonActive, selectedColor);
        if (ImGui::Button(section.title.c_str(), ImVec2(-1.0f, 0.0f))) {
            macroState.selectedSection = sectionIndex;
        }
        ImGui::PopStyleColor(3);

        if (ImGui::BeginDragDropSource(ImGuiDragDropFlags_None)) {
            const int payloadIndex = static_cast<int>(displayIndex);
            ImGui::SetDragDropPayload("SMU_SECTION_ORDER", &payloadIndex, sizeof(payloadIndex));
            ImGui::TextUnformatted(section.title.c_str());
            ImGui::EndDragDropSource();
        }
        if (ImGui::BeginDragDropTarget()) {
            if (const ImGuiPayload* payload = ImGui::AcceptDragDropPayload("SMU_SECTION_ORDER")) {
                const int payloadIndex = *static_cast<const int*>(payload->Data);
                if (payloadIndex >= 0 && payloadIndex < static_cast<int>(macroState.sectionOrder.size())) {
                    std::swap(macroState.sectionOrder[payloadIndex], macroState.sectionOrder[displayIndex]);
                }
            }
            ImGui::EndDragDropTarget();
        }

        ImGui::PopID();
    }
}

void RenderKeyBinding(const char* id, smu::core::KeyCode& key, BindingState& state)
{
    ImGui::PushID(id);
    if (ImGui::Button(state.buttonText.c_str())) {
        state.bindingMode = true;
        state.buttonText = "Press a Key...";
    }
    ImGui::SameLine();
    key = BindKeyMode(state, key);
    ImGui::SetNextItemWidth(170.0f);
    ImGui::InputText("##KeyHuman", state.keyBufferHuman, sizeof(state.keyBufferHuman), ImGuiInputTextFlags_ReadOnly);
    ImGui::SameLine();
    ImGui::SetNextItemWidth(120.0f);
    ImGui::InputText("##KeyHex", state.keyBuffer, sizeof(state.keyBuffer), ImGuiInputTextFlags_ReadOnly);
    ImGui::PopID();
}

void RenderSelectedSection(AppContext& context)
{
    auto& macroState = smu::core::GetMacroState();

    if (macroState.selectedSection < 0 || macroState.selectedSection >= smu::core::kMacroSectionCount) {
        ImGui::TextUnformatted("Select a macro section.");
        return;
    }

    auto& section = macroState.sections[macroState.selectedSection];
    ImGui::TextUnformatted(section.title.c_str());
    ImGui::Separator();

    ImGui::TextUnformatted("Keybind:");
    ImGui::SameLine();
    RenderKeyBinding("SectionKey", section.keybind, g_sectionBindingStates[macroState.selectedSection]);

    ImGui::Checkbox("Enable This Macro", &section.enabled);
    if (macroState.selectedSection != 15) {
        ImGui::SameLine();
        std::string id = "##DisableOutside" + std::to_string(macroState.selectedSection);
        RenderForegroundDependentCheckbox(context, "Disable outside of Roblox:", id.c_str(), &section.disableOutsideRoblox);
    }

    ImGui::Spacing();
    ImGui::TextWrapped("%s", section.description.c_str());
    ImGui::Separator();

    switch (macroState.selectedSection) {
    case 0:
        ImGui::InputFloat("Auto-unfreeze seconds", &macroState.maxFreezeTime, 0.0f, 0.0f, "%.2f");
        ImGui::SliderFloat("##FreezeSlider", &macroState.maxFreezeTime, 0.0f, 9.8f, "%.2f Seconds");
        ImGui::InputInt("Default unfreeze time (ms)", &macroState.maxFreezeOverrideMs);
        ImGui::Checkbox("Switch from Hold Key to Toggle Key", &macroState.freezeToggleMode);
        ImGui::Checkbox("Freeze all Found Processes Instead of Newest", &macroState.takeAllProcessIds);
        break;
    case 3:
        ImGui::InputInt("Pixel Value for 180 Degree Turn", &macroState.speedStrengthX);
        macroState.speedStrengthY = -macroState.speedStrengthX;
        ImGui::Checkbox("Switch from Toggle Key to Hold Key", &macroState.speedHoldMode);
        break;
    case 5:
        ImGui::TextUnformatted("Key to Press:");
        ImGui::SameLine();
        RenderKeyBinding("PressKeyOutput", macroState.pressKeyOutput, g_pressKeyOutputBindingState);
        ImGui::InputInt("Length of Second Button Press (ms)", &macroState.pressKeyDelayMs);
        ImGui::InputInt("Delay Before Second Press (ms)", &macroState.pressKeyBonusDelayMs);
        break;
    case 6:
        ImGui::InputInt("Horizontal flick pixels", &macroState.wallhopDx);
        macroState.wallhopDy = -macroState.wallhopDx;
        ImGui::InputInt("Vertical flick pixels", &macroState.wallhopVertical);
        ImGui::InputInt("Wallhop delay (ms)", &macroState.wallhopDelayMs);
        ImGui::Checkbox("Jump during wallhop", &macroState.wallhopToggleJump);
        ImGui::Checkbox("Flick camera during wallhop", &macroState.wallhopToggleFlick);
        break;
    case 11:
        ImGui::TextUnformatted("Key to Spam:");
        ImGui::SameLine();
        RenderKeyBinding("SpamOutput", macroState.spamOutput, g_spamOutputBindingState);
        ImGui::InputInt("Spam delay (ms)", &macroState.spamDelayMs);
        break;
    case 15: {
        auto backend = smu::platform::GetNetworkLagBackend();
        const bool available = backend && backend->isAvailable();
        ImGui::BeginDisabled(!available);
        ImGui::Checkbox("Enable lag switch", &macroState.lagSwitch.enabled);
        ImGui::Checkbox("Inbound hard block", &macroState.lagSwitch.inboundHardBlock);
        ImGui::Checkbox("Outbound hard block", &macroState.lagSwitch.outboundHardBlock);
        ImGui::Checkbox("Fake lag", &macroState.lagSwitch.fakeLagEnabled);
        ImGui::InputInt("Fake lag delay (ms)", &macroState.lagSwitch.fakeLagDelayMs);
        ImGui::EndDisabled();
        if (!available) {
            ImGui::TextWrapped("%s", backend ? backend->unsupportedReason().c_str() : "Network lag switch backend is unavailable.");
        }
        break;
    }
    default:
        ImGui::TextUnformatted("This section's detailed controls are still owned by the Windows macro framework and are being split incrementally.");
        break;
    }
}

} // namespace

void RenderPlatformCriticalNotifications()
{
    static std::vector<smu::log::LogEntry> activeNotifications;

    auto newNotifications = smu::log::DrainCriticalNotifications();
    activeNotifications.insert(activeNotifications.end(), newNotifications.begin(), newNotifications.end());

    if (activeNotifications.empty()) {
        return;
    }

    ImGui::OpenPopup("Critical Platform Error");
    ImVec2 center = ImGui::GetMainViewport()->GetCenter();
    ImGui::SetNextWindowPos(center, ImGuiCond_Appearing, ImVec2(0.5f, 0.5f));
    ImGui::SetNextWindowSize(ImVec2(560.0f, 0.0f), ImGuiCond_Appearing);

    if (ImGui::BeginPopupModal("Critical Platform Error", nullptr, ImGuiWindowFlags_AlwaysAutoResize)) {
        ImGui::TextWrapped("A required platform feature failed to initialize.");
        ImGui::Separator();
        ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + 520.0f);
        ImGui::TextUnformatted(activeNotifications.front().message.c_str());
        ImGui::PopTextWrapPos();
        ImGui::Separator();

        const bool hasMore = activeNotifications.size() > 1;
        if (ImGui::Button(hasMore ? "Next" : "Dismiss", ImVec2(120, 0))) {
            activeNotifications.erase(activeNotifications.begin());
            if (activeNotifications.empty()) {
                ImGui::CloseCurrentPopup();
            }
        }

        ImGui::EndPopup();
    }
}

void RenderPlatformWarningNotifications()
{
    static std::vector<smu::log::LogEntry> activeWarnings;

    auto newWarnings = smu::log::DrainWarningNotifications();
    activeWarnings.insert(activeWarnings.end(), newWarnings.begin(), newWarnings.end());

    if (activeWarnings.empty()) {
        return;
    }

    const ImGuiViewport* viewport = ImGui::GetMainViewport();
    ImVec2 pos = ImVec2(viewport->WorkPos.x + viewport->WorkSize.x - 24.0f, viewport->WorkPos.y + 24.0f);
    ImGui::SetNextWindowPos(pos, ImGuiCond_Always, ImVec2(1.0f, 0.0f));
    ImGui::SetNextWindowSize(ImVec2(420.0f, 0.0f), ImGuiCond_Always);
    ImGui::SetNextWindowBgAlpha(0.96f);

    ImGuiWindowFlags flags = ImGuiWindowFlags_NoDecoration |
        ImGuiWindowFlags_NoSavedSettings |
        ImGuiWindowFlags_AlwaysAutoResize |
        ImGuiWindowFlags_NoFocusOnAppearing |
        ImGuiWindowFlags_NoNav;

    if (ImGui::Begin("Platform Warnings", nullptr, flags)) {
        ImGui::TextUnformatted("Warning");
        ImGui::Separator();
        ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + 380.0f);
        ImGui::TextUnformatted(activeWarnings.front().message.c_str());
        ImGui::PopTextWrapPos();

        if (ImGui::Button("Dismiss", ImVec2(100.0f, 0.0f))) {
            activeWarnings.erase(activeWarnings.begin());
        }
        if (activeWarnings.size() > 1) {
            ImGui::SameLine();
            ImGui::Text("%zu more", activeWarnings.size() - 1);
        }
    }
    ImGui::End();
}

void RenderForegroundDependentCheckbox(AppContext& context, const char* label, const char* id, bool* value)
{
    if (!value) {
        return;
    }

    const bool fallbackActive = IsForegroundDetectionFallbackActive(context);
    bool forcedValue = false;
    bool* checkboxValue = fallbackActive ? &forcedValue : value;

    ImGui::BeginDisabled(fallbackActive);
    ImGui::TextWrapped("%s", label);
    ImGui::SameLine();
    ImGui::Checkbox(id, checkboxValue);
    ImGui::EndDisabled();

    if (fallbackActive && ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled)) {
        ImGui::SetTooltip("%s", kForegroundFallbackTooltip);
    }
}

void RenderAppUi(AppContext& context)
{
    MaybeWarnForegroundDetectionFallback(context);

    auto& appState = smu::core::GetAppState();
    auto& macroState = smu::core::GetMacroState();

    if (macroState.sections[0].title.empty()) {
        smu::core::InitializeMacroSections(appState.shortDescriptions);
    }

    const ImVec2 displaySize = ImGui::GetIO().DisplaySize;
    ImGui::SetNextWindowSize(displaySize, ImGuiCond_Always);
    ImGui::SetNextWindowPos(ImVec2(0, 0), ImGuiCond_Always);

    ImGuiWindowFlags windowFlags = ImGuiWindowFlags_NoResize |
        ImGuiWindowFlags_NoMove |
        ImGuiWindowFlags_NoTitleBar |
        ImGuiWindowFlags_NoScrollbar |
        ImGuiWindowFlags_NoScrollWithMouse |
        ImGuiWindowFlags_NoBringToFrontOnFocus;

    ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, 0.0f);
    ImGui::Begin("Main SMU Window", nullptr, windowFlags);
    ImGui::PopStyleVar();

    RenderPlatformCriticalNotifications();
    RenderPlatformWarningNotifications();

    ImGui::BeginChild("GlobalSettings", ImVec2(displaySize.x - 16.0f, 150.0f), true);
    RenderGlobalSettings(context);
    ImGui::EndChild();

    const float leftPanelWidth = std::max(260.0f, displaySize.x * 0.30f - 24.0f);
    const float contentHeight = displaySize.y - 180.0f;

    ImGui::BeginChild("LeftScrollSection", ImVec2(leftPanelWidth, contentHeight), true);
    RenderSectionList();
    ImGui::EndChild();

    ImGui::SameLine();

    ImGui::BeginGroup();
    ImGui::BeginChild("RightSection", ImVec2(0.0f, contentHeight - 96.0f), true);
    RenderSelectedSection(context);
    ImGui::EndChild();

    ImGui::BeginChild("BottomControls", ImVec2(0.0f, 88.0f), true);
    RenderCapabilities(context);
    ImGui::EndChild();
    ImGui::EndGroup();

    ImGui::End();
}

} // namespace smu::app
