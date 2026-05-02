#include "text_input_backend.h"

#include "input_backend.h"
#include "../core/key_codes.h"

#include <algorithm>
#include <chrono>
#include <cctype>
#include <thread>

namespace smu::platform {
namespace {

using smu::core::SMU_VK_0;
using smu::core::SMU_VK_1;
using smu::core::SMU_VK_2;
using smu::core::SMU_VK_3;
using smu::core::SMU_VK_4;
using smu::core::SMU_VK_5;
using smu::core::SMU_VK_6;
using smu::core::SMU_VK_7;
using smu::core::SMU_VK_8;
using smu::core::SMU_VK_9;
using smu::core::SMU_VK_OEM_1;
using smu::core::SMU_VK_OEM_2;
using smu::core::SMU_VK_OEM_3;
using smu::core::SMU_VK_OEM_4;
using smu::core::SMU_VK_OEM_5;
using smu::core::SMU_VK_OEM_6;
using smu::core::SMU_VK_OEM_7;
using smu::core::SMU_VK_OEM_COMMA;
using smu::core::SMU_VK_OEM_MINUS;
using smu::core::SMU_VK_OEM_PERIOD;
using smu::core::SMU_VK_OEM_PLUS;
using smu::core::SMU_VK_RETURN;
using smu::core::SMU_VK_SHIFT;
using smu::core::SMU_VK_SPACE;
using smu::core::SMU_VK_TAB;

constexpr PlatformKeyCode kScan1 = 0x02;
constexpr PlatformKeyCode kScan2 = 0x03;
constexpr PlatformKeyCode kScan3 = 0x04;
constexpr PlatformKeyCode kScan4 = 0x05;
constexpr PlatformKeyCode kScan5 = 0x06;
constexpr PlatformKeyCode kScan6 = 0x07;
constexpr PlatformKeyCode kScan7 = 0x08;
constexpr PlatformKeyCode kScan8 = 0x09;
constexpr PlatformKeyCode kScan9 = 0x0A;
constexpr PlatformKeyCode kScan0 = 0x0B;
constexpr PlatformKeyCode kScanMinus = 0x0C;
constexpr PlatformKeyCode kScanPlus = 0x0D;
constexpr PlatformKeyCode kScanTab = 0x0F;
constexpr PlatformKeyCode kScanReturn = 0x1C;
constexpr PlatformKeyCode kScanSpace = 0x39;

PlatformKeyCode scanCodeForKey(PlatformKeyCode key)
{
    if (key >= 'A' && key <= 'Z') {
        static constexpr PlatformKeyCode scans[] = {
            0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32,
            0x31, 0x18, 0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
        };
        return scans[key - 'A'];
    }

    switch (key) {
    case SMU_VK_1: return kScan1;
    case SMU_VK_2: return kScan2;
    case SMU_VK_3: return kScan3;
    case SMU_VK_4: return kScan4;
    case SMU_VK_5: return kScan5;
    case SMU_VK_6: return kScan6;
    case SMU_VK_7: return kScan7;
    case SMU_VK_8: return kScan8;
    case SMU_VK_9: return kScan9;
    case SMU_VK_0: return kScan0;
    case SMU_VK_SPACE: return kScanSpace;
    case SMU_VK_RETURN: return kScanReturn;
    case SMU_VK_TAB: return kScanTab;
    case SMU_VK_OEM_3: return 0x29;
    case SMU_VK_OEM_MINUS: return kScanMinus;
    case SMU_VK_OEM_PLUS: return kScanPlus;
    case SMU_VK_OEM_4: return 0x1A;
    case SMU_VK_OEM_6: return 0x1B;
    case SMU_VK_OEM_5: return 0x2B;
    case SMU_VK_OEM_1: return 0x27;
    case SMU_VK_OEM_7: return 0x28;
    case SMU_VK_OEM_COMMA: return 0x33;
    case SMU_VK_OEM_PERIOD: return 0x34;
    case SMU_VK_OEM_2: return 0x35;
    default: return kNoKey;
    }
}

KeyAction makeAction(PlatformKeyCode key, bool needsShift)
{
    return {key, scanCodeForKey(key), needsShift, key != kNoKey};
}

} // namespace

KeyAction CharToKeyAction_Compat(char c)
{
    if (c >= 'a' && c <= 'z') {
        return makeAction(static_cast<PlatformKeyCode>(std::toupper(static_cast<unsigned char>(c))), false);
    }
    if (c >= 'A' && c <= 'Z') {
        return makeAction(static_cast<PlatformKeyCode>(c), true);
    }
    if (c >= '0' && c <= '9') {
        return makeAction(static_cast<PlatformKeyCode>(c), false);
    }

    switch (c) {
    case ' ': return makeAction(SMU_VK_SPACE, false);
    case '\n': return makeAction(SMU_VK_RETURN, false);
    case '\t': return makeAction(SMU_VK_TAB, false);
    case '!': return makeAction(SMU_VK_1, true);
    case '@': return makeAction(SMU_VK_2, true);
    case '#': return makeAction(SMU_VK_3, true);
    case '$': return makeAction(SMU_VK_4, true);
    case '%': return makeAction(SMU_VK_5, true);
    case '^': return makeAction(SMU_VK_6, true);
    case '&': return makeAction(SMU_VK_7, true);
    case '*': return makeAction(SMU_VK_8, true);
    case '(': return makeAction(SMU_VK_9, true);
    case ')': return makeAction(SMU_VK_0, true);
    case '`': return makeAction(SMU_VK_OEM_3, false);
    case '-': return makeAction(SMU_VK_OEM_MINUS, false);
    case '=': return makeAction(SMU_VK_OEM_PLUS, false);
    case '[': return makeAction(SMU_VK_OEM_4, false);
    case ']': return makeAction(SMU_VK_OEM_6, false);
    case '\\': return makeAction(SMU_VK_OEM_5, false);
    case ';': return makeAction(SMU_VK_OEM_1, false);
    case '\'': return makeAction(SMU_VK_OEM_7, false);
    case ',': return makeAction(SMU_VK_OEM_COMMA, false);
    case '.': return makeAction(SMU_VK_OEM_PERIOD, false);
    case '/': return makeAction(SMU_VK_OEM_2, false);
    case '~': return makeAction(SMU_VK_OEM_3, true);
    case '_': return makeAction(SMU_VK_OEM_MINUS, true);
    case '+': return makeAction(SMU_VK_OEM_PLUS, true);
    case '{': return makeAction(SMU_VK_OEM_4, true);
    case '}': return makeAction(SMU_VK_OEM_6, true);
    case '|': return makeAction(SMU_VK_OEM_5, true);
    case ':': return makeAction(SMU_VK_OEM_1, true);
    case '"': return makeAction(SMU_VK_OEM_7, true);
    case '<': return makeAction(SMU_VK_OEM_COMMA, true);
    case '>': return makeAction(SMU_VK_OEM_PERIOD, true);
    case '?': return makeAction(SMU_VK_OEM_2, true);
    default: return {};
    }
}

KeyAction CharToKeyAction_Global(char c)
{
    return CharToKeyAction_Compat(c);
}

KeyAction charToKeyAction(char c)
{
    return CharToKeyAction_Compat(c);
}

bool typeText(InputBackend& input, std::string_view text, int delayMs)
{
    const int safeDelayMs = std::max(0, delayMs);
    bool typedAny = false;

    for (char c : text) {
        const KeyAction action = charToKeyAction(c);
        if (!action.valid) {
            continue;
        }

        if (action.needsShift) {
            input.holdKeyChord(SMU_VK_SHIFT);
        }

        input.holdKeyChord(action.key);
        std::this_thread::sleep_for(std::chrono::milliseconds(safeDelayMs));
        input.releaseKeyChord(action.key);

        if (action.needsShift) {
            input.releaseKeyChord(SMU_VK_SHIFT);
        }

        typedAny = true;
    }

    return typedAny;
}

bool pasteText(std::string_view text, int delayMs)
{
    auto input = GetInputBackend();
    if (!input) {
        return false;
    }
    return typeText(*input, text, delayMs);
}

} // namespace smu::platform
