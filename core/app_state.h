#pragma once

#include <atomic>
#include <string>

namespace smu::core {

struct AppState {
    std::atomic<bool> running{true};
    std::atomic<bool> done{false};
    std::atomic<unsigned int> robloxFps{120};

    std::string localVersion = "3.2.1";

    int screenWidth = 1280;
    int screenHeight = 800;
    int rawWindowWidth = 0;
    int rawWindowHeight = 0;
    int windowPosX = 0;
    int windowPosY = 0;
    float windowOpacityPercent = 100.0f;

    bool processFound = false;
    bool userOutdated = false;
    bool alwaysOnTop = false;
    bool macroToggled = true;
    bool notBinding = true;
    bool shortDescriptions = false;
    bool isLinuxWine = false;
    bool showThemeMenu = false;
    bool showSettingsMenu = false;
    bool antiAfkToggle = true;
    bool camFixToggle = false;

    char settingsBuffer[256] = "RobloxPlayerBeta.exe";
    char robloxFpsBuffer[256] = "120";
    char robloxSensitivityBuffer[256] = "0.5";
};

AppState& GetAppState();
void ResetRuntimeAppFlags();

} // namespace smu::core
