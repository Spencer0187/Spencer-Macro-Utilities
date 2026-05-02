#include "app_ui.h"

#include "app_profile_bridge.h"
#include "app_theme_bridge.h"
#include "input_actions.h"
#include "../core/key_codes.h"
#include "../platform/input_backend.h"
#include "../platform/logging.h"
#include "../platform/network_backend.h"
#include "../platform/process_backend.h"
#include "../platform/updater/updater.h"

#ifndef SMU_PORTABLE_GLOBALS
#define SMU_PORTABLE_GLOBALS
#endif
#include "Resource Files/globals.h"

#include "imgui.h"
#include <SDL3/SDL_clipboard.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <optional>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>

namespace smu::app {
namespace {

using namespace Globals;

const char* optionsforoffset[] = {"/e dance2", "/e laugh", "/e cheer"};

const char* kForegroundFallbackTooltip =
    "Foreground Roblox window detection is unavailable on this display server. This usually happens on Wayland because apps cannot reliably inspect other apps' active windows. This macro has been forced to always-active mode.";

struct Section {
    std::string title;
    std::string description;
    bool optionA = false;
    float settingValue = 50.0f;
};

struct SectionConfig {
    const char* title;
    const char* description;
};

std::vector<Section> sections;

std::array<SectionConfig, section_amounts> SECTION_CONFIGS = {{
    {"Freeze", "Automatically Tab Glitch With a Button"},
    {"Item Desync", "Enable Item Collision (Hold Item Before Pressing)"},
    {"Wall Helicopter High Jump", "Use COM Offset to Catapult Yourself Into The Air by Aligning your Back Angled to the Wall and Jumping and Letting Your Character Turn"},
    {"Speedglitch", "Use COM offset to Massively Increase Midair Speed"},
    {"Item Unequip COM Offset", "Automatically Do a /e dance2 Item COM Offset Where You Unequip the Item"},
    {"Press a Button", "Whenever You Press Your Keybind, it Presses the Other Button for One Frame"},
    {"Wallhop/Rotation", "Automatically Flick and Jump to easily Wallhop On All FPS"},
    {"Walless LHJ", "Lag High Jump Without a Wall by Offsetting COM Downwards or to the Right"},
    {"Item Clip", "Clip through 2-3 Stud Walls Using Gears"},
    {"Laugh Clip", "Automatically Perform a Laugh Clip"},
    {"Wall-Walk", "Walk Across Wall Seams Without Jumping"},
    {"Spam a Key", "Whenever You Press Your Keybind, it Spams the Other Button"},
    {"Ledge Bounce", "Briefly Falls off a Ledge to Then Bounce Off it While Falling"},
    {"Smart Bunnyhop", "Intelligently enables or disables Bunnyhop for any Key"},
    {"Floor Bounce", "Jump much higher from flat ground"},
    {"Lag Switch", "Modify or disable your internet connection temporarily"}
}};

struct BindingState {
    bool bindingMode = false;
    bool notBinding = true;
    std::chrono::steady_clock::time_point rebindTime{};
    char keyBuffer[64] = "0x00";
    char keyBufferHuman[64] = "None";
    std::string buttonText = "Click to Bind Key";
    bool firstRun = true;
    int lastSelectedSection = -1;
};

std::unordered_map<unsigned int*, BindingState> g_bindingStates;

struct InstanceRemoveConfirmState {
    int stage = 0;
    float timer = 0.0f;
    int section_index = -1;
    int target_instance_index = -1;
};

InstanceRemoveConfirmState g_instance_remove_confirm_state{};

struct UpdateUiState {
    std::mutex mutex;
    smu::updater::UpdaterStatus status;
    bool checkedOnce = false;
    bool checking = false;
    bool applying = false;
    std::string actionMessage;
};

UpdateUiState g_updateUiState;

void InitializeSections()
{
    sections.clear();
    for (const auto& config : SECTION_CONFIGS) {
        sections.push_back({config.title, shortdescriptions ? "" : config.description, false, 50.0f});
    }
}

void ResetInstanceRemoveConfirmState()
{
    g_instance_remove_confirm_state = {};
}

void StartUpdateCheck(bool force)
{
    {
        std::lock_guard<std::mutex> lock(g_updateUiState.mutex);
        if (g_updateUiState.checking || g_updateUiState.applying || (g_updateUiState.checkedOnce && !force)) {
            return;
        }
        g_updateUiState.checking = true;
        g_updateUiState.actionMessage = "Checking for updates...";
    }

    const std::string version = localVersion;
    std::thread([version]() {
        smu::updater::UpdaterStatus status = smu::updater::CheckForUpdate(version);
        if (!status.checkSucceeded) {
            LogWarning("Update check failed: " + status.message);
        } else if (status.updateAvailable) {
            LogInfo("Update available: " + status.localVersion + " -> " + status.latestVersion);
        }
        {
            std::lock_guard<std::mutex> lock(g_updateUiState.mutex);
            g_updateUiState.status = std::move(status);
            g_updateUiState.checkedOnce = true;
            g_updateUiState.checking = false;
            g_updateUiState.actionMessage = g_updateUiState.status.message;
        }
    }).detach();
}

void StartApplyUpdate()
{
    smu::updater::ReleaseInfo release;
    {
        std::lock_guard<std::mutex> lock(g_updateUiState.mutex);
        if (g_updateUiState.applying || !g_updateUiState.status.latestRelease) {
            return;
        }
        release = *g_updateUiState.status.latestRelease;
        g_updateUiState.applying = true;
        g_updateUiState.actionMessage = "Downloading update...";
    }

    const std::string version = localVersion;
    std::thread([release = std::move(release), version]() {
        std::string error;
        const bool ok = smu::updater::ApplyUpdate(release, version, &error);
        {
            std::lock_guard<std::mutex> lock(g_updateUiState.mutex);
            g_updateUiState.applying = false;
            g_updateUiState.actionMessage = ok
                ? "Update installer launched. The app will restart shortly."
                : (error.empty() ? "Update failed." : error);
        }
        if (!ok) {
            LogWarning(error.empty() ? "Update failed." : error);
        }
    }).detach();
}

ImVec4 Brighten(ImVec4 col, float factor)
{
    return ImVec4(std::min(col.x * factor, 1.0f),
        std::min(col.y * factor, 1.0f),
        std::min(col.z * factor, 1.0f),
        col.w);
}

void CopyString(char* destination, std::size_t size, const std::string& source)
{
    if (!destination || size == 0) {
        return;
    }
    std::snprintf(destination, size, "%s", source.c_str());
}

std::string BuildMultiInstanceName(int sectionIndex, int oneBasedIndex)
{
    if (sectionIndex >= 0 && sectionIndex < static_cast<int>(sections.size())) {
        return sections[sectionIndex].title + " " + std::to_string(oneBasedIndex);
    }
    return "Instance " + std::to_string(oneBasedIndex);
}

int GetInstanceRemovalTargetIndex(size_t instanceCount, int selectedIndex)
{
    if (instanceCount <= 1) {
        return -1;
    }
    const int lastIndex = static_cast<int>(instanceCount) - 1;
    if (selectedIndex > 0 && selectedIndex <= lastIndex) {
        return selectedIndex;
    }
    return lastIndex;
}

int GetCurrentRemovalTargetForSection(int sectionIndex)
{
    if (sectionIndex == 5) {
        return GetInstanceRemovalTargetIndex(presskey_instances.size(), selected_presskey_instance);
    }
    if (sectionIndex == 6) {
        return GetInstanceRemovalTargetIndex(wallhop_instances.size(), selected_wallhop_instance);
    }
    if (sectionIndex == 11) {
        return GetInstanceRemovalTargetIndex(spamkey_instances.size(), selected_spamkey_instance);
    }
    return -1;
}

bool* GetDisableOutsideTogglePtr(int sectionIndex)
{
    if (sectionIndex < 0 || sectionIndex >= section_amounts || sectionIndex == 15) {
        return nullptr;
    }
    if (sectionIndex == 5) {
        if (presskey_instances.empty() || selected_presskey_instance < 0 || selected_presskey_instance >= static_cast<int>(presskey_instances.size())) {
            return nullptr;
        }
        return &presskey_instances[selected_presskey_instance].presskeyinroblox;
    }
    if (sectionIndex == 6) {
        if (wallhop_instances.empty() || selected_wallhop_instance < 0 || selected_wallhop_instance >= static_cast<int>(wallhop_instances.size())) {
            return nullptr;
        }
        return &wallhop_instances[selected_wallhop_instance].disable_outside_roblox;
    }
    if (sectionIndex == 11) {
        if (spamkey_instances.empty() || selected_spamkey_instance < 0 || selected_spamkey_instance >= static_cast<int>(spamkey_instances.size())) {
            return nullptr;
        }
        return &spamkey_instances[selected_spamkey_instance].disable_outside_roblox;
    }
    return &disable_outside_roblox[sectionIndex];
}

void CopyWallhopInstanceData(const WallhopInstance& src, WallhopInstance& dst)
{
    dst.wallhop_dx = src.wallhop_dx;
    dst.wallhop_dy = src.wallhop_dy;
    dst.wallhop_vertical = src.wallhop_vertical;
    dst.WallhopDelay = src.WallhopDelay;
    dst.WallhopBonusDelay = src.WallhopBonusDelay;
    strncpy_s(dst.WallhopPixels, sizeof(dst.WallhopPixels), src.WallhopPixels, _TRUNCATE);
    strncpy_s(dst.WallhopVerticalChar, sizeof(dst.WallhopVerticalChar), src.WallhopVerticalChar, _TRUNCATE);
    strncpy_s(dst.WallhopDelayChar, sizeof(dst.WallhopDelayChar), src.WallhopDelayChar, _TRUNCATE);
    strncpy_s(dst.WallhopBonusDelayChar, sizeof(dst.WallhopBonusDelayChar), src.WallhopBonusDelayChar, _TRUNCATE);
    strncpy_s(dst.WallhopDegrees, sizeof(dst.WallhopDegrees), src.WallhopDegrees, _TRUNCATE);
    dst.wallhopswitch = src.wallhopswitch;
    dst.toggle_jump = src.toggle_jump;
    dst.toggle_flick = src.toggle_flick;
    dst.wallhopcamfix = src.wallhopcamfix;
    dst.disable_outside_roblox = src.disable_outside_roblox;
    dst.section_enabled = src.section_enabled;
    dst.vk_trigger = src.vk_trigger;
    dst.vk_jumpkey = src.vk_jumpkey;
    dst.should_exit = false;
    dst.isRunning = src.isRunning;
    dst.thread_active.store(src.thread_active.load(std::memory_order_relaxed), std::memory_order_relaxed);
}

void CopyPresskeyInstanceData(const PresskeyInstance& src, PresskeyInstance& dst)
{
    dst.vk_trigger = src.vk_trigger;
    dst.vk_presskey = src.vk_presskey;
    dst.PressKeyDelay = src.PressKeyDelay;
    dst.PressKeyBonusDelay = src.PressKeyBonusDelay;
    strncpy_s(dst.PressKeyDelayChar, sizeof(dst.PressKeyDelayChar), src.PressKeyDelayChar, _TRUNCATE);
    strncpy_s(dst.PressKeyBonusDelayChar, sizeof(dst.PressKeyBonusDelayChar), src.PressKeyBonusDelayChar, _TRUNCATE);
    dst.presskeyinroblox = src.presskeyinroblox;
    dst.section_enabled = src.section_enabled;
    dst.should_exit = false;
    dst.isRunning = src.isRunning;
    dst.thread_active.store(src.thread_active.load(std::memory_order_relaxed), std::memory_order_relaxed);
}

void CopySpamkeyInstanceData(const SpamkeyInstance& src, SpamkeyInstance& dst)
{
    dst.vk_trigger = src.vk_trigger;
    dst.vk_spamkey = src.vk_spamkey;
    dst.spam_delay = src.spam_delay;
    dst.real_delay = src.real_delay;
    strncpy_s(dst.SpamDelay, sizeof(dst.SpamDelay), src.SpamDelay, _TRUNCATE);
    dst.isspamswitch = src.isspamswitch;
    dst.disable_outside_roblox = src.disable_outside_roblox;
    dst.section_enabled = src.section_enabled;
    dst.should_exit = false;
    dst.isRunning = src.isRunning;
    dst.thread_active.store(src.thread_active.load(std::memory_order_relaxed), std::memory_order_relaxed);
}

void FormatHexKeyString(unsigned int combinedCode, char* buffer, size_t size)
{
    unsigned int vk = combinedCode & HOTKEY_KEY_MASK;
    std::string hexStr;
    if (combinedCode & HOTKEY_MASK_WIN) hexStr += "0x5B + ";
    if (combinedCode & HOTKEY_MASK_CTRL) hexStr += "0x11 + ";
    if (combinedCode & HOTKEY_MASK_ALT) hexStr += "0x12 + ";
    if (combinedCode & HOTKEY_MASK_SHIFT) hexStr += "0x10 + ";

    char vkHex[16] = {};
    std::snprintf(vkHex, sizeof(vkHex), "0x%X", vk);
    hexStr += vkHex;
    CopyString(buffer, size, hexStr);
}

void GetKeyNameFromHex(unsigned int combinedKeyCode, char* buffer, size_t bufferSize)
{
    for (auto& [keyPtr, state] : g_bindingStates) {
        if (state.bindingMode && *keyPtr == combinedKeyCode) {
            return;
        }
    }

    unsigned int vk = combinedKeyCode & HOTKEY_KEY_MASK;
    std::string prefix;
    if (combinedKeyCode & HOTKEY_MASK_WIN) prefix += "Win + ";
    if (combinedKeyCode & HOTKEY_MASK_CTRL) prefix += "Ctrl + ";
    if (combinedKeyCode & HOTKEY_MASK_ALT) prefix += "Alt + ";
    if (combinedKeyCode & HOTKEY_MASK_SHIFT) prefix += "Shift + ";

    std::string keyName;
    if (auto backend = smu::platform::GetInputBackend()) {
        keyName = backend->formatKeyName(vk);
    }
    if (keyName.empty()) {
        const auto it = vkToString.find(vk);
        keyName = it != vkToString.end() ? it->second : smu::core::FormatKeyCodeFallback(vk);
    }

    CopyString(buffer, bufferSize, prefix + keyName);
}

unsigned int CurrentModifierMask()
{
    unsigned int currentModifiers = 0;
    if (IsKeyPressed(VK_LWIN) || IsKeyPressed(VK_RWIN)) currentModifiers |= HOTKEY_MASK_WIN;
    if (IsKeyPressed(VK_CONTROL) || IsKeyPressed(VK_LCONTROL) || IsKeyPressed(VK_RCONTROL)) currentModifiers |= HOTKEY_MASK_CTRL;
    if (IsKeyPressed(VK_MENU) || IsKeyPressed(VK_LMENU) || IsKeyPressed(VK_RMENU)) currentModifiers |= HOTKEY_MASK_ALT;
    if (IsKeyPressed(VK_SHIFT) || IsKeyPressed(VK_LSHIFT) || IsKeyPressed(VK_RSHIFT)) currentModifiers |= HOTKEY_MASK_SHIFT;
    return currentModifiers;
}

unsigned int BindKeyMode(unsigned int* keyVar, unsigned int currentkey, int currentSection)
{
    BindingState& state = g_bindingStates[keyVar];

    if (state.bindingMode) {
        notbinding = false;
        state.rebindTime = std::chrono::steady_clock::now();
        state.buttonText = "Press a Key...";

        const unsigned int currentModifiers = CurrentModifierMask();
        if (currentModifiers == 0) {
            CopyString(state.keyBuffer, sizeof(state.keyBuffer), "Waiting...");
            CopyString(state.keyBufferHuman, sizeof(state.keyBufferHuman), "Waiting...");
        } else {
            char previewHex[64] = {};
            FormatHexKeyString(currentModifiers, previewHex, sizeof(previewHex));
            std::string hexStr(previewHex);
            const std::string suffix = " + 0x0";
            if (const auto pos = hexStr.find(suffix); pos != std::string::npos) {
                hexStr = hexStr.substr(0, pos) + " + ...";
            }
            std::string previewHuman;
            if (currentModifiers & HOTKEY_MASK_WIN) previewHuman += "Win + ";
            if (currentModifiers & HOTKEY_MASK_CTRL) previewHuman += "Ctrl + ";
            if (currentModifiers & HOTKEY_MASK_ALT) previewHuman += "Alt + ";
            if (currentModifiers & HOTKEY_MASK_SHIFT) previewHuman += "Shift + ";
            previewHuman += "...";
            CopyString(state.keyBuffer, sizeof(state.keyBuffer), hexStr);
            CopyString(state.keyBufferHuman, sizeof(state.keyBufferHuman), previewHuman);
        }

        auto backend = smu::platform::GetInputBackend();
        if (!backend) {
            return currentkey;
        }

        const auto pressedKey = backend->getCurrentPressedKey();
        if (!pressedKey || smu::core::IsModifierKey(*pressedKey)) {
            return currentkey;
        }

        const unsigned int finalCombo = (*pressedKey & HOTKEY_KEY_MASK) | currentModifiers;
        state.bindingMode = false;
        state.firstRun = true;
        GetKeyNameFromHex(finalCombo, state.keyBufferHuman, sizeof(state.keyBufferHuman));
        FormatHexKeyString(finalCombo, state.keyBuffer, sizeof(state.keyBuffer));
        state.buttonText = "Click to Bind Key";
        notbinding = true;
        return finalCombo;
    }

    state.firstRun = true;
    if (currentSection != state.lastSelectedSection || currentSection == -1) {
        FormatHexKeyString(currentkey, state.keyBuffer, sizeof(state.keyBuffer));
        GetKeyNameFromHex(currentkey, state.keyBufferHuman, sizeof(state.keyBufferHuman));
        if (currentSection != -1) {
            state.lastSelectedSection = currentSection;
        }
    }

    state.buttonText = "Click to Bind Key";
    auto currentTime = std::chrono::steady_clock::now();
    std::chrono::duration<double> elapsedtime = currentTime - state.rebindTime;
    if (elapsedtime.count() >= 0.3) {
        state.notBinding = true;
        notbinding = true;
    }
    return currentkey;
}

void DrawKeyBindControl(const char* id, unsigned int& key, int currentSection, float humanWidth = 170.0f, float hexWidth = 130.0f)
{
    ImGui::PushID(id);
    BindingState& state = g_bindingStates[&key];
    if (ImGui::Button(state.buttonText.c_str())) {
        state.bindingMode = true;
        state.notBinding = false;
        state.buttonText = "Press a Key...";
    }
    ImGui::SameLine();
    key = BindKeyMode(&key, key, currentSection);
    ImGui::SetNextItemWidth(humanWidth);
    GetKeyNameFromHex(key, state.keyBufferHuman, sizeof(state.keyBufferHuman));
    ImGui::InputText("##KeyHuman", state.keyBufferHuman, sizeof(state.keyBufferHuman), ImGuiInputTextFlags_ReadOnly);
    ImGui::SameLine();
    ImGui::TextWrapped("Key Binding");
    ImGui::SameLine();
    ImGui::SetNextItemWidth(hexWidth);
    ImGui::InputText("##KeyHex", state.keyBuffer, sizeof(state.keyBuffer), ImGuiInputTextFlags_CharsNoBlank | ImGuiInputTextFlags_CharsHexadecimal | ImGuiInputTextFlags_ReadOnly);
    ImGui::SameLine();
    ImGui::TextWrapped("(Hexadecimal)");
    ImGui::PopID();
}

smu::platform::LagSwitchConfig BuildLagSwitchConfigFromUiState()
{
    smu::platform::LagSwitchConfig config;
    config.enabled = bWinDivertEnabled;
    config.currentlyBlocking = g_windivert_blocking.load(std::memory_order_relaxed);
    config.inboundHardBlock = lagswitchinbound;
    config.outboundHardBlock = lagswitchoutbound;
    config.fakeLagEnabled = lagswitchlag;
    config.inboundFakeLag = lagswitchlaginbound;
    config.outboundFakeLag = lagswitchlagoutbound;
    config.fakeLagDelayMs = lagswitchlagdelay;
    config.targetRobloxOnly = lagswitchtargetroblox;
    config.useUdp = true;
    config.useTcp = lagswitchusetcp;
    config.preventDisconnect = prevent_disconnect;
    config.autoUnblock = lagswitch_autounblock;
    config.maxDurationSeconds = lagswitch_max_duration;
    config.unblockDurationMs = lagswitch_unblock_ms;
    return config;
}

void SyncLagSwitchBackendConfig()
{
    if (auto backend = smu::platform::GetNetworkLagBackend()) {
        backend->setConfig(BuildLagSwitchConfigFromUiState());
        g_windivert_blocking.store(backend->isBlockingActive(), std::memory_order_relaxed);
    }
}

bool InitializeLagSwitchBackend(std::string* errorMessage = nullptr)
{
    SyncLagSwitchBackendConfig();
    if (auto backend = smu::platform::GetNetworkLagBackend()) {
        const bool ok = backend->init(errorMessage);
        bWinDivertEnabled = ok;
        return ok;
    }
    if (errorMessage) {
        *errorMessage = "Network lagswitch backend is unavailable.";
    }
    bWinDivertEnabled = false;
    return false;
}

void ShutdownLagSwitchBackend()
{
    if (auto backend = smu::platform::GetNetworkLagBackend()) {
        backend->shutdown();
    }
    bWinDivertEnabled = false;
    g_windivert_blocking.store(false, std::memory_order_relaxed);
}

std::string FileUrlFromPath(const std::string& path)
{
    if (path.empty()) {
        return {};
    }

    std::string url = "file://";
    for (char ch : path) {
        if (ch == ' ') {
            url += "%20";
        } else {
            url += ch;
        }
    }
    return url;
}

void RenderLinuxInputSetup(AppContext& context)
{
#if defined(__linux__)
    if (!context.linuxInputSetupRequired) {
        return;
    }

    ImGui::OpenPopup("Linux Input Setup Required");
    ImVec2 center = ImGui::GetMainViewport()->GetCenter();
    ImGui::SetNextWindowPos(center, ImGuiCond_Appearing, ImVec2(0.5f, 0.5f));
    ImGui::SetNextWindowSize(ImVec2(620.0f, 0.0f), ImGuiCond_Appearing);

    if (ImGui::BeginPopupModal("Linux Input Setup Required", nullptr, ImGuiWindowFlags_AlwaysAutoResize)) {
        ImGui::TextWrapped("Native Linux input requires access to /dev/input/event* and /dev/uinput.");
        ImGui::TextWrapped("Spencer Macro Utilities can install udev rules and add your user to an smu-input group. You may need to log out and back in, or reboot, before the new group membership reaches this desktop session.");
        ImGui::Separator();
        ImGui::TextWrapped("%s", context.linuxInputPermissionSummary.c_str());
        ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + 580.0f);
        ImGui::TextUnformatted(context.linuxInputPermissionDetails.c_str());
        ImGui::PopTextWrapPos();
        ImGui::Separator();

        if (ImGui::Button("Install permissions with pkexec", ImVec2(230.0f, 0.0f))) {
            if (context.installLinuxPermissionsWithPkexec) {
                context.installLinuxPermissionsWithPkexec();
            }
        }
        ImGui::SameLine();
        if (ImGui::Button("Copy sudo command", ImVec2(170.0f, 0.0f))) {
            if (!context.linuxInputSudoCommand.empty() && SDL_SetClipboardText(context.linuxInputSudoCommand.c_str())) {
                context.linuxInputSetupActionMessage = "Copied: " + context.linuxInputSudoCommand;
            } else {
                context.linuxInputSetupActionMessage = "Could not copy the sudo command to the clipboard.";
            }
        }
        ImGui::SameLine();
        if (ImGui::Button("Open Linux setup docs", ImVec2(190.0f, 0.0f))) {
            if (context.openExternalUrl) {
                const std::string docsUrl = FileUrlFromPath(context.linuxInputSetupDocsPath);
                if (!docsUrl.empty()) {
                    context.openExternalUrl(docsUrl.c_str());
                }
            }
        }

        if (ImGui::Button("Retry permission check", ImVec2(180.0f, 0.0f))) {
            if (context.refreshLinuxInputPermissions) {
                context.refreshLinuxInputPermissions();
            }
        }

        if (!context.linuxInputSetupActionMessage.empty()) {
            ImGui::Separator();
            ImGui::PushTextWrapPos(ImGui::GetCursorPosX() + 580.0f);
            ImGui::TextUnformatted(context.linuxInputSetupActionMessage.c_str());
            ImGui::PopTextWrapPos();
        }

        if (!context.linuxInputSetupRequired) {
            ImGui::CloseCurrentPopup();
        }

        ImGui::EndPopup();
    }
#else
    (void)context;
#endif
}

void RestartLagSwitchCapture()
{
    SyncLagSwitchBackendConfig();
    if (auto backend = smu::platform::GetNetworkLagBackend()) {
        backend->restartCapture();
    }
}

void RefreshProcessStatus(AppContext& context)
{
    static double nextRefreshTime = 0.0;
    const double now = ImGui::GetTime();
    if (now < nextRefreshTime) {
        return;
    }
    nextRefreshTime = now + 1.0;

    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        processFound = false;
        context.detectedProcessCount = 0;
        return;
    }

