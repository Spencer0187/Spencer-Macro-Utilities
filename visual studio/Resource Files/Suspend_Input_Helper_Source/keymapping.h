#ifndef KEY_MAPPING_H
#define KEY_MAPPING_H

#include <cstdint>
#include <map>

// Windows Virtual-Key Codes can be found here:
// https://docs.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes

// Unassigned key code
const uint8_t VK_UNASSIGNED = 0x0F;

// Dummy macOS key codes for mouse buttons
const uint16_t MACOS_MOUSE_LEFT = 0xFF;
const uint16_t MACOS_MOUSE_RIGHT = 0xFE;
const uint16_t MACOS_MOUSE_MIDDLE = 0xFD;
const uint16_t MACOS_MOUSE_X1 = 0xFC;
const uint16_t MACOS_MOUSE_X2 = 0xFB;
const uint16_t MACOS_UNASSIGNED = 0xFFFF;

// Dummy Evdev key codes for unassigned Windows VKs where no direct Evdev equivalent exists
const uint16_t EVDEV_UNASSIGNED = 0x00; // Using 0x00 as a generic unassigned for Evdev

/**
 * @brief Maps a Linux Evdev key code to a Windows virtual key code.
 *
 * This function takes a Linux input event code (evdev) and returns the
 * corresponding Windows virtual-key code. Mappings are provided for keyboard
 * and mouse buttons. If a direct mapping is not available, VK_UNASSIGNED (0x0F)
 * is returned.
 *
 * @param evdev_key_code The Linux Evdev key code.
 * @return The corresponding Windows virtual key code, or VK_UNASSIGNED.
 */
