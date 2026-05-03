#pragma once

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

namespace smu::platform {

struct FileDialogOptions {
    std::string title;
    std::filesystem::path initialDirectory;
    std::vector<std::string> extensions;
};

enum class FileDialogResultType {
    Selected,
    Cancelled,
    Unavailable,
    Error,
};

struct FileDialogResult {
    FileDialogResultType type = FileDialogResultType::Unavailable;
    std::filesystem::path path;
    std::string error;
};

FileDialogResult OpenNativeFileDialog(const FileDialogOptions& options);

} // namespace smu::platform
