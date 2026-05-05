#pragma once

namespace smu::app {

enum class MacroTutorialImage {
    LaughClip,
    GearClip,
    Wallhop,
    Wallwalk
};

bool LoadMacroTutorialTextures();
void UnloadMacroTutorialTextures();
void RenderCenteredMacroTutorialImage(MacroTutorialImage image, float displayWidth, float displayHeight);

} // namespace smu::app
