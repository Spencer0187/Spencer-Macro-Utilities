#pragma once

#include <cmath>
#include <cctype>
#include <cstdint>
#include <cstdlib>
#include <limits>
#include <optional>
#include <string>
#include <string_view>

namespace smu::app::compat {

inline std::optional<double> ParseFiniteDouble(std::string text)
{
    const auto begin = text.find_first_not_of(" \t\r\n");
    if (begin == std::string::npos) {
        return std::nullopt;
    }
    const auto end = text.find_last_not_of(" \t\r\n");
    text = text.substr(begin, end - begin + 1);

    char* parseEnd = nullptr;
    const double value = std::strtod(text.c_str(), &parseEnd);
    if (parseEnd == text.c_str()) {
        return std::nullopt;
    }
    while (*parseEnd != '\0' && std::isspace(static_cast<unsigned char>(*parseEnd))) {
        ++parseEnd;
    }
    if (*parseEnd != '\0' || !std::isfinite(value)) {
        return std::nullopt;
    }
    return value;
}

struct MoveDegreesCalibrationResult {
    bool valid = false;
    double pixelsPerDegree = 0.0;
    std::string error;
};

inline MoveDegreesCalibrationResult ComputeMoveDegreesCalibration(
    const std::optional<bool>& camfixEnabled,
    const std::optional<std::string>& savedSensitivity)
{
    MoveDegreesCalibrationResult result;
    if (!camfixEnabled) {
        result.error = "moveDegrees requires the saved camfixtoggle setting.";
        return result;
    }
    if (!savedSensitivity) {
        result.error = "moveDegrees requires the saved RobloxSensValue setting.";
        return result;
    }

    const auto sensitivity = ParseFiniteDouble(*savedSensitivity);
    if (!sensitivity || *sensitivity <= 0.0) {
        result.error = "moveDegrees requires RobloxSensValue to be a finite number greater than 0.";
        return result;
    }

    const double baseValue = *camfixEnabled ? 1000.0 : 720.0;
    result.valid = true;
    result.pixelsPerDegree = baseValue / (360.0 * *sensitivity);
    return result;
}

struct MouseDeltaResult {
    bool valid = false;
    int value = 0;
    std::string error;
};

inline MouseDeltaResult RoundMouseDelta(double pixels, std::string_view name)
{
    MouseDeltaResult result;
    if (!std::isfinite(pixels)) {
        result.error = std::string(name) + " conversion overflowed";
        return result;
    }

    const double rounded = std::round(pixels);
    if (rounded < static_cast<double>(std::numeric_limits<int>::min()) ||
        rounded > static_cast<double>(std::numeric_limits<int>::max())) {
        result.error = std::string(name) + " conversion is outside the allowed range";
        return result;
    }

    result.valid = true;
    result.value = static_cast<int>(rounded);
    return result;
}

} // namespace smu::app::compat