uint8_t evdev_to_win_vkey(uint16_t evdev_key_code) {
    switch (evdev_key_code) {
        // Mouse Buttons
        case 0x110: return 0x01; // BTN_LEFT -> VK_LBUTTON
        case 0x111: return 0x02; // BTN_RIGHT -> VK_RBUTTON
        case 0x112: return 0x04; // BTN_MIDDLE -> VK_MBUTTON
        case 0x113: return 0x05; // BTN_SIDE -> VK_XBUTTON1
        case 0x114: return 0x06; // BTN_EXTRA -> VK_XBUTTON2

        // Keyboard Keys
        case 1:   return 0x1B; // KEY_ESC -> VK_ESCAPE
        case 2:   return 0x31; // KEY_1 -> '1'
        case 3:   return 0x32; // KEY_2 -> '2'
        case 4:   return 0x33; // KEY_3 -> '3'
        case 5:   return 0x34; // KEY_4 -> '4'
        case 6:   return 0x35; // KEY_5 -> '5'
        case 7:   return 0x36; // KEY_6 -> '6'
        case 8:   return 0x37; // KEY_7 -> '7'
        case 9:   return 0x38; // KEY_8 -> '8'
        case 10:  return 0x39; // KEY_9 -> '9'
        case 11:  return 0x30; // KEY_0 -> '0'
        case 12:  return 0xBD; // KEY_MINUS -> VK_OEM_MINUS
        case 13:  return 0xBB; // KEY_EQUAL -> VK_OEM_PLUS
        case 14:  return 0x08; // KEY_BACKSPACE -> VK_BACK
        case 15:  return 0x09; // KEY_TAB -> VK_TAB
        case 16:  return 0x51; // KEY_Q -> 'Q'
        case 17:  return 0x57; // KEY_W -> 'W'
        case 18:  return 0x45; // KEY_E -> 'E'
        case 19:  return 0x52; // KEY_R -> 'R'
        case 20:  return 0x54; // KEY_T -> 'T'
        case 21:  return 0x59; // KEY_Y -> 'Y'
        case 22:  return 0x55; // KEY_U -> 'U'
        case 23:  return 0x49; // KEY_I -> 'I'
        case 24:  return 0x4F; // KEY_O -> 'O'
        case 25:  return 0x50; // KEY_P -> 'P'
        case 26:  return 0xDB; // KEY_LEFTBRACE -> VK_OEM_4
        case 27:  return 0xDD; // KEY_RIGHTBRACE -> VK_OEM_6
        case 28:  return 0x0D; // KEY_ENTER -> VK_RETURN
        case 29:  return 0x11; // KEY_LEFTCTRL -> VK_CONTROL (or VK_LCONTROL)
        case 30:  return 0x41; // KEY_A -> 'A'
        case 31:  return 0x53; // KEY_S -> 'S'
        case 32:  return 0x44; // KEY_D -> 'D'
        case 33:  return 0x46; // KEY_F -> 'F'
        case 34:  return 0x47; // KEY_G -> 'G'
        case 35:  return 0x48; // KEY_H -> 'H'
        case 36:  return 0x4A; // KEY_J -> 'J'
        case 37:  return 0x4B; // KEY_K -> 'K'
        case 38:  return 0x4C; // KEY_L -> 'L'
        case 39:  return 0xBA; // KEY_SEMICOLON -> VK_OEM_1
        case 40:  return 0xDE; // KEY_APOSTROPHE -> VK_OEM_7
        case 41:  return 0xC0; // KEY_GRAVE -> VK_OEM_3
        case 42:  return 0x10; // KEY_LEFTSHIFT -> VK_SHIFT (or VK_LSHIFT)
        case 43:  return 0xDC; // KEY_BACKSLASH -> VK_OEM_5
        case 44:  return 0x5A; // KEY_Z -> 'Z'
        case 45:  return 0x58; // KEY_X -> 'X'
        case 46:  return 0x43; // KEY_C -> 'C'
        case 47:  return 0x56; // KEY_V -> 'V'
        case 48:  return 0x42; // KEY_B -> 'B'
        case 49:  return 0x4E; // KEY_N -> 'N'
        case 50:  return 0x4D; // KEY_M -> 'M'
        case 51:  return 0xBC; // KEY_COMMA -> VK_OEM_COMMA
        case 52:  return 0xBE; // KEY_DOT -> VK_OEM_PERIOD
        case 53:  return 0xBF; // KEY_SLASH -> VK_OEM_2
        case 54:  return 0x10; // KEY_RIGHTSHIFT -> VK_SHIFT (or VK_RSHIFT)
        case 55:  return 0x6A; // KEY_KPASTERISK -> VK_MULTIPLY
        case 56:  return 0x12; // KEY_LEFTALT -> VK_MENU (or VK_LMENU)
        case 57:  return 0x20; // KEY_SPACE -> VK_SPACE
        case 58:  return 0x14; // KEY_CAPSLOCK -> VK_CAPITAL
        case 59:  return 0x70; // KEY_F1 -> VK_F1
        case 60:  return 0x71; // KEY_F2 -> VK_F2
        case 61:  return 0x72; // KEY_F3 -> VK_F3
        case 62:  return 0x73; // KEY_F4 -> VK_F4
        case 63:  return 0x74; // KEY_F5 -> VK_F5
        case 64:  return 0x75; // KEY_F6 -> VK_F6
        case 65:  return 0x76; // KEY_F7 -> VK_F7
        case 66:  return 0x77; // KEY_F8 -> VK_F8
        case 67:  return 0x78; // KEY_F9 -> VK_F9
        case 68:  return 0x79; // KEY_F10 -> VK_F10
        case 69:  return 0x90; // KEY_NUMLOCK -> VK_NUMLOCK
        case 70:  return 0x91; // KEY_SCROLLLOCK -> VK_SCROLL
        case 71:  return 0x67; // KEY_KP7 -> VK_NUMPAD7
        case 72:  return 0x68; // KEY_KP8 -> VK_NUMPAD8
        case 73:  return 0x69; // KEY_KP9 -> VK_NUMPAD9
        case 74:  return 0x6D; // KEY_KPMINUS -> VK_SUBTRACT
        case 75:  return 0x64; // KEY_KP4 -> VK_NUMPAD4
        case 76:  return 0x65; // KEY_KP5 -> VK_NUMPAD5
        case 77:  return 0x66; // KEY_KP6 -> VK_NUMPAD6
        case 78:  return 0x6B; // KEY_KPPLUS -> VK_ADD
        case 79:  return 0x61; // KEY_KP1 -> VK_NUMPAD1
        case 80:  return 0x62; // KEY_KP2 -> VK_NUMPAD2
        case 81:  return 0x63; // KEY_KP3 -> VK_NUMPAD3
        case 82:  return 0x60; // KEY_KP0 -> VK_NUMPAD0
        case 83:  return 0x6E; // KEY_KPDOT -> VK_DECIMAL
        case 87:  return 0x7A; // KEY_F11 -> VK_F11
        case 88:  return 0x7B; // KEY_F12 -> VK_F12
        case 96:  return 0x0D; // KEY_KPENTER -> VK_RETURN
        case 97:  return 0x11; // KEY_RIGHTCTRL -> VK_CONTROL (or VK_RCONTROL)
        case 98:  return 0x6F; // KEY_KPSLASH -> VK_DIVIDE
        case 99:  return 0x2C; // KEY_SYSRQ -> VK_SNAPSHOT
        case 100: return 0x12; // KEY_RIGHTALT -> VK_MENU (or VK_RMENU)
        case 102: return 0x24; // KEY_HOME -> VK_HOME
        case 103: return 0x26; // KEY_UP -> VK_UP
        case 104: return 0x21; // KEY_PAGEUP -> VK_PRIOR
        case 105: return 0x25; // KEY_LEFT -> VK_LEFT
        case 106: return 0x27; // KEY_RIGHT -> VK_RIGHT
        case 107: return 0x23; // KEY_END -> VK_END
        case 108: return 0x28; // KEY_DOWN -> VK_DOWN
        case 109: return 0x22; // KEY_PAGEDOWN -> VK_NEXT
        case 110: return 0x2D; // KEY_INSERT -> VK_INSERT
        case 111: return 0x2E; // KEY_DELETE -> VK_DELETE
        case 119: return 0x13; // KEY_PAUSE -> VK_PAUSE
        case 125: return 0x5B; // KEY_LEFTMETA -> VK_LWIN
        case 126: return 0x5C; // KEY_RIGHTMETA -> VK_RWIN
        case 127: return 0x5D; // KEY_COMPOSE -> VK_APPS
        case 183: return 0x7C; // KEY_F13 -> VK_F13
        case 184: return 0x7D; // KEY_F14 -> VK_F14
        case 185: return 0x7E; // KEY_F15 -> VK_F15
        case 186: return 0x7F; // KEY_F16 -> VK_F16
        case 187: return 0x80; // KEY_F17 -> VK_F17
        case 188: return 0x81; // KEY_F18 -> VK_F18
        case 189: return 0x82; // KEY_F19 -> VK_F19
        case 190: return 0x83; // KEY_F20 -> VK_F20
        case 191: return 0x84; // KEY_F21 -> VK_F21
        case 192: return 0x85; // KEY_F22 -> VK_F22
        case 193: return 0x86; // KEY_F23 -> VK_F23
        case 194: return 0x87; // KEY_F24 -> VK_F24

        default:  return VK_UNASSIGNED;
    }
}

