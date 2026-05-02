#include "macro_state.h"

#include <array>

namespace smu::core {
namespace {

struct SectionDefaults {
    const char* title;
    const char* description;
    bool enabled;
    bool disableOutsideRoblox;
    KeyCode keybind;
};

constexpr std::array<SectionDefaults, kMacroSectionCount> kSectionDefaults = {{
    {"Freeze", "Automatically Tab Glitch With a Button", true, true, SMU_VK_MBUTTON},
    {"Item Desync", "Enable Item Collision (Hold Item Before Pressing)", true, true, SMU_VK_F5},
    {"Wall Helicopter High Jump", "Use COM Offset to Catapult Yourself Into The Air", true, false, SMU_VK_XBUTTON1},
    {"Speedglitch", "Use COM offset to Massively Increase Midair Speed", true, true, SMU_VK_X},
    {"Item Unequip COM Offset", "Automatically Do a /e dance2 Item COM Offset Where You Unequip the Item", true, true, SMU_VK_F8},
    {"Press a Button", "Whenever You Press Your Keybind, it Presses the Other Button for One Frame", false, true, SMU_VK_Z},
    {"Wallhop/Rotation", "Automatically Flick and Jump to easily Wallhop On All FPS", true, false, SMU_VK_XBUTTON2},
    {"Walless LHJ", "Lag High Jump Without a Wall by Offsetting COM Downwards or to the Right", true, false, SMU_VK_F6},
    {"Item Clip", "Clip through 2-3 Stud Walls Using Gears", true, true, SMU_VK_F3},
    {"Laugh Clip", "Automatically Perform a Laugh Clip", false, true, SMU_VK_F7},
    {"Wall-Walk", "Walk Across Wall Seams Without Jumping", false, true, SMU_VK_F1},
    {"Spam a Key", "Whenever You Press Your Keybind, it Spams the Other Button", false, false, SMU_VK_OEM_4},
    {"Ledge Bounce", "Briefly Falls off a Ledge to Then Bounce Off it While Falling", false, true, SMU_VK_C},
    {"Smart Bunnyhop", "Intelligently enables or disables Bunnyhop for any Key", false, true, SMU_VK_SPACE},
    {"Floor Bounce", "Jump much higher from flat ground", false, true, SMU_VK_F4},
    {"Lag Switch", "Modify or disable your internet connection temporarily", false, false, SMU_VK_OEM_PLUS},
}};

constexpr std::array<int, kMacroSectionCount> kDefaultSectionOrder = {
    0, 1, 2, 15, 3, 4, 5, 6, 13, 7, 8, 9, 10, 11, 12, 14
};

MacroState g_macroState;

} // namespace

MacroState& GetMacroState()
{
    return g_macroState;
}

void InitializeMacroSections(bool shortDescriptions)
{
    for (std::size_t index = 0; index < kSectionDefaults.size(); ++index) {
        const SectionDefaults& defaults = kSectionDefaults[index];
        MacroSection& section = g_macroState.sections[index];
        section.title = defaults.title;
        section.description = shortDescriptions ? "" : defaults.description;
        section.enabled = defaults.enabled;
        section.disableOutsideRoblox = defaults.disableOutsideRoblox;
        section.keybind = defaults.keybind;
    }

    g_macroState.sectionOrder = kDefaultSectionOrder;
}

void ResetMacroState()
{
    g_macroState = MacroState{};
    InitializeMacroSections(false);
}

} // namespace smu::core
