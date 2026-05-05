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
    {"Freeze", "Freeze the Roblox process.", true, true, SMU_VK_MBUTTON},
    {"Item Desync", "Desynchronize a held item from the client and server.", true, true, SMU_VK_F5},
    {"Wall Helicopter High Jump", "Use COM offset to launch yourself high into the air.", true, false, SMU_VK_XBUTTON1},
    {"Speedglitch", "Use COM offset to gain extreme midair speed.", true, true, SMU_VK_X},
    {"Item Unequip COM Offset", "Create a COM offset by using an emote and unequipping an item.", true, true, SMU_VK_F8},
    {"Press a Button", "Press another key for a short moment.", false, true, SMU_VK_Z},
    {"Wallhop/Rotation", "Flick and jump automatically to perform wallhops.", true, false, SMU_VK_XBUTTON2},
    {"Walless LHJ", "Perform a lag high jump without using a wall.", true, false, SMU_VK_F6},
    {"Item Clip", "Clip through thin walls using item equip timing.", true, true, SMU_VK_F3},
    {"Laugh Clip", "Perform a laugh clip automatically.", false, true, SMU_VK_F7},
    {"Wall-Walk", "Walk along wall seams without jumping.", false, true, SMU_VK_F1},
    {"Spam a Key", "Spam a selected key while the macro is active.", false, false, SMU_VK_OEM_4},
    {"Ledge Bounce", "Drop from a ledge and bounce back with timed camera movement.", false, true, SMU_VK_C},
    {"Smart Bunnyhop", "Automatically bunnyhop while avoiding chat/input conflicts.", false, true, SMU_VK_SPACE},
    {"Floor Bounce", "Perform a high jump from flat ground.", false, true, SMU_VK_F4},
    {"Lag Switch", "Temporarily block or delay Roblox network traffic.", false, false, SMU_VK_OEM_PLUS},
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
