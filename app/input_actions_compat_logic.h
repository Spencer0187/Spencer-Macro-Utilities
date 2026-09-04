#pragma once

#include "../core/key_codes.h"

namespace smu::app::compat {

template <typename IsPressed>
bool IsModifierPressedWithPredicate(IsPressed&& isPressed, core::KeyCode key)
{
    switch (key) {
    case core::SMU_VK_SHIFT:
        return isPressed(core::SMU_VK_SHIFT) || isPressed(core::SMU_VK_LSHIFT) ||
            isPressed(core::SMU_VK_RSHIFT);
    case core::SMU_VK_CONTROL:
        return isPressed(core::SMU_VK_CONTROL) || isPressed(core::SMU_VK_LCONTROL) ||
            isPressed(core::SMU_VK_RCONTROL);
    case core::SMU_VK_MENU:
        return isPressed(core::SMU_VK_MENU) || isPressed(core::SMU_VK_LMENU) ||
            isPressed(core::SMU_VK_RMENU);
    case core::SMU_VK_LWIN:
        return isPressed(core::SMU_VK_LWIN) || isPressed(core::SMU_VK_RWIN);
    default:
        return isPressed(key);
    }
}

template <typename IsPressed>
bool IsHotkeyModifierStateExactWithPredicate(IsPressed&& isPressed, unsigned int combinedKey)
{
    const auto key = static_cast<core::KeyCode>(combinedKey & core::HOTKEY_KEY_MASK);
    const bool wantsWin = (combinedKey & core::HOTKEY_MASK_WIN) != 0 ||
        key == core::SMU_VK_LWIN || key == core::SMU_VK_RWIN;
    const bool wantsCtrl = (combinedKey & core::HOTKEY_MASK_CTRL) != 0 ||
        key == core::SMU_VK_CONTROL || key == core::SMU_VK_LCONTROL || key == core::SMU_VK_RCONTROL;
    const bool wantsAlt = (combinedKey & core::HOTKEY_MASK_ALT) != 0 ||
        key == core::SMU_VK_MENU || key == core::SMU_VK_LMENU || key == core::SMU_VK_RMENU;
    const bool wantsShift = (combinedKey & core::HOTKEY_MASK_SHIFT) != 0 ||
        key == core::SMU_VK_SHIFT || key == core::SMU_VK_LSHIFT || key == core::SMU_VK_RSHIFT;

    return IsModifierPressedWithPredicate(isPressed, core::SMU_VK_LWIN) == wantsWin &&
        IsModifierPressedWithPredicate(isPressed, core::SMU_VK_CONTROL) == wantsCtrl &&
        IsModifierPressedWithPredicate(isPressed, core::SMU_VK_MENU) == wantsAlt &&
        IsModifierPressedWithPredicate(isPressed, core::SMU_VK_SHIFT) == wantsShift;
}

template <typename IsPressed>
bool IsHotkeyPressedWithPredicate(IsPressed&& isPressed, unsigned int combinedKey, bool strict)
{
    const unsigned int key = combinedKey & core::HOTKEY_KEY_MASK;
    if (key == 0) {
        return false;
    }

    if ((combinedKey & core::HOTKEY_MASK_WIN) &&
        !IsModifierPressedWithPredicate(isPressed, core::SMU_VK_LWIN)) {
        return false;
    }
    if ((combinedKey & core::HOTKEY_MASK_CTRL) &&
        !IsModifierPressedWithPredicate(isPressed, core::SMU_VK_CONTROL)) {
        return false;
    }
    if ((combinedKey & core::HOTKEY_MASK_ALT) &&
        !IsModifierPressedWithPredicate(isPressed, core::SMU_VK_MENU)) {
        return false;
    }
    if ((combinedKey & core::HOTKEY_MASK_SHIFT) &&
        !IsModifierPressedWithPredicate(isPressed, core::SMU_VK_SHIFT)) {
        return false;
    }
    if (strict && !IsHotkeyModifierStateExactWithPredicate(isPressed, combinedKey)) {
        return false;
    }

    return IsModifierPressedWithPredicate(isPressed, static_cast<core::KeyCode>(key));
}

} // namespace smu::app::compat