/**
 * @brief Maps a macOS Quartz key code to a Windows virtual key code.
 *
 * This function takes a macOS Quartz key code and returns the
 * corresponding Windows virtual-key code. If a direct mapping is not
 * available, VK_UNASSIGNED (0x0F) is returned.
 *
 * @param macos_key_code The macOS Quartz key code.
 * @return The corresponding Windows virtual key code, or VK_UNASSIGNED.
 */
uint8_t macos_to_win_vkey(uint16_t macos_key_code) {
    switch (macos_key_code) {
        case 0x00: return 0x41; // kVK_ANSI_A -> 'A'
        case 0x01: return 0x53; // kVK_ANSI_S -> 'S'
        case 0x02: return 0x44; // kVK_ANSI_D -> 'D'
        case 0x03: return 0x46; // kVK_ANSI_F -> 'F'
        case 0x04: return 0x48; // kVK_ANSI_H -> 'H'
        case 0x05: return 0x47; // kVK_ANSI_G -> 'G'
        case 0x06: return 0x5A; // kVK_ANSI_Z -> 'Z'
        case 0x07: return 0x58; // kVK_ANSI_X -> 'X'
        case 0x08: return 0x43; // kVK_ANSI_C -> 'C'
        case 0x09: return 0x56; // kVK_ANSI_V -> 'V'
        case 0x0B: return 0x42; // kVK_ANSI_B -> 'B'
        case 0x0C: return 0x51; // kVK_ANSI_Q -> 'Q'
        case 0x0D: return 0x57; // kVK_ANSI_W -> 'W'
        case 0x0E: return 0x45; // kVK_ANSI_E -> 'E'
        case 0x0F: return 0x52; // kVK_ANSI_R -> 'R'
        case 0x10: return 0x59; // kVK_ANSI_Y -> 'Y'
        case 0x11: return 0x54; // kVK_ANSI_T -> 'T'
        case 0x12: return 0x31; // kVK_ANSI_1 -> '1'
        case 0x13: return 0x32; // kVK_ANSI_2 -> '2'
        case 0x14: return 0x33; // kVK_ANSI_3 -> '3'
        case 0x15: return 0x34; // kVK_ANSI_4 -> '4'
        case 0x16: return 0x36; // kVK_ANSI_6 -> '6'
        case 0x17: return 0x35; // kVK_ANSI_5 -> '5'
        case 0x18: return 0xBB; // kVK_ANSI_Equal -> VK_OEM_PLUS
        case 0x19: return 0x39; // kVK_ANSI_9 -> '9'
        case 0x1A: return 0x37; // kVK_ANSI_7 -> '7'
        case 0x1B: return 0xBD; // kVK_ANSI_Minus -> VK_OEM_MINUS
        case 0x1C: return 0x38; // kVK_ANSI_8 -> '8'
        case 0x1D: return 0x30; // kVK_ANSI_0 -> '0'
        case 0x1E: return 0xDD; // kVK_ANSI_RightBracket -> VK_OEM_6
        case 0x1F: return 0x4F; // kVK_ANSI_O -> 'O'
        case 0x20: return 0x55; // kVK_ANSI_U -> 'U'
        case 0x21: return 0xDB; // kVK_ANSI_LeftBracket -> VK_OEM_4
        case 0x22: return 0x49; // kVK_ANSI_I -> 'I'
        case 0x23: return 0x50; // kVK_ANSI_P -> 'P'
        case 0x24: return 0x0D; // kVK_Return -> VK_RETURN
        case 0x25: return 0x4C; // kVK_ANSI_L -> 'L'
        case 0x26: return 0x4A; // kVK_ANSI_J -> 'J'
        case 0x27: return 0xDE; // kVK_ANSI_Quote -> VK_OEM_7
        case 0x28: return 0x4B; // kVK_ANSI_K -> 'K'
        case 0x29: return 0xBA; // kVK_ANSI_Semicolon -> VK_OEM_1
        case 0x2A: return 0xDC; // kVK_ANSI_Backslash -> VK_OEM_5
        case 0x2B: return 0xBC; // kVK_ANSI_Comma -> VK_OEM_COMMA
        case 0x2C: return 0xBF; // kVK_ANSI_Slash -> VK_OEM_2
        case 0x2D: return 0x4E; // kVK_ANSI_N -> 'N'
        case 0x2E: return 0x4D; // kVK_ANSI_M -> 'M'
        case 0x2F: return 0xBE; // kVK_ANSI_Period -> VK_OEM_PERIOD
        case 0x30: return 0x09; // kVK_Tab -> VK_TAB
        case 0x31: return 0x20; // kVK_Space -> VK_SPACE
        case 0x32: return 0xC0; // kVK_ANSI_Grave -> VK_OEM_3
        case 0x33: return 0x08; // kVK_Delete -> VK_BACK
        case 0x35: return 0x1B; // kVK_Escape -> VK_ESCAPE
        case 0x37: return 0x5B; // kVK_Command -> VK_LWIN
        case 0x38: return 0x10; // kVK_Shift -> VK_SHIFT
        case 0x39: return 0x14; // kVK_CapsLock -> VK_CAPITAL
        case 0x3A: return 0x12; // kVK_Option -> VK_MENU
        case 0x3B: return 0x11; // kVK_Control -> VK_CONTROL
        case 0x3C: return 0x10; // kVK_RightShift -> VK_RSHIFT
        case 0x3D: return 0x12; // kVK_RightOption -> VK_RMENU
        case 0x3E: return 0x11; // kVK_RightControl -> VK_RCONTROL
        case 0x41: return 0x6E; // kVK_ANSI_KeypadDecimal -> VK_DECIMAL
        case 0x43: return 0x6A; // kVK_ANSI_KeypadMultiply -> VK_MULTIPLY
        case 0x45: return 0x6B; // kVK_ANSI_KeypadPlus -> VK_ADD
        case 0x4B: return 0x6F; // kVK_ANSI_KeypadDivide -> VK_DIVIDE
        case 0x4C: return 0x0D; // kVK_ANSI_KeypadEnter -> VK_RETURN
        case 0x4E: return 0x6D; // kVK_ANSI_KeypadMinus -> VK_SUBTRACT
        case 0x52: return 0x60; // kVK_ANSI_Keypad0 -> VK_NUMPAD0
        case 0x53: return 0x61; // kVK_ANSI_Keypad1 -> VK_NUMPAD1
        case 0x54: return 0x62; // kVK_ANSI_Keypad2 -> VK_NUMPAD2
        case 0x55: return 0x63; // kVK_ANSI_Keypad3 -> VK_NUMPAD3
        case 0x56: return 0x64; // kVK_ANSI_Keypad4 -> VK_NUMPAD4
        case 0x57: return 0x65; // kVK_ANSI_Keypad5 -> VK_NUMPAD5
        case 0x58: return 0x66; // kVK_ANSI_Keypad6 -> VK_NUMPAD6
        case 0x59: return 0x67; // kVK_ANSI_Keypad7 -> VK_NUMPAD7
        case 0x5B: return 0x68; // kVK_ANSI_Keypad8 -> VK_NUMPAD8
        case 0x5C: return 0x69; // kVK_ANSI_Keypad9 -> VK_NUMPAD9
        case 0x60: return 0x74; // kVK_F5 -> VK_F5
        case 0x61: return 0x75; // kVK_F6 -> VK_F6
        case 0x62: return 0x76; // kVK_F7 -> VK_F7
        case 0x63: return 0x72; // kVK_F3 -> VK_F3
        case 0x64: return 0x77; // kVK_F8 -> VK_F8
        case 0x65: return 0x78; // kVK_F9 -> VK_F9
        case 0x67: return 0x7A; // kVK_F11 -> VK_F11
        case 0x69: return 0x7C; // kVK_F13 -> VK_F13
        case 0x6B: return 0x7D; // kVK_F14 -> VK_F14
        case 0x6D: return 0x79; // kVK_F10 -> VK_F10
        case 0x6F: return 0x7B; // kVK_F12 -> VK_F12
        case 0x71: return 0x7E; // kVK_F15 -> VK_F15
        case 0x72: return 0x2D; // kVK_Help -> VK_INSERT
        case 0x73: return 0x24; // kVK_Home -> VK_HOME
        case 0x74: return 0x21; // kVK_PageUp -> VK_PRIOR
        case 0x75: return 0x2E; // kVK_ForwardDelete -> VK_DELETE
        case 0x76: return 0x73; // kVK_F4 -> VK_F4
        case 0x77: return 0x23; // kVK_End -> VK_END
        case 0x78: return 0x71; // kVK_F2 -> VK_F2
        case 0x79: return 0x22; // kVK_PageDown -> VK_NEXT
        case 0x7A: return 0x70; // kVK_F1 -> VK_F1
        case 0x7B: return 0x25; // kVK_LeftArrow -> VK_LEFT
        case 0x7C: return 0x27; // kVK_RightArrow -> VK_RIGHT
        case 0x7D: return 0x28; // kVK_DownArrow -> VK_DOWN
        case 0x7E: return 0x26; // kVK_UpArrow -> VK_UP

        default:   return VK_UNASSIGNED;
    }
}

