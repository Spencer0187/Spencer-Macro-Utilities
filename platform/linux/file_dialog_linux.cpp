#include "../file_dialog.h"

#if defined(__linux__)

#include <array>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <optional>
#include <sstream>

namespace smu::platform {
namespace {

std::string ShellQuote(const std::string& value)
{
    std::string out = "'";
    for (char ch : value) {
        if (ch == '\'') {
            out += "'\\''";
        } else {
            out += ch;
        }
    }
    out += "'";
    return out;
}

std::optional<std::string> RunCommandCapture(const std::string& command)
{
    FILE* pipe = popen(command.c_str(), "r");
    if (!pipe) {
        return std::nullopt;
    }

    std::string output;
    std::array<char, 512> buffer{};
    while (fgets(buffer.data(), static_cast<int>(buffer.size()), pipe)) {
        output += buffer.data();
    }

    const int status = pclose(pipe);
    if (status != 0 || output.empty()) {
        return std::nullopt;
    }

    while (!output.empty() && (output.back() == '\n' || output.back() == '\r')) {
        output.pop_back();
    }
    if (output.empty()) {
        return std::nullopt;
    }
    return output;
}

bool CommandExists(const char* command)
{
    const std::string check = std::string("command -v ") + command + " >/dev/null 2>&1";
    return std::system(check.c_str()) == 0;
}

std::string InitialDirectory(const FileDialogOptions& options)
{
    if (!options.initialDirectory.empty()) {
        return options.initialDirectory.string();
    }
    if (const char* home = std::getenv("HOME")) {
        return home;
    }
    return ".";
}

} // namespace

FileDialogResult OpenNativeFileDialog(const FileDialogOptions& options)
{
    const std::string title = options.title.empty() ? "Import SMU Script" : options.title;

    if (CommandExists("zenity")) {
        const std::string command =
            "zenity --file-selection --title=" + ShellQuote(title) +
            " --file-filter=" + ShellQuote("SMU Scripts | *.smus *.hss *.lua");
        if (auto output = RunCommandCapture(command)) {
            return {FileDialogResultType::Selected, std::filesystem::path(*output), {}};
        }
        return {FileDialogResultType::Cancelled, {}, {}};
    }

    if (CommandExists("kdialog")) {
        const std::string command =
            "kdialog --getopenfilename " + ShellQuote(InitialDirectory(options)) +
            " " + ShellQuote("*.smus *.hss *.lua|SMU Scripts");
        if (auto output = RunCommandCapture(command)) {
            return {FileDialogResultType::Selected, std::filesystem::path(*output), {}};
        }
        return {FileDialogResultType::Cancelled, {}, {}};
    }

    return {FileDialogResultType::Unavailable, {}, "zenity and kdialog are unavailable."};
}

} // namespace smu::platform

#endif
