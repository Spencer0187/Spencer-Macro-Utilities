#include "script_metadata.h"

#include <algorithm>
#include <cctype>
#include <fstream>
#include <sstream>
#include <unordered_map>
#include <vector>

namespace smu::app {
namespace {

std::string Trim(std::string value)
{
    const auto first = std::find_if_not(value.begin(), value.end(), [](unsigned char ch) {
        return std::isspace(ch) != 0;
    });
    const auto last = std::find_if_not(value.rbegin(), value.rend(), [](unsigned char ch) {
        return std::isspace(ch) != 0;
    }).base();
    if (first >= last) {
        return {};
    }
    return std::string(first, last);
}

std::string LowerNoSeparators(std::string value)
{
    std::string out;
    out.reserve(value.size());
    for (unsigned char ch : value) {
        if (ch == ' ' || ch == '_' || ch == '-') {
            continue;
        }
        out.push_back(static_cast<char>(std::tolower(ch)));
    }
    return out;
}

struct MetadataScanResult {
    ImportedScriptMetadata metadata;
    std::optional<std::string> keybind;
};

MetadataScanResult ScanMetadata(const std::filesystem::path& path)
{
    MetadataScanResult result;
    result.metadata.name = path.stem().string();

    std::ifstream file(path);
    if (!file.is_open()) {
        return result;
    }

    std::string line;
    while (std::getline(file, line)) {
        std::string trimmed = Trim(line);
        if (trimmed.empty()) {
            continue;
        }
        if (trimmed.rfind("--", 0) != 0) {
            break;
        }

        trimmed = Trim(trimmed.substr(2));
        if (trimmed.empty() || trimmed.front() != '@') {
            continue;
        }

        const auto colon = trimmed.find(':');
        if (colon == std::string::npos) {
            continue;
        }

        const std::string key = LowerNoSeparators(trimmed.substr(1, colon - 1));
        const std::string value = Trim(trimmed.substr(colon + 1));
        if (value.empty()) {
            continue;
        }

        if (key == "name") {
            result.metadata.name = value;
        } else if (key == "desc") {
            if (result.metadata.description.empty()) {
                result.metadata.description = value;
            }
        } else if (key == "description") {
            result.metadata.description = value;
        } else if (key == "author") {
            result.metadata.author = value;
        } else if (key == "version") {
            result.metadata.version = value;
        } else if (key == "keybind") {
            result.keybind = value;
        }
    }

    if (result.metadata.name.empty()) {
        result.metadata.name = path.stem().string();
    }

    return result;
}

std::vector<std::string> SplitHotkeyParts(const std::string& text)
{
    std::vector<std::string> parts;
    std::stringstream stream(text);
    std::string part;
    while (std::getline(stream, part, '+')) {
        part = Trim(part);
        if (!part.empty()) {
            parts.push_back(part);
        }
    }
    return parts;
}

const std::unordered_map<std::string, core::KeyCode>& KeyNameMap()
{
    static const std::unordered_map<std::string, core::KeyCode> map = {
        {"lmb", core::SMU_VK_LBUTTON},
        {"mouseleft", core::SMU_VK_LBUTTON},
        {"leftmouse", core::SMU_VK_LBUTTON},
        {"rmb", core::SMU_VK_RBUTTON},
        {"mouseright", core::SMU_VK_RBUTTON},
        {"rightmouse", core::SMU_VK_RBUTTON},
        {"mmb", core::SMU_VK_MBUTTON},
        {"mousemiddle", core::SMU_VK_MBUTTON},
        {"middlemouse", core::SMU_VK_MBUTTON},
        {"mouse4", core::SMU_VK_XBUTTON1},
        {"xbutton1", core::SMU_VK_XBUTTON1},
        {"mouse5", core::SMU_VK_XBUTTON2},
        {"xbutton2", core::SMU_VK_XBUTTON2},
        {"space", core::SMU_VK_SPACE},
        {"enter", core::SMU_VK_RETURN},
        {"return", core::SMU_VK_RETURN},
        {"escape", core::SMU_VK_ESCAPE},
        {"esc", core::SMU_VK_ESCAPE},
        {"tab", core::SMU_VK_TAB},
        {"backspace", core::SMU_VK_BACK},
        {"delete", core::SMU_VK_DELETE},
        {"del", core::SMU_VK_DELETE},
        {"insert", core::SMU_VK_INSERT},
        {"ins", core::SMU_VK_INSERT},
        {"home", core::SMU_VK_HOME},
        {"end", core::SMU_VK_END},
        {"pageup", core::SMU_VK_PRIOR},
        {"pagedown", core::SMU_VK_NEXT},
        {"pgup", core::SMU_VK_PRIOR},
        {"pgdn", core::SMU_VK_NEXT},
        {"up", core::SMU_VK_UP},
        {"down", core::SMU_VK_DOWN},
        {"left", core::SMU_VK_LEFT},
        {"right", core::SMU_VK_RIGHT},
        {"slash", core::SMU_VK_OEM_2},
        {"/", core::SMU_VK_OEM_2},
        {"backslash", core::SMU_VK_OEM_5},
        {"\\", core::SMU_VK_OEM_5},
        {"equals", core::SMU_VK_OEM_PLUS},
        {"equal", core::SMU_VK_OEM_PLUS},
        {"=", core::SMU_VK_OEM_PLUS},
        {"minus", core::SMU_VK_OEM_MINUS},
        {"-", core::SMU_VK_OEM_MINUS},
        {"comma", core::SMU_VK_OEM_COMMA},
        {",", core::SMU_VK_OEM_COMMA},
        {"period", core::SMU_VK_OEM_PERIOD},
        {"dot", core::SMU_VK_OEM_PERIOD},
        {".", core::SMU_VK_OEM_PERIOD},
        {"semicolon", core::SMU_VK_OEM_1},
        {";", core::SMU_VK_OEM_1},
        {"quote", core::SMU_VK_OEM_7},
        {"apostrophe", core::SMU_VK_OEM_7},
        {"'", core::SMU_VK_OEM_7},
        {"bracketleft", core::SMU_VK_OEM_4},
        {"leftbracket", core::SMU_VK_OEM_4},
        {"[", core::SMU_VK_OEM_4},
        {"bracketright", core::SMU_VK_OEM_6},
        {"rightbracket", core::SMU_VK_OEM_6},
        {"]", core::SMU_VK_OEM_6},
        {"grave", core::SMU_VK_OEM_3},
        {"backtick", core::SMU_VK_OEM_3},
        {"`", core::SMU_VK_OEM_3},
        {"capslock", core::SMU_VK_CAPITAL},
        {"numlock", core::SMU_VK_NUMLOCK},
        {"scrolllock", core::SMU_VK_SCROLL},
        {"lshift", core::SMU_VK_LSHIFT},
        {"rshift", core::SMU_VK_RSHIFT},
        {"shift", core::SMU_VK_SHIFT},
        {"lctrl", core::SMU_VK_LCONTROL},
        {"rctrl", core::SMU_VK_RCONTROL},
        {"ctrl", core::SMU_VK_CONTROL},
        {"control", core::SMU_VK_CONTROL},
        {"lalt", core::SMU_VK_LMENU},
        {"ralt", core::SMU_VK_RMENU},
        {"alt", core::SMU_VK_MENU},
        {"win", core::SMU_VK_LWIN},
        {"super", core::SMU_VK_LWIN},
        {"meta", core::SMU_VK_LWIN},
    };
    return map;
}

} // namespace

ImportedScriptMetadata ParseScriptMetadata(const std::filesystem::path& path)
{
    return ScanMetadata(path).metadata;
}

std::optional<unsigned int> ParseScriptHotkeyMetadata(const std::filesystem::path& path)
{
    const MetadataScanResult result = ScanMetadata(path);
    if (!result.keybind) {
        return std::nullopt;
    }
    return ParseScriptHotkeyString(*result.keybind);
}

std::optional<core::KeyCode> ParseScriptKeyName(const std::string& text)
{
    std::string normalized = LowerNoSeparators(Trim(text));
    if (normalized.empty()) {
        return std::nullopt;
    }

    if (normalized.size() == 1) {
        const char ch = static_cast<char>(std::toupper(static_cast<unsigned char>(normalized[0])));
        if (ch >= 'A' && ch <= 'Z') {
            return static_cast<core::KeyCode>(ch);
        }
        if (ch >= '0' && ch <= '9') {
            return static_cast<core::KeyCode>(ch);
        }
    }

    if (normalized.size() >= 2 && normalized.front() == 'f') {
        const std::string numberText = normalized.substr(1);
        if (std::all_of(numberText.begin(), numberText.end(), [](unsigned char ch) { return std::isdigit(ch) != 0; })) {
            const int number = std::stoi(numberText);
            if (number >= 1 && number <= 24) {
                return core::SMU_VK_F1 + static_cast<core::KeyCode>(number - 1);
            }
        }
    }

    if (normalized.rfind("numpad", 0) == 0 && normalized.size() == 7 && std::isdigit(static_cast<unsigned char>(normalized[6]))) {
        return core::SMU_VK_NUMPAD0 + static_cast<core::KeyCode>(normalized[6] - '0');
    }

    const auto& map = KeyNameMap();
    const auto it = map.find(normalized);
    if (it != map.end()) {
        return it->second;
    }
    return std::nullopt;
}

std::optional<unsigned int> ParseScriptHotkeyString(const std::string& text)
{
    const std::vector<std::string> parts = SplitHotkeyParts(text);
    if (parts.empty()) {
        return std::nullopt;
    }

    unsigned int modifiers = 0;
    std::optional<core::KeyCode> mainKey;

    for (const std::string& part : parts) {
        const std::string normalized = LowerNoSeparators(part);
        if (normalized == "shift") {
            modifiers |= core::HOTKEY_MASK_SHIFT;
        } else if (normalized == "ctrl" || normalized == "control") {
            modifiers |= core::HOTKEY_MASK_CTRL;
        } else if (normalized == "alt") {
            modifiers |= core::HOTKEY_MASK_ALT;
        } else if (normalized == "win" || normalized == "super" || normalized == "meta") {
            modifiers |= core::HOTKEY_MASK_WIN;
        } else if (auto key = ParseScriptKeyName(part)) {
            mainKey = key;
        } else {
            return std::nullopt;
        }
    }

    if (!mainKey || *mainKey == core::SMU_VK_NONE) {
        return std::nullopt;
    }

    return (*mainKey & core::HOTKEY_KEY_MASK) | modifiers;
}

bool IsSupportedScriptExtension(const std::filesystem::path& path)
{
    std::string ext = path.extension().string();
    std::transform(ext.begin(), ext.end(), ext.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return ext == ".smus" || ext == ".hss" || ext == ".lua";
}

} // namespace smu::app