/**
 * @brief Maps a Windows virtual key code to a macOS Quartz key code.
 *
 * This function takes a Windows virtual key code and returns the
 * corresponding macOS Quartz key code. For mouse buttons, it returns
 * custom dummy key codes (MACOS_MOUSE_LEFT, MACOS_MOUSE_RIGHT, MACOS_MOUSE_MIDDLE)
 * that your macOS application can handle.
 * If a direct mapping is not available, VK_UNASSIGNED (0x0F) is returned.
 *
 * @param win_vkey_code The Windows virtual key code.
 * @return The corresponding macOS Quartz key code, or VK_UNASSIGNED.
 */
uint16_t win_vkey_to_macos_key(uint8_t win_vkey_code) {
    switch (win_vkey_code) {
        // Mouse Buttons (using custom dummy macOS key codes)
        case 0x01: return MACOS_MOUSE_LEFT;   // VK_LBUTTON
        case 0x02: return MACOS_MOUSE_RIGHT;  // VK_RBUTTON
        case 0x04: return MACOS_MOUSE_MIDDLE; // VK_MBUTTON
        case 0x05: return MACOS_MOUSE_X1; // VK_XBUTTON1
        case 0x06: return MACOS_MOUSE_X2; // VK_XBUTTON1

        // No direct macOS virtual key codes for XBUTTON1/2, so they map to UNASSIGNED here.
        // If you need them, you'd define more dummy codes.

        // Keyboard Keys (inverse of macos_to_win_vkey)
        case 0x41: return 0x00; // 'A' -> kVK_ANSI_A
        case 0x53: return 0x01; // 'S' -> kVK_ANSI_S
        case 0x44: return 0x02; // 'D' -> kVK_ANSI_D
        case 0x46: return 0x03; // 'F' -> kVK_ANSI_F
        case 0x48: return 0x04; // 'H' -> kVK_ANSI_H
        case 0x47: return 0x05; // 'G' -> kVK_ANSI_G
        case 0x5A: return 0x06; // 'Z' -> kVK_ANSI_Z
        case 0x58: return 0x07; // 'X' -> kVK_ANSI_X
        case 0x43: return 0x08; // 'C' -> kVK_ANSI_C
        case 0x56: return 0x09; // 'V' -> kVK_ANSI_V
        case 0x42: return 0x0B; // 'B' -> kVK_ANSI_B
        case 0x51: return 0x0C; // 'Q' -> kVK_ANSI_Q
        case 0x57: return 0x0D; // 'W' -> kVK_ANSI_W
        case 0x45: return 0x0E; // 'E' -> kVK_ANSI_E
        case 0x52: return 0x0F; // 'R' -> kVK_ANSI_R
        case 0x59: return 0x10; // 'Y' -> kVK_ANSI_Y
        case 0x54: return 0x11; // 'T' -> kVK_ANSI_T
        case 0x31: return 0x12; // '1' -> kVK_ANSI_1
        case 0x32: return 0x13; // '2' -> kVK_ANSI_2
        case 0x33: return 0x14; // '3' -> kVK_ANSI_3
        case 0x34: return 0x15; // '4' -> kVK_ANSI_4
        case 0x36: return 0x16; // '6' -> kVK_ANSI_6
        case 0x35: return 0x17; // '5' -> kVK_ANSI_5
        case 0xBB: return 0x18; // VK_OEM_PLUS -> kVK_ANSI_Equal
        case 0x39: return 0x19; // '9' -> kVK_ANSI_9
        case 0x37: return 0x1A; // '7' -> kVK_ANSI_7
        case 0xBD: return 0x1B; // VK_OEM_MINUS -> kVK_ANSI_Minus
        case 0x38: return 0x1C; // '8' -> kVK_ANSI_8
        case 0x30: return 0x1D; // '0' -> kVK_ANSI_0
        case 0xDD: return 0x1E; // VK_OEM_6 -> kVK_ANSI_RightBracket
        case 0x4F: return 0x1F; // 'O' -> kVK_ANSI_O
        case 0x55: return 0x20; // 'U' -> kVK_ANSI_U
        case 0xDB: return 0x21; // VK_OEM_4 -> kVK_ANSI_LeftBracket
        case 0x49: return 0x22; // 'I' -> kVK_ANSI_I
        case 0x50: return 0x23; // 'P' -> kVK_ANSI_P
        case 0x0D: return 0x24; // VK_RETURN -> kVK_Return
        case 0x4C: return 0x25; // 'L' -> kVK_ANSI_L
        case 0x4A: return 0x26; // 'J' -> kVK_ANSI_J
        case 0xDE: return 0x27; // VK_OEM_7 -> kVK_ANSI_Quote
        case 0x4B: return 0x28; // 'K' -> kVK_ANSI_K
        case 0xBA: return 0x29; // VK_OEM_1 -> kVK_ANSI_Semicolon
        case 0xDC: return 0x2A; // VK_OEM_5 -> kVK_ANSI_Backslash
        case 0xBC: return 0x2B; // VK_OEM_COMMA -> kVK_ANSI_Comma
        case 0xBF: return 0x2C; // VK_OEM_2 -> kVK_ANSI_Slash
        case 0x4E: return 0x2D; // 'N' -> kVK_ANSI_N
        case 0x4D: return 0x2E; // 'M' -> kVK_ANSI_M
        case 0xBE: return 0x2F; // VK_OEM_PERIOD -> kVK_ANSI_Period
        case 0x09: return 0x30; // VK_TAB -> kVK_Tab
        case 0x20: return 0x31; // VK_SPACE -> kVK_Space
        case 0xC0: return 0x32; // VK_OEM_3 -> kVK_ANSI_Grave
        case 0x08: return 0x33; // VK_BACK -> kVK_Delete
        case 0x1B: return 0x35; // VK_ESCAPE -> kVK_Escape
        case 0x5B: return 0x37; // VK_LWIN -> kVK_Command
        case 0x10: return 0x38; // VK_SHIFT (L or R) -> kVK_Shift (or kVK_RightShift if differentiating)
        case 0x14: return 0x39; // VK_CAPITAL -> kVK_CapsLock
        case 0x12: return 0x3A; // VK_MENU (L or R) -> kVK_Option (or kVK_RightOption if differentiating)
        case 0x11: return 0x3B; // VK_CONTROL (L or R) -> kVK_Control (or kVK_RightControl if differentiating)
        // case 0x10 (VK_RSHIFT) maps to 0x3C, but 0x10 (VK_LSHIFT) already maps to 0x38.
        // Prioritizing the generic SHIFT, you might need more complex logic if L/R SHIFT are distinct in your usage.
        // For simplicity, using the first mapping for shared VKs.
        // case 0x12 (VK_RMENU) maps to 0x3D
        // case 0x11 (VK_RCONTROL) maps to 0x3E
        case 0x6E: return 0x41; // VK_DECIMAL -> kVK_ANSI_KeypadDecimal
        case 0x6A: return 0x43; // VK_MULTIPLY -> kVK_ANSI_KeypadMultiply
        case 0x6B: return 0x45; // VK_ADD -> kVK_ANSI_KeypadPlus
        case 0x6F: return 0x4B; // VK_DIVIDE -> kVK_ANSI_KeypadDivide
        case 0x6D: return 0x4E; // VK_SUBTRACT -> kVK_ANSI_KeypadMinus
        case 0x60: return 0x52; // VK_NUMPAD0 -> kVK_ANSI_Keypad0
        case 0x61: return 0x53; // VK_NUMPAD1 -> kVK_ANSI_Keypad1
        case 0x62: return 0x54; // VK_NUMPAD2 -> kVK_ANSI_Keypad2
        case 0x63: return 0x55; // VK_NUMPAD3 -> kVK_ANSI_Keypad3
        case 0x64: return 0x56; // VK_NUMPAD4 -> kVK_ANSI_Keypad4
        case 0x65: return 0x57; // VK_NUMPAD5 -> kVK_ANSI_Keypad5
        case 0x66: return 0x58; // VK_NUMPAD6 -> kVK_ANSI_Keypad6
        case 0x67: return 0x59; // VK_NUMPAD7 -> kVK_ANSI_Keypad7
        case 0x68: return 0x5B; // VK_NUMPAD8 -> kVK_ANSI_Keypad8
        case 0x69: return 0x5C; // VK_NUMPAD9 -> kVK_ANSI_Keypad9
        case 0x74: return 0x60; // VK_F5 -> kVK_F5
        case 0x75: return 0x61; // VK_F6 -> kVK_F6
        case 0x76: return 0x62; // VK_F7 -> kVK_F7
        case 0x72: return 0x63; // VK_F3 -> kVK_F3
        case 0x77: return 0x64; // VK_F8 -> kVK_F8
        case 0x78: return 0x65; // VK_F9 -> kVK_F9
        case 0x7A: return 0x67; // VK_F11 -> kVK_F11
        case 0x7C: return 0x69; // VK_F13 -> kVK_F13
        case 0x7D: return 0x6B; // VK_F14 -> kVK_F14
        case 0x79: return 0x6D; // VK_F10 -> kVK_F10
        case 0x7B: return 0x6F; // VK_F12 -> kVK_F12
        case 0x7E: return 0x71; // VK_F15 -> kVK_F15
        case 0x2D: return 0x72; // VK_INSERT -> kVK_Help (macOS 'Help' is often 'Insert' on Windows)
        case 0x24: return 0x73; // VK_HOME -> kVK_Home
        case 0x21: return 0x74; // VK_PRIOR -> kVK_PageUp
        case 0x2E: return 0x75; // VK_DELETE -> kVK_ForwardDelete
        case 0x73: return 0x76; // VK_F4 -> kVK_F4
        case 0x23: return 0x77; // VK_END -> kVK_End
        case 0x71: return 0x78; // VK_F2 -> kVK_F2
        case 0x22: return 0x79; // VK_NEXT -> kVK_PageDown
        case 0x70: return 0x7A; // VK_F1 -> kVK_F1
        case 0x25: return 0x7B; // VK_LEFT -> kVK_LeftArrow
        case 0x27: return 0x7C; // VK_RIGHT -> kVK_RightArrow
        case 0x28: return 0x7D; // VK_DOWN -> kVK_DownArrow
        case 0x26: return 0x7E; // VK_UP -> kVK_UpArrow

        default:   return MACOS_UNASSIGNED; // Or a specific MACOS_UNASSIGNED if you define one
    }
}