    const auto pids = backend->findAllProcesses(settingsBuffer);
    context.detectedProcessCount = pids.size();
    processFound = !pids.empty();
}

void RenderUpdaterPanel()
{
    StartUpdateCheck(false);

    smu::updater::UpdaterStatus status;
    bool checking = false;
    bool applying = false;
    bool checkedOnce = false;
    std::string actionMessage;
    {
        std::lock_guard<std::mutex> lock(g_updateUiState.mutex);
        status = g_updateUiState.status;
        checking = g_updateUiState.checking;
        applying = g_updateUiState.applying;
        checkedOnce = g_updateUiState.checkedOnce;
        actionMessage = g_updateUiState.actionMessage;
    }

    UserOutdated = checkedOnce && status.updateAvailable;

    if (!ImGui::CollapsingHeader("Updates", ImGuiTreeNodeFlags_DefaultOpen)) {
        return;
    }

    ImGui::Text("Local version: %s", localVersion.c_str());
    if (checkedOnce && !status.latestVersion.empty()) {
        ImGui::Text("Latest version: %s", status.latestVersion.c_str());
    } else {
        ImGui::TextUnformatted("Latest version: unknown");
    }

    if (checking) {
        ImGui::TextUnformatted("Status: checking...");
    } else if (checkedOnce) {
        ImGui::TextWrapped("Status: %s", status.message.c_str());
    } else {
        ImGui::TextUnformatted("Status: not checked yet");
    }

    if (status.selectedAsset) {
        ImGui::TextWrapped("Selected package: %s", status.selectedAsset->name.c_str());
    }

    if (ImGui::Button("Check for updates")) {
        StartUpdateCheck(true);
    }

    const bool canApply = checkedOnce &&
        status.updateAvailable &&
        status.autoApplySupported &&
        status.latestRelease.has_value() &&
        status.selectedAsset.has_value() &&
        !checking &&
        !applying;

    ImGui::SameLine();
    ImGui::BeginDisabled(!canApply);
    if (ImGui::Button(applying ? "Applying update..." : "Download and install update")) {
        StartApplyUpdate();
    }
    ImGui::EndDisabled();

    if (checkedOnce && status.updateAvailable && !status.autoApplySupported) {
        ImGui::TextColored(GetCurrentTheme().warning_color,
            "Update check available, auto-apply not implemented for this platform.");
    }

    if (!actionMessage.empty()) {
        ImGui::TextWrapped("%s", actionMessage.c_str());
    }

    ImGui::Separator();
}

