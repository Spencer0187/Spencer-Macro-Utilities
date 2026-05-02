#pragma once

#include <cstdint>
#include <string>
#include <string_view>

namespace smu::core {

using KeyCode = std::uint32_t;

inline constexpr KeyCode SMU_VK_NONE = 0x00;
inline constexpr KeyCode SMU_VK_LBUTTON = 0x01;
inline constexpr KeyCode SMU_VK_RBUTTON = 0x02;
inline constexpr KeyCode SMU_VK_CANCEL = 0x03;
inline constexpr KeyCode SMU_VK_MBUTTON = 0x04;
inline constexpr KeyCode SMU_VK_XBUTTON1 = 0x05;
inline constexpr KeyCode SMU_VK_XBUTTON2 = 0x06;
inline constexpr KeyCode SMU_VK_BACK = 0x08;
inline constexpr KeyCode SMU_VK_TAB = 0x09;
inline constexpr KeyCode SMU_VK_RETURN = 0x0D;
inline constexpr KeyCode SMU_VK_SHIFT = 0x10;
inline constexpr KeyCode SMU_VK_CONTROL = 0x11;
inline constexpr KeyCode SMU_VK_MENU = 0x12;
inline constexpr KeyCode SMU_VK_PAUSE = 0x13;
inline constexpr KeyCode SMU_VK_CAPITAL = 0x14;
inline constexpr KeyCode SMU_VK_ESCAPE = 0x1B;
inline constexpr KeyCode SMU_VK_SPACE = 0x20;
inline constexpr KeyCode SMU_VK_PRIOR = 0x21;
inline constexpr KeyCode SMU_VK_NEXT = 0x22;
inline constexpr KeyCode SMU_VK_END = 0x23;
inline constexpr KeyCode SMU_VK_HOME = 0x24;
inline constexpr KeyCode SMU_VK_LEFT = 0x25;
inline constexpr KeyCode SMU_VK_UP = 0x26;
inline constexpr KeyCode SMU_VK_RIGHT = 0x27;
inline constexpr KeyCode SMU_VK_DOWN = 0x28;
inline constexpr KeyCode SMU_VK_SNAPSHOT = 0x2C;
inline constexpr KeyCode SMU_VK_INSERT = 0x2D;
inline constexpr KeyCode SMU_VK_DELETE = 0x2E;

inline constexpr KeyCode SMU_VK_0 = 0x30;
inline constexpr KeyCode SMU_VK_1 = 0x31;
inline constexpr KeyCode SMU_VK_2 = 0x32;
inline constexpr KeyCode SMU_VK_3 = 0x33;
inline constexpr KeyCode SMU_VK_4 = 0x34;
inline constexpr KeyCode SMU_VK_5 = 0x35;
inline constexpr KeyCode SMU_VK_6 = 0x36;
inline constexpr KeyCode SMU_VK_7 = 0x37;
inline constexpr KeyCode SMU_VK_8 = 0x38;
inline constexpr KeyCode SMU_VK_9 = 0x39;

inline constexpr KeyCode SMU_VK_A = 0x41;
inline constexpr KeyCode SMU_VK_B = 0x42;
inline constexpr KeyCode SMU_VK_C = 0x43;
inline constexpr KeyCode SMU_VK_D = 0x44;
inline constexpr KeyCode SMU_VK_E = 0x45;
inline constexpr KeyCode SMU_VK_F = 0x46;
inline constexpr KeyCode SMU_VK_G = 0x47;
inline constexpr KeyCode SMU_VK_H = 0x48;
inline constexpr KeyCode SMU_VK_I = 0x49;
inline constexpr KeyCode SMU_VK_J = 0x4A;
inline constexpr KeyCode SMU_VK_K = 0x4B;
inline constexpr KeyCode SMU_VK_L = 0x4C;
inline constexpr KeyCode SMU_VK_M = 0x4D;
inline constexpr KeyCode SMU_VK_N = 0x4E;
inline constexpr KeyCode SMU_VK_O = 0x4F;
inline constexpr KeyCode SMU_VK_P = 0x50;
inline constexpr KeyCode SMU_VK_Q = 0x51;
inline constexpr KeyCode SMU_VK_R = 0x52;
inline constexpr KeyCode SMU_VK_S = 0x53;
inline constexpr KeyCode SMU_VK_T = 0x54;
inline constexpr KeyCode SMU_VK_U = 0x55;
inline constexpr KeyCode SMU_VK_V = 0x56;
inline constexpr KeyCode SMU_VK_W = 0x57;
inline constexpr KeyCode SMU_VK_X = 0x58;
inline constexpr KeyCode SMU_VK_Y = 0x59;
inline constexpr KeyCode SMU_VK_Z = 0x5A;

inline constexpr KeyCode SMU_VK_LWIN = 0x5B;
inline constexpr KeyCode SMU_VK_RWIN = 0x5C;
inline constexpr KeyCode SMU_VK_APPS = 0x5D;
inline constexpr KeyCode SMU_VK_NUMPAD0 = 0x60;
inline constexpr KeyCode SMU_VK_F1 = 0x70;
inline constexpr KeyCode SMU_VK_F2 = 0x71;
inline constexpr KeyCode SMU_VK_F3 = 0x72;
inline constexpr KeyCode SMU_VK_F4 = 0x73;
inline constexpr KeyCode SMU_VK_F5 = 0x74;
inline constexpr KeyCode SMU_VK_F6 = 0x75;
inline constexpr KeyCode SMU_VK_F7 = 0x76;
inline constexpr KeyCode SMU_VK_F8 = 0x77;
inline constexpr KeyCode SMU_VK_F9 = 0x78;
inline constexpr KeyCode SMU_VK_F10 = 0x79;
inline constexpr KeyCode SMU_VK_F11 = 0x7A;
inline constexpr KeyCode SMU_VK_F12 = 0x7B;
inline constexpr KeyCode SMU_VK_NUMLOCK = 0x90;
inline constexpr KeyCode SMU_VK_SCROLL = 0x91;
inline constexpr KeyCode SMU_VK_LSHIFT = 0xA0;
inline constexpr KeyCode SMU_VK_RSHIFT = 0xA1;
inline constexpr KeyCode SMU_VK_LCONTROL = 0xA2;
inline constexpr KeyCode SMU_VK_RCONTROL = 0xA3;
inline constexpr KeyCode SMU_VK_LMENU = 0xA4;
inline constexpr KeyCode SMU_VK_RMENU = 0xA5;
inline constexpr KeyCode SMU_VK_OEM_1 = 0xBA;
inline constexpr KeyCode SMU_VK_OEM_PLUS = 0xBB;
inline constexpr KeyCode SMU_VK_OEM_COMMA = 0xBC;
inline constexpr KeyCode SMU_VK_OEM_MINUS = 0xBD;
inline constexpr KeyCode SMU_VK_OEM_PERIOD = 0xBE;
inline constexpr KeyCode SMU_VK_OEM_2 = 0xBF;
inline constexpr KeyCode SMU_VK_OEM_3 = 0xC0;
inline constexpr KeyCode SMU_VK_OEM_4 = 0xDB;
inline constexpr KeyCode SMU_VK_OEM_5 = 0xDC;
inline constexpr KeyCode SMU_VK_OEM_6 = 0xDD;
inline constexpr KeyCode SMU_VK_OEM_7 = 0xDE;

inline constexpr KeyCode SMU_VK_MOUSE_WHEEL_UP = 256;
inline constexpr KeyCode SMU_VK_MOUSE_WHEEL_DOWN = 257;

inline constexpr KeyCode HOTKEY_MASK_SHIFT = 0x10000;
inline constexpr KeyCode HOTKEY_MASK_CTRL = 0x20000;
inline constexpr KeyCode HOTKEY_MASK_ALT = 0x40000;
inline constexpr KeyCode HOTKEY_MASK_WIN = 0x80000;
inline constexpr KeyCode HOTKEY_KEY_MASK = 0xFFFF;

inline bool IsModifierKey(KeyCode key)
{
    return key == SMU_VK_SHIFT || key == SMU_VK_LSHIFT || key == SMU_VK_RSHIFT ||
           key == SMU_VK_CONTROL || key == SMU_VK_LCONTROL || key == SMU_VK_RCONTROL ||
           key == SMU_VK_MENU || key == SMU_VK_LMENU || key == SMU_VK_RMENU ||
           key == SMU_VK_LWIN || key == SMU_VK_RWIN;
}

inline std::string_view KeyCodeName(KeyCode key)
{
    switch (key) {
    case SMU_VK_LBUTTON: return "Mouse Left";
    case SMU_VK_RBUTTON: return "Mouse Right";
    case SMU_VK_MBUTTON: return "Mouse Middle";
    case SMU_VK_XBUTTON1: return "Mouse X1";
    case SMU_VK_XBUTTON2: return "Mouse X2";
    case SMU_VK_BACK: return "Backspace";
    case SMU_VK_TAB: return "Tab";
    case SMU_VK_RETURN: return "Enter";
    case SMU_VK_SHIFT: return "Shift";
    case SMU_VK_CONTROL: return "Ctrl";
    case SMU_VK_MENU: return "Alt";
    case SMU_VK_PAUSE: return "Pause";
    case SMU_VK_CAPITAL: return "Caps Lock";
    case SMU_VK_ESCAPE: return "Escape";
    case SMU_VK_SPACE: return "Space";
    case SMU_VK_PRIOR: return "Page Up";
    case SMU_VK_NEXT: return "Page Down";
    case SMU_VK_END: return "End";
    case SMU_VK_HOME: return "Home";
    case SMU_VK_LEFT: return "Left";
    case SMU_VK_UP: return "Up";
    case SMU_VK_RIGHT: return "Right";
    case SMU_VK_DOWN: return "Down";
    case SMU_VK_SNAPSHOT: return "Print Screen";
    case SMU_VK_INSERT: return "Insert";
    case SMU_VK_DELETE: return "Delete";
    case SMU_VK_LWIN: return "Left Super";
    case SMU_VK_RWIN: return "Right Super";
    case SMU_VK_APPS: return "Menu";
    case SMU_VK_NUMLOCK: return "Num Lock";
    case SMU_VK_SCROLL: return "Scroll Lock";
    case SMU_VK_LSHIFT: return "Left Shift";
    case SMU_VK_RSHIFT: return "Right Shift";
    case SMU_VK_LCONTROL: return "Left Ctrl";
    case SMU_VK_RCONTROL: return "Right Ctrl";
    case SMU_VK_LMENU: return "Left Alt";
    case SMU_VK_RMENU: return "Right Alt";
    case SMU_VK_OEM_1: return ";";
    case SMU_VK_OEM_PLUS: return "=";
    case SMU_VK_OEM_COMMA: return ",";
    case SMU_VK_OEM_MINUS: return "-";
    case SMU_VK_OEM_PERIOD: return ".";
    case SMU_VK_OEM_2: return "/";
    case SMU_VK_OEM_3: return "`";
    case SMU_VK_OEM_4: return "[";
    case SMU_VK_OEM_5: return "\\";
    case SMU_VK_OEM_6: return "]";
    case SMU_VK_OEM_7: return "'";
    case SMU_VK_MOUSE_WHEEL_UP: return "Mouse Wheel Up";
    case SMU_VK_MOUSE_WHEEL_DOWN: return "Mouse Wheel Down";
    default: return {};
    }
}

inline std::string FormatKeyCodeFallback(KeyCode key)
{
    if (key >= SMU_VK_A && key <= SMU_VK_Z) {
        return std::string(1, static_cast<char>(key));
    }
    if (key >= SMU_VK_0 && key <= SMU_VK_9) {
        return std::string(1, static_cast<char>(key));
    }
    if (key >= SMU_VK_F1 && key <= SMU_VK_F12) {
        return "F" + std::to_string(key - SMU_VK_F1 + 1);
    }
    if (key >= SMU_VK_NUMPAD0 && key <= SMU_VK_NUMPAD0 + 9) {
        return "Numpad " + std::to_string(key - SMU_VK_NUMPAD0);
    }

    const std::string_view name = KeyCodeName(key);
    if (!name.empty()) {
        return std::string(name);
    }

    return "0x" + std::to_string(key);
}

} // namespace smu::core