/**
 * @brief Maps a Windows virtual key code to a Linux Evdev key code.
 *
 * This function takes a Windows virtual key code and returns the
 * corresponding Linux Evdev key code.
 * If a direct mapping is not available, EVDEV_UNASSIGNED (0x00) is returned.
 *
 * @param win_vkey_code The Windows virtual key code.
 * @return The corresponding Linux Evdev key code, or EVDEV_UNASSIGNED.
 */
uint16_t win_vkey_to_evdev_key(uint8_t win_vkey_code) {
    switch (win_vkey_code) {
        // Mouse Buttons
        case 0x01: return 0x110; // VK_LBUTTON -> BTN_LEFT
        case 0x02: return 0x111; // VK_RBUTTON -> BTN_RIGHT
        case 0x04: return 0x112; // VK_MBUTTON -> BTN_MIDDLE
        case 0x05: return 0x113; // VK_XBUTTON1 -> BTN_SIDE
        case 0x06: return 0x114; // VK_XBUTTON2 -> BTN_EXTRA

        // Keyboard Keys (inverse of evdev_to_win_vkey)
        case 0x1B: return 1;   // VK_ESCAPE -> KEY_ESC
        case 0x31: return 2;   // '1' -> KEY_1
        case 0x32: return 3;   // '2' -> KEY_2
        case 0x33: return 4;   // '3' -> KEY_3
        case 0x34: return 5;   // '4' -> KEY_4
        case 0x35: return 6;   // '5' -> KEY_5
        case 0x36: return 7;   // '6' -> KEY_6
        case 0x37: return 8;   // '7' -> KEY_7
        case 0x38: return 9;   // '8' -> KEY_8
        case 0x39: return 10;  // '9' -> KEY_9
        case 0x30: return 11;  // '0' -> KEY_0
        case 0xBD: return 12;  // VK_OEM_MINUS -> KEY_MINUS
        case 0xBB: return 13;  // VK_OEM_PLUS -> KEY_EQUAL
        case 0x08: return 14;  // VK_BACK -> KEY_BACKSPACE
        case 0x09: return 15;  // VK_TAB -> KEY_TAB
        case 0x51: return 16;  // 'Q' -> KEY_Q
        case 0x57: return 17;  // 'W' -> KEY_W
        case 0x45: return 18;  // 'E' -> KEY_E
        case 0x52: return 19;  // 'R' -> KEY_R
        case 0x54: return 20;  // 'T' -> KEY_T
        case 0x59: return 21;  // 'Y' -> KEY_Y
        case 0x55: return 22;  // 'U' -> KEY_U
        case 0x49: return 23;  // 'I' -> KEY_I
        case 0x4F: return 24;  // 'O' -> KEY_O
        case 0x50: return 25;  // 'P' -> KEY_P
        case 0xDB: return 26;  // VK_OEM_4 -> KEY_LEFTBRACE
        case 0xDD: return 27;  // VK_OEM_6 -> KEY_RIGHTBRACE
        case 0x0D: return 28;  // VK_RETURN -> KEY_ENTER (also for VK_KP_ENTER, requires context if differentiation needed)
        case 0x11: return 29;  // VK_CONTROL (L or R) -> KEY_LEFTCTRL (or KEY_RIGHTCTRL)
        case 0x41: return 30;  // 'A' -> KEY_A
        case 0x53: return 31;  // 'S' -> KEY_S
        case 0x44: return 32;  // 'D' -> KEY_D
        case 0x46: return 33;  // 'F' -> KEY_F
        case 0x47: return 34;  // 'G' -> KEY_G
        case 0x48: return 35;  // 'H' -> KEY_H
        case 0x4A: return 36;  // 'J' -> KEY_J
        case 0x4B: return 37;  // 'K' -> KEY_K
        case 0x4C: return 38;  // 'L' -> KEY_L
        case 0xBA: return 39;  // VK_OEM_1 -> KEY_SEMICOLON
        case 0xDE: return 40;  // VK_OEM_7 -> KEY_APOSTROPHE
        case 0xC0: return 41;  // VK_OEM_3 -> KEY_GRAVE
        case 0x10: return 42;  // VK_SHIFT (L or R) -> KEY_LEFTSHIFT (or KEY_RIGHTSHIFT)
        case 0xDC: return 43;  // VK_OEM_5 -> KEY_BACKSLASH
        case 0x5A: return 44;  // 'Z' -> KEY_Z
        case 0x58: return 45;  // 'X' -> KEY_X
        case 0x43: return 46;  // 'C' -> KEY_C
        case 0x56: return 47;  // 'V' -> KEY_V
        case 0x42: return 48;  // 'B' -> KEY_B
        case 0x4E: return 49;  // 'N' -> KEY_N
        case 0x4D: return 50;  // 'M' -> KEY_M
        case 0xBC: return 51;  // VK_OEM_COMMA -> KEY_COMMA
        case 0xBE: return 52;  // VK_OEM_PERIOD -> KEY_DOT
        case 0xBF: return 53;  // VK_OEM_2 -> KEY_SLASH
        // case 0x10 (VK_RSHIFT) -> 54, but already mapped for VK_LSHIFT.
        case 0x6A: return 55;  // VK_MULTIPLY -> KEY_KPASTERISK
        case 0x12: return 56;  // VK_MENU (L or R) -> KEY_LEFTALT (or KEY_RIGHTALT)
        case 0x20: return 57;  // VK_SPACE -> KEY_SPACE
        case 0x14: return 58;  // VK_CAPITAL -> KEY_CAPSLOCK
        case 0x70: return 59;  // VK_F1 -> KEY_F1
        case 0x71: return 60;  // VK_F2 -> KEY_F2
        case 0x72: return 61;  // VK_F3 -> KEY_F3
        case 0x73: return 62;  // VK_F4 -> KEY_F4
        case 0x74: return 63;  // VK_F5 -> KEY_F5
        case 0x75: return 64;  // VK_F6 -> KEY_F6
        case 0x76: return 65;  // VK_F7 -> KEY_F7
        case 0x77: return 66;  // VK_F8 -> KEY_F8
        case 0x78: return 67;  // VK_F9 -> KEY_F9
        case 0x79: return 68;  // VK_F10 -> KEY_F10
        case 0x90: return 69;  // VK_NUMLOCK -> KEY_NUMLOCK
        case 0x91: return 70;  // VK_SCROLL -> KEY_SCROLLLOCK
        case 0x67: return 71;  // VK_NUMPAD7 -> KEY_KP7
        case 0x68: return 72;  // VK_NUMPAD8 -> KEY_KP8
        case 0x69: return 73;  // VK_NUMPAD9 -> KEY_KP9
        case 0x6D: return 74;  // VK_SUBTRACT -> KEY_KPMINUS
        case 0x64: return 75;  // VK_NUMPAD4 -> KEY_KP4
        case 0x65: return 76;  // VK_NUMPAD5 -> KEY_KP5
        case 0x66: return 77;  // VK_NUMPAD6 -> KEY_KP6
        case 0x6B: return 78;  // VK_ADD -> KEY_KPPLUS
        case 0x61: return 79;  // VK_NUMPAD1 -> KEY_KP1
        case 0x62: return 80;  // VK_NUMPAD2 -> KEY_KP2
        case 0x63: return 81;  // VK_NUMPAD3 -> KEY_KP3
        case 0x60: return 82;  // VK_NUMPAD0 -> KEY_KP0
        case 0x6E: return 83;  // VK_DECIMAL -> KEY_KPDOT
        case 0x7A: return 87;  // VK_F11 -> KEY_F11
        case 0x7B: return 88;  // VK_F12 -> KEY_F12
        // VK_RETURN maps to 28 for main ENTER, and 96 for KP_ENTER.
        // Assuming primary mapping for VK_RETURN.
        // 0x11 (VK_RCONTROL) -> 97, but already mapped for VK_LCONTROL.
        case 0x6F: return 98;  // VK_DIVIDE -> KEY_KPSLASH
        case 0x2C: return 99;  // VK_SNAPSHOT -> KEY_SYSRQ
        // 0x12 (VK_RMENU) -> 100, but already mapped for VK_LMENU.
        case 0x24: return 102; // VK_HOME -> KEY_HOME
        case 0x26: return 103; // VK_UP -> KEY_UP
        case 0x21: return 104; // VK_PRIOR -> KEY_PAGEUP
        case 0x25: return 105; // VK_LEFT -> KEY_LEFT
        case 0x27: return 106; // VK_RIGHT -> KEY_RIGHT
        case 0x23: return 107; // VK_END -> KEY_END
        case 0x28: return 108; // VK_DOWN -> KEY_DOWN
        case 0x22: return 109; // VK_NEXT -> KEY_PAGEDOWN
        case 0x2D: return 110; // VK_INSERT -> KEY_INSERT
        case 0x2E: return 111; // VK_DELETE -> KEY_DELETE
        case 0x13: return 119; // VK_PAUSE -> KEY_PAUSE
        case 0x5B: return 125; // VK_LWIN -> KEY_LEFTMETA
        case 0x5C: return 126; // VK_RWIN -> KEY_RIGHTMETA
        case 0x5D: return 127; // VK_APPS -> KEY_COMPOSE
        case 0x7C: return 183; // VK_F13 -> KEY_F13
        case 0x7D: return 184; // VK_F14 -> KEY_F14
        case 0x7E: return 185; // VK_F15 -> KEY_F15
        case 0x7F: return 186; // VK_F16 -> KEY_F16
        case 0x80: return 187; // VK_F17 -> KEY_F17
        case 0x81: return 188; // VK_F18 -> KEY_F18
        case 0x82: return 189; // VK_F19 -> KEY_F19
        case 0x83: return 190; // VK_F20 -> KEY_F20
        case 0x84: return 191; // VK_F21 -> KEY_F21
        case 0x85: return 192; // VK_F22 -> KEY_F22
        case 0x86: return 193; // VK_F23 -> KEY_F23
        case 0x87: return 194; // VK_F24 -> KEY_F24

        default:  return EVDEV_UNASSIGNED;
    }
}

#endif // KEY_MAPPING_H