void RenderSettingsMenu(AppContext& context, bool* open)
{
    if (!*open) {
        return;
    }

    ImVec2 mainWindowSize = ImGui::GetIO().DisplaySize;
    float childWidth = mainWindowSize.x * 0.5f;
    float childHeight = mainWindowSize.y * 0.5f;
    ImVec2 childPos((mainWindowSize.x * 0.4f), (mainWindowSize.y - childHeight - 90) * 0.5f);

    ImGui::SetNextWindowPos(childPos, ImGuiCond_Once);
    ImGui::SetNextWindowSize(ImVec2(childWidth, childHeight), ImGuiCond_Always);

    if (ImGui::Begin("Settings Menu", open, ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoCollapse)) {
        ImGui::BeginChild("SettingsList", ImVec2(0, 0), true);

        RenderUpdaterPanel();

        ImGui::TextUnformatted("Your Current Windows Display Scale Value (10-500%):");
        ImGui::SetNextItemWidth(150);
        if (ImGui::InputInt("##DisplayScale", &display_scale)) {
            display_scale = std::clamp(display_scale, 10, 500);
        }
        ImGui::SameLine();
        ImGui::Text("%%");
        ImGui::Separator();

        ImGui::TextWrapped("Custom Shiftlock Key:");
        DrawKeyBindControl("ShiftKey", vk_shiftkey, selected_section, 150.0f, 50.0f);
        ImGui::Separator();

        ImGui::Checkbox("Force-Set Chat Open Key to \"/\" (Most Stable)", &chatoverride);
        ImGui::Separator();

        ImGui::TextWrapped("Custom Chat Key (Must disable Force-Set):");
        DrawKeyBindControl("ChatKey", vk_chatkey, selected_section, 150.0f, 50.0f);
        ImGui::Separator();

        ImGui::Checkbox("##Oldpaste", &useoldpaste);
        ImGui::SameLine();
        ImGui::SetCursorPosX(ImGui::GetCursorPosX() - 4);
        ImGui::TextWrapped("Old Unicode Chat-Typing (Works across languages but may be blocked by some anticheats)");
        ImGui::Separator();

        ImGui::AlignTextToFramePadding();
        ImGui::TextWrapped("Delay between every key press in chat (ms):");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(30.0f);
        if (ImGui::InputText("##PasteDelay", PasteDelayChar, sizeof(PasteDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            try { PasteDelay = std::stoi(PasteDelayChar); } catch (...) {}
        }
        ImGui::Separator();

        ImGui::TextWrapped("Custom Anti-AFK Key (That the macro uses):");
        DrawKeyBindControl("AfkKey", vk_afkkey, selected_section, 150.0f, 50.0f);
        ImGui::Separator();

        ImGui::AlignTextToFramePadding();
        ImGui::Text("Amount of Minutes Between Anti-AFK Runs:");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(30.0f);
        if (ImGui::InputText("##AntiAFKTime", AntiAFKTimeChar, sizeof(AntiAFKTimeChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            try { AntiAFKTime = std::stoi(AntiAFKTimeChar); } catch (...) {}
        }
        ImGui::Separator();

        ImGui::Checkbox("Replace shiftlock with zooming in", &globalzoomin);
        ImGui::SameLine();
        ImGui::Checkbox("Reverse Direction?", &globalzoominreverse);
        ImGui::Separator();
        ImGui::Checkbox("Double-Press AFK keybind during Anti-AFK", &doublepressafkkey);
        ImGui::Separator();

#if defined(__linux__)
        if (ImGui::CollapsingHeader("Debug", ImGuiTreeNodeFlags_DefaultOpen)) {
            auto backend = smu::platform::GetInputBackend();
            const auto pressedKey = backend ? backend->getCurrentPressedKey() : std::optional<smu::platform::PlatformKeyCode>{};
            const std::string pressedName = pressedKey ? backend->formatKeyName(*pressedKey) : "None";
            ImGui::Text("Input backend: %s", context.inputBackendAvailable ? "initialized" : "unavailable");
            ImGui::Text("Process backend: %s", context.processBackendAvailable ? "initialized" : "unavailable");
            ImGui::Text("Last physical key: %s", pressedName.c_str());
            if (ImGui::Button("Test input injection: press Space")) {
                if (backend && context.inputBackendAvailable) {
                    backend->pressKey(smu::core::SMU_VK_SPACE);
                    LogInfo("Linux debug input injection test dispatched Space through uinput.");
                } else {
                    LogWarning("Linux debug input injection test failed: input backend is unavailable.");
                }
            }
        }
        ImGui::Separator();
#endif

        if (ImGui::Checkbox("Remove Side-Bar Macro Descriptions", &shortdescriptions)) {
            InitializeSections();
        }
        ImGui::Separator();

        ImGui::Dummy(ImVec2(0.0f, ImGui::GetTextLineHeight() * 0.5f));
        ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_primary);
        ImGui::Text("%s", "Want to Donate directly to my Github?");
        if (ImGui::IsItemHovered()) {
            ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
            if (ImGui::IsItemClicked() && context.openExternalUrl) {
                context.openExternalUrl("https://github.com/sponsors/Spencer0187");
            }
        }
        ImGui::PopStyleColor();
        ImGui::Text("%s", "Github Doesn't take any of the profits.");

        ImGui::EndChild();
    }
    ImGui::End();
}

void RenderGlobalSettings(AppContext& context, ImVec2 displaySize)
{
    StartUpdateCheck(false);
    {
        std::lock_guard<std::mutex> lock(g_updateUiState.mutex);
        if (g_updateUiState.checkedOnce) {
            UserOutdated = g_updateUiState.status.updateAvailable;
        }
    }
    RefreshProcessStatus(context);

    ImGui::AlignTextToFramePadding();
    ImGui::TextWrapped("Global Settings");
    if (UserOutdated) {
        ImGui::SameLine(135);
        ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().error_color);
        ImGui::TextWrapped("(OUTDATED VERSION)");
        ImGui::PopStyleColor();
    }

    ImGui::SameLine(ImGui::GetWindowWidth() - 795);
    ImGui::TextWrapped("DISCLAIMER: THIS IS NOT A CHEAT, IT NEVER INTERACTS WITH ROBLOX MEMORY.");

    ImGui::PushStyleColor(ImGuiCol_Text, macrotoggled ? GetCurrentTheme().success_color : GetCurrentTheme().error_color);
    ImGui::AlignTextToFramePadding();
    ImGui::Checkbox("Macro Toggle (Anti-AFK remains!)", &macrotoggled);
    ImGui::PopStyleColor();

    ImGui::SameLine(ImGui::GetWindowWidth() - 790);
    ImGui::TextWrapped("The ONLY official source for this is");
    ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_secondary);
    ImGui::SameLine(ImGui::GetWindowWidth() - 499);
    ImGui::TextWrapped("https://github.com/Spencer0187/Spencer-Macro-Utilities");
    if (ImGui::IsItemHovered()) {
        ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
        if (ImGui::IsItemClicked() && context.openExternalUrl) {
            context.openExternalUrl("https://github.com/Spencer0187/Spencer-Macro-Utilities");
        }
    }
    ImGui::PopStyleColor();

    ImGui::AlignTextToFramePadding();
    ImGui::TextWrapped(g_isLinuxWine ? "Roblox Executable Name/PIDs (Space Separated):" : "Roblox Executable Name:");
    ImGui::SameLine();
    ImGui::SetNextItemWidth(250.0f);
    if (ImGui::InputText("##SettingsTextbox", settingsBuffer, sizeof(settingsBuffer))) {
        TrimWhitespace(settingsBuffer);
    }

    ImGui::SameLine();
    if (ImGui::Button("R", ImVec2(25, 0))) {
        std::snprintf(settingsBuffer, sizeof(settingsBuffer), "%s", g_isLinuxWine ? "sober" : "RobloxPlayerBeta.exe");
    }
    ImGui::SameLine();

    ImDrawList* drawList = ImGui::GetWindowDrawList();
    ImVec2 pos = ImGui::GetCursorScreenPos();
    pos.y += ImGui::GetTextLineHeight() / 2 - 3;
    ImU32 color = processFound ? ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().success_color) : ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().error_color);
    drawList->AddCircleFilled(ImVec2(pos.x + 5, pos.y + 6), 5, color);
    ImGui::Dummy(ImVec2(10, 10));
    if (!processFound) {
        ImGui::SameLine();
        ImGui::SetCursorPosY(ImGui::GetCursorPosY() - 1);
        if (!g_isLinuxWine) {
            ImGui::TextWrapped("Roblox Not Found");
        }
    }

    ImGui::SameLine(ImGui::GetWindowWidth() - 360);
    ImVec2 tooltipCursorPos = ImGui::GetCursorScreenPos();
    ImGui::Text("Toggle Anti-AFK (");
    ImGui::SameLine(0, 0);
    ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_primary);
    ImGui::Text("?");
    ImGui::PopStyleColor();
    ImGui::SameLine(0, 0);
    ImGui::Text("):");
    ImGui::SetCursorScreenPos(tooltipCursorPos);
    ImVec2 textSizeCalc = ImGui::CalcTextSize("Toggle Anti-AFK (?)");
    ImGui::InvisibleButton("##tooltip", textSizeCalc);
    if (ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled)) {
        ImGui::SetTooltip("Anti-AFK only works on Windows right now due to Linux restraints.\nAnti-AFK functions by counting up a timer constantly. If you are tabbed into roblox\nand you press any key on your keyboard, the timer resets.\nIf the timer expires, if the current active window is Roblox, it will press the \"\\\" key\ntwo times, which will toggle on and off UI navigation.");
    }
    ImGui::SetCursorScreenPos(ImVec2(tooltipCursorPos.x + textSizeCalc.x, tooltipCursorPos.y));
    ImGui::SameLine(ImGui::GetCursorScreenPos().x + 5);
    ImGui::Checkbox("##AntiAFKToggle", &antiafktoggle);

    ImGui::SameLine(ImGui::GetWindowWidth() - 130);
    ImGui::Text("%s", ("VERSION " + localVersion).c_str());

    ImGui::AlignTextToFramePadding();
    ImGui::TextWrapped("Roblox Sensitivity (0-4):");
    ImGui::SameLine();
    ImGui::SetNextItemWidth(70.0f);
    if (ImGui::InputText("##Sens", RobloxSensValue, sizeof(RobloxSensValue), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
        PreviousSensValue = -1;
        float sensValue = static_cast<float>(std::atof(RobloxSensValue));
        if (sensValue != 0.0f) {
            for (auto& winst : wallhop_instances) {
                float pixels = static_cast<float>(std::atof(winst.WallhopDegrees)) * (camfixtoggle ? 1000.0f : 720.0f) / (360.0f * sensValue);
                std::snprintf(winst.WallhopPixels, sizeof(WallhopInstance::WallhopPixels), "%.0f", pixels);
                try {
                    winst.wallhop_dx = static_cast<int>(std::round(std::stoi(winst.WallhopPixels)));
                    winst.wallhop_dy = -static_cast<int>(std::round(std::stoi(winst.WallhopPixels)));
                } catch (...) {}
            }
        }
    }
    ImGui::SameLine();
    ImGui::TextWrapped("Your Roblox FPS:");
    ImGui::SameLine();
    ImGui::SetNextItemWidth(40.0f);
    if (ImGui::InputText("##FPS", RobloxFPSChar, sizeof(RobloxFPSChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
        try { RobloxFPS = std::stoi(RobloxFPSChar); } catch (...) {}
    }
    ImGui::SameLine();
    ImGui::Text("Game Uses Cam-Fix:");
    ImGui::SameLine();

    if (ImGui::Checkbox("##CamfixToggle", &camfixtoggle) || PreviousSensValue == -1) {
        PreviousSensValue = -1;
        PreviousWallWalkValue = -1;
        float currentWallWalkValue = static_cast<float>(std::atof(RobloxSensValue));
        float baseValue = camfixtoggle ? 500.0f : 360.0f;
        if (currentWallWalkValue != 0.0f) {
            wallwalk_strengthx = -static_cast<int>(std::round((baseValue / currentWallWalkValue) * 0.13f));
            wallwalk_strengthy = static_cast<int>(std::round((baseValue / currentWallWalkValue) * 0.13f));
            std::snprintf(RobloxWallWalkValueChar, sizeof(RobloxWallWalkValueChar), "%d", wallwalk_strengthx);
        }
        float currentSensValue = static_cast<float>(std::atof(RobloxSensValue));
        if (currentSensValue != 0.0f) {
            float speedBase = camfixtoggle ? 500.0f : 360.0f;
            RobloxPixelValue = static_cast<int>(std::round((speedBase / currentSensValue) * (359.0f / 360.0f) * (359.0f / 360.0f)));
            PreviousSensValue = currentSensValue;
            std::snprintf(RobloxPixelValueChar, sizeof(RobloxPixelValueChar), "%d", RobloxPixelValue);
            speed_strengthx = RobloxPixelValue;
            speed_strengthy = -RobloxPixelValue;
        }
    }

    static bool showSettingsMenu = false;
    ImGui::SameLine(ImGui::GetWindowWidth() - 107);
    if (showSettingsMenu ? ImGui::Button("Settings <-") : ImGui::Button("Settings ->")) {
        showSettingsMenu = !showSettingsMenu;
    }
    RenderSettingsMenu(context, &showSettingsMenu);

    ImGui::SameLine(ImGui::GetWindowWidth() - 257);
    if (show_theme_menu ? ImGui::Button("Theme Editor <-") : ImGui::Button("Theme Editor ->")) {
        show_theme_menu = !show_theme_menu;
    }
    RenderSharedThemeEditor(&show_theme_menu);

    (void)displaySize;
}

void RenderSectionSidebar(float leftPanelWidth)
{
    for (size_t displayIndex = 0; displayIndex < section_amounts; ++displayIndex) {
        int i = section_order[displayIndex];
        ImGui::PushID(i);

        float buttonWidth = leftPanelWidth - ImGui::GetStyle().FramePadding.x * 2;
        Globals::Theme& theme = GetCurrentTheme();

        bool sectionActive = section_toggles[i];
        if (i == 5) {
            sectionActive = !presskey_instances.empty() && presskey_instances[0].section_enabled;
        } else if (i == 6) {
            sectionActive = !wallhop_instances.empty() && wallhop_instances[0].section_enabled;
        } else if (i == 11) {
            sectionActive = !spamkey_instances.empty() && spamkey_instances[0].section_enabled;
        }

        if (sectionActive) {
            if (selected_section == i) {
                ImGui::PushStyleColor(ImGuiCol_Button, Brighten(theme.accent_primary, 1.4f));
                ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.accent_primary, 1.6f));
                ImGui::PushStyleColor(ImGuiCol_ButtonActive, Brighten(theme.accent_primary, 1.7f));
            } else {
                ImGui::PushStyleColor(ImGuiCol_Button, theme.accent_primary);
                ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.accent_primary, 1.3f));
                ImGui::PushStyleColor(ImGuiCol_ButtonActive, Brighten(theme.accent_primary, 0.8f));
            }
        } else {
            if (selected_section == i) {
                ImGui::PushStyleColor(ImGuiCol_Button, Brighten(theme.disabled_color, 1.6f));
                ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.disabled_color, 1.9f));
                ImGui::PushStyleColor(ImGuiCol_ButtonActive, Brighten(theme.disabled_color, 2.1f));
            } else {
                ImGui::PushStyleColor(ImGuiCol_Button, theme.disabled_color);
                ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.disabled_color, 1.3f));
                ImGui::PushStyleColor(ImGuiCol_ButtonActive, Brighten(theme.disabled_color, 1.1f));
            }
        }

        ImVec2 titleSize = ImGui::CalcTextSize(sections[i].title.c_str(), nullptr, true);
        ImVec2 descriptionSize = ImGui::CalcTextSize(sections[i].description.c_str(), nullptr, true, buttonWidth - 20);
        float buttonHeight = titleSize.y + descriptionSize.y + ImGui::GetStyle().FramePadding.y * 2;
        const float scrollAdjustment = ImGui::GetScrollMaxY() == 0 ? 7.0f : 18.0f;
        if (ImGui::Button("", ImVec2(buttonWidth - scrollAdjustment, buttonHeight))) {
            selected_section = i;
            if (i == 5) selected_presskey_instance = 0;
            else if (i == 6) selected_wallhop_instance = 0;
            else if (i == 11) selected_spamkey_instance = 0;
        }

        if (ImGui::BeginDragDropSource(ImGuiDragDropFlags_None)) {
            int payloadIndex = static_cast<int>(displayIndex);
            ImGui::SetDragDropPayload("DND_SECTION", &payloadIndex, sizeof(int));
            ImGui::EndDragDropSource();
        }
        if (ImGui::BeginDragDropTarget()) {
            if (const ImGuiPayload* payload = ImGui::AcceptDragDropPayload("DND_SECTION")) {
                int payloadIndex = *(const int*)payload->Data;
                std::swap(section_order[payloadIndex], section_order[displayIndex]);
            }
            ImGui::EndDragDropTarget();
        }

        ImVec2 buttonPos = ImGui::GetItemRectMin();
        ImVec2 textPos(buttonPos.x + ImGui::GetStyle().FramePadding.x, buttonPos.y + ImGui::GetStyle().FramePadding.y);
        ImDrawList* drawList = ImGui::GetWindowDrawList();
        drawList->AddText(textPos, ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().text_primary), sections[i].title.c_str());

        std::stringstream ss(sections[i].description);
        std::string word;
        std::string currentLine;
        textPos.y += titleSize.y;
        const float wrapWidth = buttonWidth - (ImGui::GetScrollMaxY() == 0 ? 11.0f : 22.0f);
        while (ss >> word) {
            std::string potentialLine = currentLine + (currentLine.empty() ? "" : " ") + word;
            if (ImGui::CalcTextSize(potentialLine.c_str()).x > wrapWidth) {
                drawList->AddText(textPos, ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().text_primary), currentLine.c_str());
                textPos.y += ImGui::GetTextLineHeight();
                currentLine = word;
            } else {
                currentLine = potentialLine;
            }
        }
        if (!currentLine.empty()) {
            drawList->AddText(textPos, ImGui::ColorConvertFloat4ToU32(GetCurrentTheme().text_primary), currentLine.c_str());
        }

        ImGui::PopStyleColor(3);
        ImGui::PopID();

        if (i == 5 || i == 6 || i == 11) {
            float subBtnW = (ImGui::GetScrollMaxY() == 0) ? (buttonWidth - 7) : (buttonWidth - 18);
            size_t instCount = (i == 5) ? presskey_instances.size() : (i == 6) ? wallhop_instances.size() : spamkey_instances.size();
            int& selInst = (i == 5) ? selected_presskey_instance : (i == 6) ? selected_wallhop_instance : selected_spamkey_instance;

            for (size_t j = 1; j < instCount; ++j) {
                ImGui::PushID(static_cast<int>(j) * 1000 + i);
                bool instEnabled = (i == 5) ? presskey_instances[j].section_enabled : (i == 6) ? wallhop_instances[j].section_enabled : spamkey_instances[j].section_enabled;
                bool isSelJ = selected_section == i && selInst == static_cast<int>(j);
                ImGui::PushStyleColor(ImGuiCol_Button, instEnabled ? (isSelJ ? Brighten(theme.accent_primary, 1.4f) : theme.accent_primary) : (isSelJ ? Brighten(theme.disabled_color, 1.6f) : theme.disabled_color));
                ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.accent_primary, 1.3f));
                ImGui::PushStyleColor(ImGuiCol_ButtonActive, Brighten(theme.accent_primary, 0.8f));
                std::string subLabel = BuildMultiInstanceName(i, static_cast<int>(j) + 1);
                if (ImGui::Button(subLabel.c_str(), ImVec2(subBtnW, 0))) {
                    selected_section = i;
                    selInst = static_cast<int>(j);
                }
                ImGui::PopStyleColor(3);
                ImGui::PopID();
            }

            ImGui::PushID(9999 + i);
            ImGui::PushStyleColor(ImGuiCol_Button, theme.bg_light);
            ImGui::PushStyleColor(ImGuiCol_ButtonHovered, Brighten(theme.bg_light, 1.3f));
            ImGui::PushStyleColor(ImGuiCol_ButtonActive, Brighten(theme.bg_light, 1.1f));
            if (ImGui::Button("+", ImVec2(subBtnW, 0))) {
                if (i == 5) presskey_instances.emplace_back();
                else if (i == 6) wallhop_instances.emplace_back();
                else if (i == 11) spamkey_instances.emplace_back();
            }
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("Add a new independent instance of this macro");
            }
            ImGui::PopStyleColor(3);
            ImGui::PopID();
        }

        ImGui::Separator();
    }
}

