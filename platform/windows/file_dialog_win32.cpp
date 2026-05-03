#include "../file_dialog.h"

#if defined(_WIN32)

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <commdlg.h>

#include <array>

namespace smu::platform {
namespace {

std::wstring Widen(const std::string& text)
{
    if (text.empty()) {
        return {};
    }
    const int size = MultiByteToWideChar(CP_UTF8, 0, text.c_str(), -1, nullptr, 0);
    if (size <= 0) {
        return {};
    }
    std::wstring wide(static_cast<std::size_t>(size - 1), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, text.c_str(), -1, wide.data(), size);
    return wide;
}

} // namespace

FileDialogResult OpenNativeFileDialog(const FileDialogOptions& options)
{
    std::array<wchar_t, 32768> fileBuffer{};
    const std::wstring title = Widen(options.title.empty() ? "Import SMU Script" : options.title);
    const std::wstring initialDir = options.initialDirectory.empty() ? std::wstring() : options.initialDirectory.wstring();
    const wchar_t filter[] = L"SMU Scripts (*.smus;*.hss;*.lua)\0*.smus;*.hss;*.lua\0All Files (*.*)\0*.*\0\0";

    OPENFILENAMEW ofn{};
    ofn.lStructSize = sizeof(ofn);
    ofn.hwndOwner = nullptr;
    ofn.lpstrFile = fileBuffer.data();
    ofn.nMaxFile = static_cast<DWORD>(fileBuffer.size());
    ofn.lpstrFilter = filter;
    ofn.nFilterIndex = 1;
    ofn.lpstrTitle = title.c_str();
    ofn.lpstrInitialDir = initialDir.empty() ? nullptr : initialDir.c_str();
    ofn.Flags = OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR;

    if (GetOpenFileNameW(&ofn)) {
        return {FileDialogResultType::Selected, std::filesystem::path(fileBuffer.data()), {}};
    }

    const DWORD error = CommDlgExtendedError();
    if (error == 0) {
        return {FileDialogResultType::Cancelled, {}, {}};
    }
    return {FileDialogResultType::Error, {}, "Windows file dialog failed: " + std::to_string(error)};
}

} // namespace smu::platform

#endif