void RenderRemoveInstanceButton(const std::string& id, int sectionIndex, size_t instanceCount, int selectedInstance, std::atomic<int>& removeRequest)
{
    if (instanceCount <= 1) {
        return;
    }

    const int removeTarget = GetInstanceRemovalTargetIndex(instanceCount, selectedInstance);
    const bool removingMainSlot = selectedInstance == 0;
    const bool isConfirmArmed = g_instance_remove_confirm_state.stage == 1 &&
        g_instance_remove_confirm_state.section_index == sectionIndex &&
        g_instance_remove_confirm_state.target_instance_index == removeTarget;

    ImGui::SameLine();
    if (isConfirmArmed) {
        ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(0.0f, 0.0f, 0.0f, 1.0f));
        ImGui::PushStyleColor(ImGuiCol_Button, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
        ImGui::PushStyleColor(ImGuiCol_ButtonHovered, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
        ImGui::PushStyleColor(ImGuiCol_ButtonActive, ImVec4(1.0f, 1.0f, 0.0f, 1.0f));
    }

    std::string label = removingMainSlot ? "- Remove Last Instance" : "- Remove Current Instance";
    bool clicked = ImGui::Button((label + "##" + id).c_str());

    if (isConfirmArmed) {
        ImGui::PopStyleColor(4);
    }

    if (clicked) {
        if (isConfirmArmed) {
            removeRequest.store(removeTarget, std::memory_order_release);
            if (sectionIndex == 5 && removeTarget > 0 && removeTarget < static_cast<int>(presskey_instances.size())) {
                for (int i = removeTarget; i < static_cast<int>(presskey_instances.size()) - 1; ++i) {
                    CopyPresskeyInstanceData(presskey_instances[i + 1], presskey_instances[i]);
                }
                presskey_instances.pop_back();
                selected_presskey_instance = std::min(selected_presskey_instance, static_cast<int>(presskey_instances.size()) - 1);
            } else if (sectionIndex == 6 && removeTarget > 0 && removeTarget < static_cast<int>(wallhop_instances.size())) {
                for (int i = removeTarget; i < static_cast<int>(wallhop_instances.size()) - 1; ++i) {
                    CopyWallhopInstanceData(wallhop_instances[i + 1], wallhop_instances[i]);
                }
                wallhop_instances.pop_back();
                selected_wallhop_instance = std::min(selected_wallhop_instance, static_cast<int>(wallhop_instances.size()) - 1);
            } else if (sectionIndex == 11 && removeTarget > 0 && removeTarget < static_cast<int>(spamkey_instances.size())) {
                for (int i = removeTarget; i < static_cast<int>(spamkey_instances.size()) - 1; ++i) {
                    CopySpamkeyInstanceData(spamkey_instances[i + 1], spamkey_instances[i]);
                }
                spamkey_instances.pop_back();
                selected_spamkey_instance = std::min(selected_spamkey_instance, static_cast<int>(spamkey_instances.size()) - 1);
            }
            ResetInstanceRemoveConfirmState();
        } else {
            g_instance_remove_confirm_state.stage = 1;
            g_instance_remove_confirm_state.timer = 0.7f;
            g_instance_remove_confirm_state.section_index = sectionIndex;
            g_instance_remove_confirm_state.target_instance_index = removeTarget;
        }
    }
}

void RenderSelectedSection(AppContext& context)
{
    if (selected_section < 0 || selected_section >= static_cast<int>(sections.size())) {
        ImGui::TextWrapped("Select a section to see its settings.");
        return;
    }

    unsigned int* currentKey = nullptr;
    bool* instanceEnabledPtr = nullptr;
    if (selected_section == 5 && !presskey_instances.empty()) {
        selected_presskey_instance = std::clamp(selected_presskey_instance, 0, static_cast<int>(presskey_instances.size()) - 1);
        currentKey = &presskey_instances[selected_presskey_instance].vk_trigger;
        instanceEnabledPtr = &presskey_instances[selected_presskey_instance].section_enabled;
    } else if (selected_section == 6 && !wallhop_instances.empty()) {
        selected_wallhop_instance = std::clamp(selected_wallhop_instance, 0, static_cast<int>(wallhop_instances.size()) - 1);
        currentKey = &wallhop_instances[selected_wallhop_instance].vk_trigger;
        instanceEnabledPtr = &wallhop_instances[selected_wallhop_instance].section_enabled;
    } else if (selected_section == 11 && !spamkey_instances.empty()) {
        selected_spamkey_instance = std::clamp(selected_spamkey_instance, 0, static_cast<int>(spamkey_instances.size()) - 1);
        currentKey = &spamkey_instances[selected_spamkey_instance].vk_trigger;
        instanceEnabledPtr = &spamkey_instances[selected_spamkey_instance].section_enabled;
    } else {
        currentKey = section_to_key.at(selected_section);
    }

    if ((selected_section == 5 || selected_section == 6 || selected_section == 11) &&
        ((selected_section == 5 && presskey_instances.size() > 1) || (selected_section == 6 && wallhop_instances.size() > 1) || (selected_section == 11 && spamkey_instances.size() > 1))) {
        int instanceIndex = selected_section == 5 ? selected_presskey_instance : selected_section == 6 ? selected_wallhop_instance : selected_spamkey_instance;
        ImGui::TextWrapped("Settings for %s", BuildMultiInstanceName(selected_section, instanceIndex + 1).c_str());
    } else {
        ImGui::TextWrapped("Settings for %s", sections[selected_section].title.c_str());
    }
    ImGui::Separator();
    ImGui::NewLine();

    ImGui::TextWrapped("Keybind:");
    ImGui::SameLine();
    DrawKeyBindControl("SectionKey", *currentKey, selected_section);

    bool toggleVal = instanceEnabledPtr ? *instanceEnabledPtr : section_toggles[selected_section];
    ImGui::PushStyleColor(ImGuiCol_Text, toggleVal ? GetCurrentTheme().success_color : GetCurrentTheme().error_color);
    ImGui::TextWrapped("Enable This Macro:");
    ImGui::PopStyleColor();
    ImGui::SameLine();
    if (instanceEnabledPtr) {
        ImGui::Checkbox(("##SectionToggle" + std::to_string(selected_section)).c_str(), instanceEnabledPtr);
    } else {
        ImGui::Checkbox(("##SectionToggle" + std::to_string(selected_section)).c_str(), &section_toggles[selected_section]);
    }

    if (bool* disableOutsidePtr = GetDisableOutsideTogglePtr(selected_section)) {
        std::string disableOutsideId = "##DisableOutsideRoblox_" + std::to_string(selected_section);
        if (selected_section == 5) disableOutsideId += "_" + std::to_string(selected_presskey_instance);
        else if (selected_section == 6) disableOutsideId += "_" + std::to_string(selected_wallhop_instance);
        else if (selected_section == 11) disableOutsideId += "_" + std::to_string(selected_spamkey_instance);
        ImGui::SameLine();
        RenderForegroundDependentCheckbox(context, "Disable outside of Roblox:", disableOutsideId.c_str(), disableOutsidePtr);
    }

    if (selected_section == 0) {
        ImGui::TextWrapped("Automatically Unfreeze after this amount of seconds (Anti-Internet-Kick)");
        ImGui::SetNextItemWidth(60.0f);
        ImGui::InputFloat("##FreezeFloat", &maxfreezetime, 0.0f, 0.0f, "%.2f");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(300.0f);
        ImGui::SliderFloat("##FreezeSlider", &maxfreezetime, 0.0f, 9.8f, "%.2f Seconds");
        char maxfreezeoverrideBuffer[16];
        std::snprintf(maxfreezeoverrideBuffer, sizeof(maxfreezeoverrideBuffer), "%d", maxfreezeoverride);
        ImGui::SetNextItemWidth(50.0f);
        if (ImGui::InputText("Modify 50ms Default Unfreeze Time (MS)", maxfreezeoverrideBuffer, sizeof(maxfreezeoverrideBuffer), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            maxfreezeoverride = std::atoi(maxfreezeoverrideBuffer);
        }
        ImGui::Checkbox("Switch from Hold Key to Toggle Key", &isfreezeswitch);
        if (isfreezeswitch || takeallprocessids) {
            disable_outside_roblox[0] = false;
        }
        ImGui::Checkbox("Freeze all Found Processes Instead of Newest", &takeallprocessids);
        ImGui::SameLine();
        ImGui::TextWrapped("(ONLY EVER USE FOR COMPATIBILITY ISSUES WITH NON-ROBLOX GAMES)");
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("Hold the hotkey to freeze your game, let go of it to release it. Suspending your game also pauses ALL network and physics activity that the server sends or recieves from you.");
    }

    if (selected_section == 1) {
        ImGui::TextWrapped("Gear Slot:");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(30.0f);
        if (ImGui::InputText("##ItemDesync", ItemDesyncSlot, sizeof(ItemDesyncSlot), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            try { desync_slot = std::stoi(ItemDesyncSlot); } catch (...) {}
        }
        ImGui::Separator();
        ImGui::TextWrapped("Equip your item inside of the slot you have picked here, then hold the keybind for 4-7 seconds");
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("This Macro rapidly sends number inputs to your roblox client, enough that the server begins to throttle you. The item that you're holding must not make a serverside sound, else desyncing yourself will be very buggy, and you will be unable to send any physics data to the server.");
        ImGui::Separator();
        ImGui::TextWrapped("If 'Disable outside of Roblox' is enabled, this module will only run while tabbed into Roblox. Turning it off allows use in other windows (use carefully).");
    }

    if (selected_section == 2) {
        ImGui::Checkbox("Automatically time inputs", &autotoggle);
        ImGui::SameLine();
        ImGui::TextWrapped("(EXTREMELY BUGGY/EXPERIMENTAL, WORKS BEST ON HIGH FPS AND SHALLOW ANGLE TO WALL)");
        ImGui::Checkbox("Decrease Freeze Duration (Speedrunner Mode)", &fasthhj);
        if (ImGui::Button("R##HHJLength", ImVec2(25, 0))) {
            HHJLength = 243;
            std::snprintf(HHJLengthChar, sizeof(HHJLengthChar), "%d", HHJLength);
        }
        ImGui::SameLine();
        ImGui::TextWrapped("Length of HHJ flicks (ms):");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(52);
        if (ImGui::InputText("##HHJLength", HHJLengthChar, sizeof(HHJLengthChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            try { HHJLength = std::stoi(HHJLengthChar); } catch (...) {}
        }
        if (ImGui::CollapsingHeader("Advanced HHJ Options", showadvancedhhj ? ImGuiTreeNodeFlags_DefaultOpen : 0)) {
            showadvancedhhj = true;
            auto drawTiming = [](const char* resetId, const char* label, int& value, char* buffer, size_t bufferSize, int defaultValue) {
                ImGui::Indent();
                if (ImGui::Button(resetId, ImVec2(25, 0))) {
                    value = defaultValue;
                    std::snprintf(buffer, bufferSize, "%d", value);
                }
                ImGui::SameLine();
                ImGui::TextWrapped("%s", label);
                ImGui::SameLine();
                ImGui::SetNextItemWidth(50);
                if (ImGui::InputText((std::string("##") + resetId).c_str(), buffer, bufferSize, ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                    try { value = std::stoi(buffer); } catch (...) {}
                }
                ImGui::Unindent();
            };
            ImGui::Spacing();
            ImGui::Indent();
            if (ImGui::Button("R##HHJFreezeDelayOverride", ImVec2(25, 0))) {
                HHJFreezeDelayOverride = 500;
                std::snprintf(HHJFreezeDelayOverrideChar, sizeof(HHJFreezeDelayOverrideChar), "%d", HHJFreezeDelayOverride);
                HHJFreezeDelayApply = false;
            }
            ImGui::SameLine();
            ImGui::TextWrapped("Set freeze delay (ms): ");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(50);
            if (ImGui::InputText("##HHJFreezeDelayOverride", HHJFreezeDelayOverrideChar, sizeof(HHJFreezeDelayOverrideChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try { HHJFreezeDelayOverride = std::stoi(HHJFreezeDelayOverrideChar); } catch (...) {}
            }
            ImGui::SameLine();
            ImGui::Checkbox("Apply##HHJFreezeDelayApply", &HHJFreezeDelayApply);
            ImGui::Unindent();
            drawTiming("R##HHJDelay1", "Delay after freezing before shiftlock is held (ms): ", HHJDelay1, HHJDelay1Char, sizeof(HHJDelay1Char), 9);
            drawTiming("R##HHJDelay2", "Time shiftlock is held before spinning (ms): ", HHJDelay2, HHJDelay2Char, sizeof(HHJDelay2Char), 17);
            drawTiming("R##HHJDelay3", "Time shiftlock is held after freezing (ms): ", HHJDelay3, HHJDelay3Char, sizeof(HHJDelay3Char), 16);
        }
        if (ImGui::CollapsingHeader("Customize Automatic HHJ", showautomatichhj ? ImGuiTreeNodeFlags_DefaultOpen : 0)) {
            showautomatichhj = true;
            ImGui::Spacing();
            ImGui::TextWrapped("First Key Press:");
            ImGui::Indent();
            if (ImGui::Button("R##AutoHHJKey1Reset", ImVec2(25, 0))) vk_autohhjkey1 = VK_SPACE;
            ImGui::SameLine();
            DrawKeyBindControl("AutoHHJKey1", vk_autohhjkey1, selected_section, 150.0f, 50.0f);
            if (ImGui::Button("R##AutoHHJKey1Time", ImVec2(25, 0))) {
                AutoHHJKey1Time = 550;
                std::snprintf(AutoHHJKey1TimeChar, sizeof(AutoHHJKey1TimeChar), "%d", AutoHHJKey1Time);
            }
            ImGui::SameLine();
            ImGui::TextWrapped("Hold time (ms): ");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(50);
            if (ImGui::InputText("##AutoHHJKey1Time", AutoHHJKey1TimeChar, sizeof(AutoHHJKey1TimeChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try { AutoHHJKey1Time = std::stoi(AutoHHJKey1TimeChar); } catch (...) {}
            }
            ImGui::Unindent();
            ImGui::Spacing();
            ImGui::TextWrapped("Second Key Press:");
            ImGui::Indent();
            if (ImGui::Button("R##AutoHHJKey2Reset", ImVec2(25, 0))) vk_autohhjkey2 = VK_W;
            ImGui::SameLine();
            DrawKeyBindControl("AutoHHJKey2", vk_autohhjkey2, selected_section, 150.0f, 50.0f);
            if (ImGui::Button("R##AutoHHJKey2Time", ImVec2(25, 0))) {
                AutoHHJKey2Time = 68;
                std::snprintf(AutoHHJKey2TimeChar, sizeof(AutoHHJKey2TimeChar), "%d", AutoHHJKey2Time);
            }
            ImGui::SameLine();
            ImGui::TextWrapped("Hold time (ms): ");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(50);
            if (ImGui::InputText("##AutoHHJKey2Time", AutoHHJKey2TimeChar, sizeof(AutoHHJKey2TimeChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try { AutoHHJKey2Time = std::stoi(AutoHHJKey2TimeChar); } catch (...) {}
            }
            ImGui::Unindent();
        }
        ImGui::Separator();
        ImGui::TextWrapped("This module abuses Roblox's conversion from angular velocity to regular velocity, and its flawed centre of mass calculation.");
        ImGui::Separator();
        ImGui::TextWrapped("IMPORTANT:");
        ImGui::TextWrapped("Have your Sensitivity and Cam-Fix options set before using this module.");
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("Assuming unequip com offset is used prior to offset com, align yourself with your back against the wall and rotate slightly to the left. Now turn your camera towards the wall, hold W, and press the assigned hotkey.");
    }

    if (selected_section == 3) {
        float currentSensValue = static_cast<float>(std::atof(RobloxSensValue));
        if (currentSensValue != 0.0f && currentSensValue != PreviousSensValue) {
            RobloxPixelValue = static_cast<int>(((camfixtoggle ? 500.0f : 360.0f) / currentSensValue) * (359.0f / 360.0f) + 0.5f);
            PreviousSensValue = currentSensValue;
            std::snprintf(RobloxPixelValueChar, sizeof(RobloxPixelValueChar), "%d", RobloxPixelValue);
        }
        ImGui::TextWrapped("Pixel Value for 180 Degree Turn BASED ON SENSITIVITY:");
        ImGui::SetNextItemWidth(90.0f);
        ImGui::SameLine();
        if (ImGui::InputText("##PixelValue", RobloxPixelValueChar, sizeof(RobloxPixelValueChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            try {
                speed_strengthx = std::stoi(RobloxPixelValueChar);
                speed_strengthy = -std::stoi(RobloxPixelValueChar);
            } catch (...) {}
        }
        ImGui::Checkbox("Switch from Toggle Key to Hold Key", &isspeedswitch);
        ImGui::TextWrapped("This module abuses Roblox's conversion from angular velocity to regular velocity, and its flawed centre of mass calculation.");
        ImGui::Separator();
        ImGui::TextWrapped("IMPORTANT: Have your Sensitivity and Cam-Fix options set before using this module.");
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("Enable shiftlock mode, press the keybind once to start the macro, then jump and hold W. The macro should rotate you exactly 180 degrees.");
    }

    if (selected_section == 4) {
        ImGui::TextWrapped("Gear Slot:");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(30.0f);
        if (ImGui::InputText("##Gearslot", ItemSpeedSlot, sizeof(ItemSpeedSlot), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            try { speed_slot = std::stoi(ItemSpeedSlot); } catch (...) {}
        }
        ImGui::TextWrapped("Type in a custom chat message! (Disables gear equipping, just pastes your message in chat)");
        ImGui::TextWrapped("(Leave this blank if you don't want a custom message)");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(ImGui::GetContentRegionAvail().x);
        ImGui::InputText("##CustomText", CustomTextChar, sizeof(CustomTextChar));
        ImGui::SetNextItemWidth(150.0f);
        if (ImGui::BeginCombo("Select Emote", optionsforoffset[selected_dropdown])) {
            for (int i = 0; i < IM_ARRAYSIZE(optionsforoffset); i++) {
                bool isSelected = selected_dropdown == i;
                if (ImGui::Selectable(optionsforoffset[i], isSelected)) {
                    selected_dropdown = i;
                    text = optionsforoffset[selected_dropdown];
                }
                if (isSelected) ImGui::SetItemDefaultFocus();
            }
            ImGui::EndCombo();
        }
        ImGui::AlignTextToFramePadding();
        ImGui::TextWrapped("Key to Press After Message/Emote paste:");
        ImGui::SameLine();
        DrawKeyBindControl("EnterKey", vk_enterkey, selected_section, 150.0f, 50.0f);
        ImGui::Checkbox("Let the macro Keep the item equipped", &unequiptoggle);
        ImGui::Separator();
        ImGui::TextWrapped("IMPORTANT: This glitch has been patched by Roblox. This is currently deprecated. You may get a COM offset manually by equipping an item without doing any animations, but it will be very small.");
        ImGui::Separator();
        ImGui::TextWrapped("This module allows you to trick Roblox into thinking your centre of mass is elsewhere. This is used in the Helicopter High Jump, Speed Glitch and Walless LHJ modules.");
    }

    if (selected_section == 5) {
        if (presskey_instances.empty()) {
            ImGui::TextWrapped("No presskey instances.");
        } else {
            PresskeyInstance& inst = presskey_instances[selected_presskey_instance];
            std::string sid = "_pk" + std::to_string(selected_presskey_instance);
            if (presskey_instances.size() > 1) {
                ImGui::TextWrapped("%s of %d", BuildMultiInstanceName(5, selected_presskey_instance + 1).c_str(), static_cast<int>(presskey_instances.size()));
                ImGui::SameLine();
            }
            if (ImGui::Button(("+ Add New##AddPK" + sid).c_str())) presskey_instances.emplace_back();
            RenderRemoveInstanceButton("RemPK" + sid, 5, presskey_instances.size(), selected_presskey_instance, request_remove_presskey_instance_index);
            ImGui::Separator();
            ImGui::TextWrapped("Key to Press:");
            ImGui::SameLine();
            DrawKeyBindControl(("PressKey" + sid).c_str(), inst.vk_presskey, selected_section);
            ImGui::Text("Length of Second Button Press (ms):");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(80);
            if (ImGui::InputText(("##PressKeyDelayChar" + sid).c_str(), inst.PressKeyDelayChar, sizeof(PresskeyInstance::PressKeyDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try { inst.PressKeyDelay = std::stoi(inst.PressKeyDelayChar); } catch (...) {}
            }
            ImGui::Text("Delay Before Second Press (ms):");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(80);
            if (ImGui::InputText(("##PressKeyBonusDelayChar" + sid).c_str(), inst.PressKeyBonusDelayChar, sizeof(PresskeyInstance::PressKeyBonusDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try { inst.PressKeyBonusDelay = std::stoi(inst.PressKeyBonusDelayChar); } catch (...) {}
            }
            ImGui::Separator();
            ImGui::TextWrapped("Explanation:");
            ImGui::NewLine();
            ImGui::TextWrapped("It will press the second keybind for a single frame whenever you press the first keybind. This is most commonly used for micro-adjustments while moving, especially if you do this while jumping.");
        }
    }

    if (selected_section == 6) {
        if (wallhop_instances.empty()) {
            ImGui::TextWrapped("No wallhop instances.");
        } else {
            WallhopInstance& inst = wallhop_instances[selected_wallhop_instance];
            std::string sid = "_wh" + std::to_string(selected_wallhop_instance);
            if (wallhop_instances.size() > 1) {
                ImGui::TextWrapped("%s of %d", BuildMultiInstanceName(6, selected_wallhop_instance + 1).c_str(), static_cast<int>(wallhop_instances.size()));
                ImGui::SameLine();
            }
            if (ImGui::Button(("+ Add New##AddWH" + sid).c_str())) wallhop_instances.emplace_back();
            RenderRemoveInstanceButton("RemWH" + sid, 6, wallhop_instances.size(), selected_wallhop_instance, request_remove_wallhop_instance_index);
            ImGui::Separator();
            ImGui::TextWrapped("Flick Degrees (Estimated):");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(70.0f);
            float sensValue = static_cast<float>(std::atof(RobloxSensValue));
            if (sensValue != 0.0f) {
                std::snprintf(inst.WallhopDegrees, sizeof(WallhopInstance::WallhopDegrees), "%d", static_cast<int>(360 * (std::atof(inst.WallhopPixels) * std::atof(RobloxSensValue)) / (camfixtoggle ? 1000 : 720)));
            }
            if (ImGui::InputText(("##WallhopDegrees" + sid).c_str(), inst.WallhopDegrees, sizeof(WallhopInstance::WallhopDegrees), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                float pixels = static_cast<float>(std::atof(inst.WallhopDegrees) * (camfixtoggle ? 1000.0f : 720.0f) / (360.0f * std::atof(RobloxSensValue)));
                std::snprintf(inst.WallhopPixels, sizeof(WallhopInstance::WallhopPixels), "%.0f", pixels);
                try {
                    inst.wallhop_dx = static_cast<int>(std::round(std::stoi(inst.WallhopPixels)));
                    inst.wallhop_dy = -static_cast<int>(std::round(std::stoi(inst.WallhopPixels)));
                } catch (...) {}
            }
            ImGui::TextWrapped("Flick Pixel Amount:");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(70.0f);
            if (ImGui::InputText(("##WallhopPixels" + sid).c_str(), inst.WallhopPixels, sizeof(WallhopInstance::WallhopPixels), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try {
                    inst.wallhop_dx = static_cast<int>(std::round(std::stoi(inst.WallhopPixels)));
                    inst.wallhop_dy = -static_cast<int>(std::round(std::stoi(inst.WallhopPixels)));
                } catch (...) {}
            }
            ImGui::SameLine();
            ImGui::Text("Vertical Pixel Movement:");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(70.0f);
            if (ImGui::InputText(("##WallhopVertical" + sid).c_str(), inst.WallhopVerticalChar, sizeof(inst.WallhopVerticalChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try { inst.wallhop_vertical = static_cast<int>(std::round(std::stoi(inst.WallhopVerticalChar))); } catch (...) {}
            }
            ImGui::TextWrapped("Wallhop Length (ms):");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(70.0f);
            if (ImGui::InputText(("##WallhopDelay" + sid).c_str(), inst.WallhopDelayChar, sizeof(WallhopInstance::WallhopDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try { inst.WallhopDelay = static_cast<int>(std::round(std::stoi(inst.WallhopDelayChar))); } catch (...) {}
            }
            ImGui::TextWrapped("Bonus Wallhop Delay Before Jumping (ms):");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(70.0f);
            if (ImGui::InputText(("##WallhopBonusDelay" + sid).c_str(), inst.WallhopBonusDelayChar, sizeof(WallhopInstance::WallhopBonusDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try { inst.WallhopBonusDelay = static_cast<int>(std::round(std::stoi(inst.WallhopBonusDelayChar))); } catch (...) {}
            }
            ImGui::Checkbox(("Switch to Left-Flick Wallhop##" + sid).c_str(), &inst.wallhopswitch);
            ImGui::Checkbox(("Jump During Wallhop##" + sid).c_str(), &inst.toggle_jump);
            ImGui::Checkbox(("Flick-Back During Wallhop##" + sid).c_str(), &inst.toggle_flick);
            if (ImGui::CollapsingHeader(("Hotkeys##WallhopHotkeys" + sid).c_str(), ImGuiTreeNodeFlags_DefaultOpen)) {
                if (ImGui::Button(("R##WallhopJumpKeyReset" + sid).c_str(), ImVec2(25, 0))) inst.vk_jumpkey = VK_SPACE;
                ImGui::SameLine();
                ImGui::TextWrapped("Jump Key:");
                ImGui::SameLine();
                DrawKeyBindControl(("WallhopJumpKey" + sid).c_str(), inst.vk_jumpkey, selected_section, 150.0f, 50.0f);
            }
            ImGui::Separator();
            ImGui::TextWrapped("IMPORTANT:");
            ImGui::TextWrapped("THE ANGLE THAT YOU TURN IS DIRECTLY RELATED TO YOUR ROBLOX SENSITIVITY. INTEGERS ONLY!");
            ImGui::Separator();
            ImGui::TextWrapped("Explanation:");
            ImGui::NewLine();
            ImGui::TextWrapped("This Macro automatically flicks your screen AND jumps at the same time, performing a wallhop.");
        }
    }

    if (selected_section == 7) {
        ImGui::Checkbox("Switch to Left-Sided LHJ", &wallesslhjswitch);
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("If you offset your center of mass to any direction EXCEPT directly upwards, you will be able to perform 14 stud jumps using this macro. However, you need at LEAST one FULL FOOT on the platform in order to do it.");
    }

    if (selected_section == 8) {
        ImGui::TextWrapped("Item Clip Slot:");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(30.0f);
        if (ImGui::InputText("##ItemClipSlot", ItemClipSlot, sizeof(ItemClipSlot), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            try { clip_slot = std::stoi(ItemClipSlot); } catch (...) {}
        }
        ImGui::TextWrapped("Item Clip Delay in Milliseconds (Default 34ms):");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(120.0f);
        if (ImGui::InputText("##ItemClipDelay", ItemClipDelay, sizeof(ItemClipDelay), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            try { clip_delay = std::stoi(ItemClipDelay); } catch (...) {}
        }
        ImGui::Checkbox("Switch from Toggle Key to Hold Key", &isitemclipswitch);
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("This macro will equip and unequip your item in the amount of milliseconds you put in. It's recommended to shiftlock, jump, and hold W while staying at the wall.");
        ImGui::TextWrapped("If 'Disable outside of Roblox' is enabled, item clip only runs while tabbed into Roblox.");
    }

    if (selected_section == 9) {
        ImGui::Checkbox("Disable S being pressed (Slightly weaker laugh clips, but interferes with movement less)", &laughmoveswitch);
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("MUST BE ABOVE 60 FPS AND IN R6!");
        ImGui::TextWrapped("Go against a wall unshiftlocked and angle your camera DIRECTLY OPPOSITE TO THE WALL. The Macro will Automatically type out /e laugh using the settings inside of the \"Unequip Com\" section.");
    }

    if (selected_section == 10) {
        float currentWallWalkValue = static_cast<float>(std::atof(RobloxSensValue));
        if (currentWallWalkValue != 0.0f && currentWallWalkValue != PreviousWallWalkValue) {
            wallwalk_strengthx = static_cast<int>(std::round(((camfixtoggle ? 500.0f : 360.0f) / currentWallWalkValue) * 0.13f));
            wallwalk_strengthy = -wallwalk_strengthx;
        }
        PreviousWallWalkValue = currentWallWalkValue;
        std::snprintf(RobloxWallWalkValueChar, sizeof(RobloxWallWalkValueChar), "%d", wallwalk_strengthx);
        ImGui::TextWrapped("Wall-Walk Pixel Value BASED ON SENSITIVITY (meant to be low):");
        ImGui::SetNextItemWidth(90.0f);
        ImGui::SameLine();
        ImGui::InputText("##PixelValue", RobloxWallWalkValueChar, sizeof(RobloxWallWalkValueChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
        ImGui::Checkbox("Switch to Left-Flick Wallwalk", &wallwalktoggleside);
        ImGui::SetNextItemWidth(100.0f);
        ImGui::InputText("Delay Between Flicks (Don't change from 72720 unless neccessary):", RobloxWallWalkValueDelayChar, sizeof(RobloxWallWalkValueDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank);
        try { RobloxWallWalkValueDelay = static_cast<int>(std::atof(RobloxWallWalkValueDelayChar)); } catch (...) {}
        try {
            wallwalk_strengthx = std::stoi(RobloxWallWalkValueChar);
            wallwalk_strengthy = -std::stoi(RobloxWallWalkValueChar);
        } catch (...) {}
        ImGui::Checkbox("Switch from Toggle Key to Hold Key", &iswallwalkswitch);
        ImGui::Separator();
        ImGui::TextWrapped("IMPORTANT: FOR MOST OPTIMAL RESULTS, INPUT YOUR ROBLOX INGAME SENSITIVITY!");
        ImGui::TextWrapped("THE HIGHER FPS YOU ARE, THE MORE STABLE IT GETS, HOWEVER 60 FPS IS ENOUGH FOR INFINITE DISTANCE");
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("This macro abuses the way leg raycast physics work to permanently keep wallhopping, without jumping you can walk up to a wall, maybe at a bit of an angle, and hold W and D or A to slowly walk across.");
    }

    if (selected_section == 11) {
        if (spamkey_instances.empty()) {
            ImGui::TextWrapped("No spamkey instances.");
        } else {
            SpamkeyInstance& inst = spamkey_instances[selected_spamkey_instance];
            std::string sid = "_sk" + std::to_string(selected_spamkey_instance);
            if (spamkey_instances.size() > 1) {
                ImGui::TextWrapped("%s of %d", BuildMultiInstanceName(11, selected_spamkey_instance + 1).c_str(), static_cast<int>(spamkey_instances.size()));
                ImGui::SameLine();
            }
            if (ImGui::Button(("+ Add New##AddSK" + sid).c_str())) spamkey_instances.emplace_back();
            RenderRemoveInstanceButton("RemSK" + sid, 11, spamkey_instances.size(), selected_spamkey_instance, request_remove_spamkey_instance_index);
            ImGui::Separator();
            ImGui::TextWrapped("Key to Press:");
            ImGui::SameLine();
            DrawKeyBindControl(("SpamKey" + sid).c_str(), inst.vk_spamkey, selected_section);
            ImGui::TextWrapped("Spam Delay (Milliseconds):");
            ImGui::SameLine();
            ImGui::SetNextItemWidth(120.0f);
            if (ImGui::InputText(("##SpamDelay" + sid).c_str(), inst.SpamDelay, sizeof(SpamkeyInstance::SpamDelay), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                try {
                    inst.spam_delay = std::stof(inst.SpamDelay);
                    inst.real_delay = static_cast<int>((inst.spam_delay + 0.5f) / 2);
                } catch (...) {}
            }
            ImGui::TextWrapped("I do not take any responsibility if you set the delay to 0ms");
            ImGui::Checkbox(("Switch from Toggle Key to Hold Key##" + sid).c_str(), &inst.isspamswitch);
            ImGui::Separator();
            ImGui::TextWrapped("Explanation:");
            ImGui::NewLine();
            ImGui::TextWrapped("This macro will spam the second key with a millisecond delay. This can be used as an autoclicker for any games you want, or a key-spam.");
        }
    }

    if (selected_section == 12) {
        ImGui::Checkbox("Switch Ledge Bounce to Left-Sided", &bouncesidetoggle);
        ImGui::Checkbox("Stay Horizontal After Bounce", &bouncerealignsideways);
        ImGui::Checkbox("Automatically Hold Movement Keys", &bounceautohold);
        ImGui::Separator();
        ImGui::TextWrapped("IMPORTANT:");
        ImGui::TextWrapped("PLEASE SET YOUR ROBLOX SENS AND CAM-FIX CORRECTLY SO IT CAN ACTUALLY DO THE PROPER TURNS! It works best at high FPS (120+).");
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("Walk up to a ledge with your camera sideways, about half of your left foot should be on the platform. The Macro will Automatically flick your camera 90 degrees, let you fall, and then flick back.");
    }

    if (selected_section == 13) {
        ImGui::TextWrapped("Bunnyhop Delay in Milliseconds (Default 10ms):");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(120.0f);
        if (ImGui::InputText("##BunnyhopDelay", BunnyHopDelayChar, sizeof(BunnyHopDelayChar), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            try { BunnyHopDelay = static_cast<int>(std::atof(BunnyHopDelayChar)); } catch (...) {}
        }
        ImGui::Checkbox("Enable Intelligent Auto-Toggle", &bunnyhopsmart);
        ImGui::Separator();
        ImGui::TextWrapped("If Intelligent Auto-Toggle is on, pressing your chat key will temporarily disable bhop until you press left click or enter to leave the chat.");
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("This Macro will automatically spam your key (typically space) with a specified delay whenever space is held down. This is created as a more functional Spamkey implementation specifically for Bhop/Bunnyhop.");
        ImGui::TextWrapped("This will only be restricted to Roblox when 'Disable outside of Roblox' is enabled.");
    }

    if (selected_section == 14) {
        ImGui::Checkbox("Attempt to HHJ for potentially slightly more height (EXPERIMENTAL, NOT RECOMMENDED)", &floorbouncehhj);
        if (ImGui::CollapsingHeader("Advanced Floor Bounce HHJ Options", showadvancedhhjbounce ? ImGuiTreeNodeFlags_DefaultOpen : 0)) {
            showadvancedhhjbounce = true;
            auto drawDelay = [](const char* id, const char* label, int& value, char* buffer, size_t bufferSize, int defaultValue) {
                ImGui::Indent();
                if (ImGui::Button(id, ImVec2(25, 0))) {
                    value = defaultValue;
                    std::snprintf(buffer, bufferSize, "%d", value);
                }
                ImGui::SameLine();
                ImGui::TextWrapped("%s", label);
                ImGui::SameLine();
                ImGui::SetNextItemWidth(50);
                if (ImGui::InputText((std::string("##") + id).c_str(), buffer, bufferSize, ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
                    try { value = std::stoi(buffer); } catch (...) {}
                }
                ImGui::Unindent();
            };
            ImGui::Spacing();
            drawDelay("R##FloorBounceDelay1", "Delay (Milliseconds) after unfreezing and before shiftlocking", FloorBounceDelay1, FloorBounceDelay1Char, sizeof(FloorBounceDelay1Char), 5);
            drawDelay("R##FloorBounceDelay2", "Delay (Milliseconds) before enabling helicoptering", FloorBounceDelay2, FloorBounceDelay2Char, sizeof(FloorBounceDelay2Char), 8);
            drawDelay("R##FloorBounceDelay3", "Time (Milliseconds) spent helicoptering", FloorBounceDelay3, FloorBounceDelay3Char, sizeof(FloorBounceDelay3Char), 100);
            ImGui::Spacing();
        }
        ImGui::TextWrapped("IMPORTANT:");
        ImGui::TextWrapped("This module only works at default Roblox gravity. Only works in R6 for now. FPS must be set to 160 or more to function properly. Higher is better.");
        ImGui::Separator();
        ImGui::TextWrapped("Explanation:");
        ImGui::NewLine();
        ImGui::TextWrapped("This Macro will automate an extremely precise glitch involving basically doing a lag high jump off the floor.");
    }

    if (selected_section == 15) {
        auto backend = smu::platform::GetNetworkLagBackend();
        const bool backendAvailable = backend && backend->isAvailable();
        if (!backendAvailable) {
            ImGui::BeginDisabled();
        }

        ImGui::Checkbox("Switch from Hold Key to Toggle Key", &islagswitchswitch);
        ImGui::Separator();
        ImVec2 tooltipCursorPos = ImGui::GetCursorScreenPos();
        ImGui::Checkbox("Prevent Roblox Disconnection (", &prevent_disconnect);
        ImGui::SameLine(0, 0);
        ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().accent_primary);
        ImGui::Text("?");
        ImGui::PopStyleColor();
        ImGui::SameLine(0, 0);
        ImGui::Text(")");
        ImGui::SetCursorScreenPos(tooltipCursorPos);
        ImGui::InvisibleButton("##tooltip", ImGui::CalcTextSize("Prevent Roblox Disconnection (?)      "));
        if (ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled)) {
            ImGui::SetTooltip("Prevents Timeout past the usual 10s threshold.\nExperimental, may break or kick. This is actively being worked on.");
        }
        ImGui::SetCursorScreenPos(ImVec2(tooltipCursorPos.x, tooltipCursorPos.y + 6));
        ImGui::NewLine();

        bool filterChanged = false;
        if (ImGui::Checkbox("Only Lag Switch Roblox", &lagswitchtargetroblox)) filterChanged = true;
        if (ImGui::IsItemHovered()) ImGui::SetTooltip("Filters only Roblox traffic.");
        if (ImGui::Checkbox("Also Block TCP (Websites)", &lagswitchusetcp)) filterChanged = true;
        ImGui::Separator();
        ImGui::Checkbox("Auto-Unlag (Anti-Kick) (Non-Roblox Games Only)", &lagswitch_autounblock);
        if (!lagswitch_autounblock) ImGui::BeginDisabled();
        ImGui::TextWrapped("Automatically stops lagging after this amount of seconds");
        ImGui::SetNextItemWidth(60.0f);
        ImGui::InputFloat("##LagFloat", &lagswitch_max_duration, 0.0f, 0.0f, "%.2f");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(300.0f);
        ImGui::SliderFloat("##LagSlider", &lagswitch_max_duration, 0.0f, 15.0f, "%.2f Seconds");
        char lagUnblockBuffer[16];
        std::snprintf(lagUnblockBuffer, sizeof(lagUnblockBuffer), "%d", lagswitch_unblock_ms);
        ImGui::SetNextItemWidth(50.0f);
        if (ImGui::InputText("Modify 50ms Default Unlag Time (MS)", lagUnblockBuffer, sizeof(lagUnblockBuffer), ImGuiInputTextFlags_CharsDecimal | ImGuiInputTextFlags_CharsNoBlank)) {
            lagswitch_unblock_ms = std::atoi(lagUnblockBuffer);
        }
        if (!lagswitch_autounblock) ImGui::EndDisabled();

        ImGui::Separator();
        if (ImGui::Checkbox("Block Outbound (Upload/Send) (Players won't be able to see you move)", &lagswitchoutbound)) filterChanged = true;
        if (ImGui::Checkbox("Block Inbound (Download/Recv) (You won't be able to see other players move)", &lagswitchinbound)) filterChanged = true;
        if (!lagswitchoutbound && !lagswitchinbound) {
            ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().warning_color);
            ImGui::TextWrapped("WARNING: Both Inbound and Outbound are unchecked.\nThe Lag Switch will not block any packets.");
            ImGui::PopStyleColor();
        }

        ImGui::Separator();
        if (ImGui::Checkbox("Fake Lag (Simulate High Ping)", &lagswitchlag)) filterChanged = true;
        if (!lagswitchlag) ImGui::BeginDisabled();
        ImGui::Indent();
        ImGui::Text("Delay Amount (Milliseconds):");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(150.0f);
        ImGui::InputInt("##LagDelayInput", &lagswitchlagdelay, 10, 100);
        if (ImGui::Checkbox("Lag Inbound (Recv)##FakeLag", &lagswitchlaginbound)) filterChanged = true;
        ImGui::SameLine();
        if (ImGui::Checkbox("Lag Outbound (Send)##FakeLag", &lagswitchlagoutbound)) filterChanged = true;
        if (lagswitchlag && !lagswitchlaginbound && !lagswitchlagoutbound) {
            ImGui::TextColored(GetCurrentTheme().warning_color, "Select at least one direction to lag!");
        }
        if (lagswitchlag && ((lagswitchoutbound && lagswitchlagoutbound) || (lagswitchinbound && lagswitchlaginbound))) {
            ImGui::PushStyleColor(ImGuiCol_Text, GetCurrentTheme().warning_color);
            ImGui::TextWrapped("WARNING: Both Blocking and Lagging are enabled for Inbound/Outbound packets.\nBlocking takes priority over lagging.");
            ImGui::PopStyleColor();
        }
        ImGui::Unindent();
        if (!lagswitchlag) ImGui::EndDisabled();
        ImGui::Separator();

        if (filterChanged && bWinDivertEnabled) RestartLagSwitchCapture();
        else SyncLagSwitchBackendConfig();

        ImGui::Checkbox("Show Lagswitch Status Overlay", &show_lag_overlay);
        if (!show_lag_overlay) ImGui::BeginDisabled();
        ImGui::Indent();
        ImGui::Checkbox("Hide When Not Actively Lagswitching", &overlay_hide_inactive);
        int screenW = static_cast<int>(ImGui::GetIO().DisplaySize.x);
        int screenH = static_cast<int>(ImGui::GetIO().DisplaySize.y);
        if (overlay_x == -1) overlay_x = static_cast<int>(screenW * 0.8f);
        ImGui::PushItemWidth(500);
        ImGui::SliderInt("Overlay X", &overlay_x, 0, screenW);
        ImGui::SliderInt("Overlay Y", &overlay_y, 0, screenH);
        ImGui::SliderInt("Text Size", &overlay_size, 10, 100);
        ImGui::Checkbox("Add Background", &overlay_use_bg);
        if (!overlay_use_bg) ImGui::BeginDisabled();
        float colors[3] = {overlay_bg_r, overlay_bg_g, overlay_bg_b};
        if (ImGui::ColorEdit3("Background Color", colors)) {
            overlay_bg_r = colors[0];
            overlay_bg_g = colors[1];
            overlay_bg_b = colors[2];
        }
        ImGui::PopItemWidth();
        if (!overlay_use_bg) ImGui::EndDisabled();
        ImGui::Unindent();
        if (!show_lag_overlay) ImGui::EndDisabled();

        ImGui::Separator();
        ImGui::NewLine();
        ImGui::Separator();
        if (ImGui::Button(bWinDivertEnabled ? "Disable WinDivert" : "Enable WinDivert")) {
            if (!bWinDivertEnabled) {
                std::string backendError;
                if (!InitializeLagSwitchBackend(&backendError) && !backendError.empty()) {
                    LogCritical(backendError);
                }
            } else {
                ShutdownLagSwitchBackend();
            }
        }
        ImGui::SameLine();
        ImGui::TextColored(bWinDivertEnabled ? GetCurrentTheme().success_color : GetCurrentTheme().error_color,
            bWinDivertEnabled ? "Driver Running" : "Driver Not Running");
        if (bWinDivertEnabled) {
            ImGui::SameLine();
            ImGui::Text(" |  Status: ");
            ImGui::SameLine();
            ImGui::TextColored(g_windivert_blocking ? GetCurrentTheme().error_color : GetCurrentTheme().success_color,
                g_windivert_blocking ? "LAGGING" : "Clear");
        }

        if (!backendAvailable) {
            ImGui::EndDisabled();
            ImGui::Separator();
            ImGui::TextColored(GetCurrentTheme().warning_color, "%s", backend ? backend->unsupportedReason().c_str() : "Network lagswitch backend is unavailable.");
        }
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
    ImVec2 pos(viewport->WorkPos.x + viewport->WorkSize.x - 24.0f, viewport->WorkPos.y + 24.0f);
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
    if (sections.empty()) {
        InitializeSections();
    }
    if (presskey_instances.empty()) presskey_instances.emplace_back();
    if (wallhop_instances.empty()) wallhop_instances.emplace_back();
    if (spamkey_instances.empty()) spamkey_instances.emplace_back();

    ApplySharedTheme();

    ImGuiIO& io = ImGui::GetIO();
    if (g_instance_remove_confirm_state.stage != 0) {
        g_instance_remove_confirm_state.timer -= io.DeltaTime;
        bool shouldReset = g_instance_remove_confirm_state.timer <= 0.0f;
        const int currentTarget = GetCurrentRemovalTargetForSection(g_instance_remove_confirm_state.section_index);
        if (currentTarget < 0 || currentTarget != g_instance_remove_confirm_state.target_instance_index) {
            shouldReset = true;
        }
        if (selected_section != g_instance_remove_confirm_state.section_index) {
            shouldReset = true;
        }
        if (shouldReset) {
            ResetInstanceRemoveConfirmState();
        }
    }

    ImVec2 displaySize = io.DisplaySize;
    ImGui::SetNextWindowSize(displaySize, ImGuiCond_Always);
    ImGui::SetNextWindowPos(ImVec2(0, 0), ImGuiCond_Always);

    ImGuiWindowFlags windowFlags = ImGuiWindowFlags_NoResize |
        ImGuiWindowFlags_NoMove |
        ImGuiWindowFlags_NoTitleBar |
        ImGuiWindowFlags_NoScrollbar |
        ImGuiWindowFlags_NoScrollWithMouse |
        ImGuiWindowFlags_NoBringToFrontOnFocus;

    ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, 0);
    ImGui::Begin("Main SMU Window", nullptr, windowFlags);
    ImGui::PopStyleVar();

    RenderPlatformCriticalNotifications();
    RenderPlatformWarningNotifications();
    RenderLinuxInputSetup(context);

    float settingsPanelHeight = 140.0f;
    ImGui::BeginChild("GlobalSettings", ImVec2(displaySize.x - 16, settingsPanelHeight), true);
    RenderGlobalSettings(context, displaySize);
    ImGui::EndChild();

    float leftPanelWidth = ImGui::GetWindowSize().x * 0.3f - 23;
    ImGui::BeginChild("LeftScrollSection", ImVec2(leftPanelWidth, ImGui::GetWindowSize().y - settingsPanelHeight - 20), true);
    RenderSectionSidebar(leftPanelWidth);
    ImGui::EndChild();

    ImGui::SameLine();
    ImVec2 rightSectionSize(displaySize.x - 23 - leftPanelWidth, displaySize.y - settingsPanelHeight - 20 - 30);
    ImGui::BeginChild("RightSection", rightSectionSize, true);
    RenderSelectedSection(context);
    ImGui::EndChild();

    ImVec2 childPos = ImGui::GetItemRectMin();
    ImVec2 childSize = ImGui::GetItemRectSize();
    ImDrawList* drawList = ImGui::GetWindowDrawList();
    ImU32 bgColor = ImGui::GetColorU32(ImGuiCol_ChildBg);
    ImU32 borderColor = ImGui::GetColorU32(ImGuiCol_Border);
    float rounding = ImGui::GetStyle().ChildRounding;

    ImVec2 windowSize = ImGui::GetWindowSize();
    ImGui::SetCursorPosY(windowSize.y - 30 - ImGui::GetStyle().WindowPadding.y);
    ImGui::SetCursorPosX(childPos.x);
    ImGui::PushStyleVar(ImGuiStyleVar_ChildRounding, 0.0f);

    if (ImGui::BeginChild("BottomControls", ImVec2(childSize.x - 1, 30), false, ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse)) {
        ImGui::SameLine(childSize.x - 616);
        ImGui::AlignTextToFramePadding();
        ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
        ImGui::Text("Always On-Top");
        ImGui::SameLine();
        ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
        if (ImGui::Checkbox("##OnTopToggle", &ontoptoggle)) {
            if (context.setAlwaysOnTop && !context.setAlwaysOnTop(ontoptoggle)) {
                LogWarning("Always-on-top could not be applied to the SDL window on this platform.");
            }
        }
        ImGui::SameLine();
        ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
        ImGui::Text("Opacity");
        ImGui::SameLine();
        ImGui::SetNextItemWidth(100.0f);
        ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
        if (ImGui::SliderFloat("##OpacitySlider", &windowOpacityPercent, 20.0f, 100.0f, "%.0f%%")) {
            if (context.setWindowOpacityPercent && !context.setWindowOpacityPercent(windowOpacityPercent)) {
                LogWarning("Window opacity could not be applied to the SDL window on this platform.");
            }
        }

        drawList->AddRectFilled(ImVec2(childPos.x, childPos.y + childSize.y - rounding), ImVec2(childPos.x + rounding, childPos.y + childSize.y), bgColor);
        drawList->AddRectFilled(ImVec2(childPos.x + childSize.x - rounding, childPos.y + childSize.y - rounding), ImVec2(childPos.x + childSize.x, childPos.y + childSize.y), bgColor);
        drawList->AddLine(ImVec2(childPos.x, childPos.y + childSize.y - rounding - 1), ImVec2(childPos.x, childPos.y + childSize.y + 29), borderColor);
        drawList->AddLine(ImVec2(childPos.x + childSize.x - 1, childPos.y + childSize.y - rounding - 1), ImVec2(childPos.x + childSize.x - 1, childPos.y + childSize.y + 29), borderColor);
        drawList->AddLine(ImVec2(childPos.x + 1, childPos.y + childSize.y - 1), ImVec2(childPos.x + childSize.x - 1, childPos.y + childSize.y - 1), bgColor, 1.0f);
        drawList->AddLine(ImVec2(childPos.x, childPos.y + childSize.y + 29), ImVec2(childPos.x + childSize.x, childPos.y + childSize.y + 29), borderColor, 1.0f);

        ImGui::SameLine();
        ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 3);
        RenderSharedProfileManager();
    }

    ImGui::PopStyleVar();
    ImGui::EndChild();
    ImGui::End();
}

} // namespace smu::app
