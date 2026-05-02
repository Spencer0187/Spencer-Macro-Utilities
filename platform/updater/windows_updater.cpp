#if defined(_WIN32)

#include "updater.h"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <shlwapi.h>
#include <wininet.h>

#include <algorithm>
#include <cctype>
#include <codecvt>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <random>
#include <sstream>
#include <string>
#include <vector>

namespace smu::updater::detail {
namespace {

std::wstring Utf8ToWide(const std::string& value)
{
    if (value.empty()) {
        return {};
    }

    const int size = MultiByteToWideChar(CP_UTF8, 0, value.c_str(), -1, nullptr, 0);
    if (size <= 0) {
        return {};
    }

    std::wstring result(static_cast<std::size_t>(size - 1), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, value.c_str(), -1, result.data(), size);
    return result;
}

std::string Lower(std::string value)
{
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return value;
}

bool Contains(const std::string& value, const char* needle)
{
    return value.find(needle) != std::string::npos;
}

bool EndsWith(const std::string& value, const char* suffix)
{
    const std::string suffixText(suffix);
    return value.size() >= suffixText.size() &&
        value.compare(value.size() - suffixText.size(), suffixText.size(), suffixText) == 0;
}

std::wstring GenerateRandomHexString()
{
    std::random_device rd;
    std::mt19937 gen(rd());
    std::uniform_int_distribution<> distrib(0, 15);

    std::wstringstream stream;
    for (int i = 0; i < 16; ++i) {
        stream << std::hex << distrib(gen);
    }
    return stream.str();
}

bool ReadInternetHandle(HINTERNET handle, std::vector<char>& data, std::string* errorMessage)
{
    char buffer[8192];
    DWORD bytesRead = 0;
    for (;;) {
        if (!InternetReadFile(handle, buffer, sizeof(buffer), &bytesRead)) {
            if (errorMessage) {
                *errorMessage = "WinINet failed while reading the update response.";
            }
            return false;
        }
        if (bytesRead == 0) {
            break;
        }
        data.insert(data.end(), buffer, buffer + bytesRead);
    }
    return !data.empty();
}

bool DownloadUrlToMemoryImpl(const std::string& url, std::vector<char>& data, std::string* errorMessage)
{
    data.clear();
    const std::wstring wideUrl = Utf8ToWide(url);
    if (wideUrl.empty()) {
        if (errorMessage) {
            *errorMessage = "Updater received an invalid URL.";
        }
        return false;
    }

    HINTERNET internet = InternetOpenW(
        L"Spencer-Macro-Utilities-Updater",
        INTERNET_OPEN_TYPE_DIRECT,
        nullptr,
        nullptr,
        0);
    if (!internet) {
        if (errorMessage) {
            *errorMessage = "WinINet InternetOpen failed.";
        }
        return false;
    }

    DWORD timeoutMs = 10000;
    InternetSetOptionW(internet, INTERNET_OPTION_CONNECT_TIMEOUT, &timeoutMs, sizeof(timeoutMs));
    InternetSetOptionW(internet, INTERNET_OPTION_RECEIVE_TIMEOUT, &timeoutMs, sizeof(timeoutMs));

    constexpr const wchar_t* headers =
        L"User-Agent: Spencer-Macro-Utilities-Updater\r\n"
        L"Accept: application/vnd.github+json\r\n";
    HINTERNET connection = InternetOpenUrlW(
        internet,
        wideUrl.c_str(),
        headers,
        static_cast<DWORD>(-1),
        INTERNET_FLAG_RELOAD |
            INTERNET_FLAG_NO_CACHE_WRITE |
            INTERNET_FLAG_SECURE |
            INTERNET_FLAG_NO_UI,
        0);
    if (!connection) {
        InternetCloseHandle(internet);
        if (errorMessage) {
            *errorMessage = "WinINet could not open the update URL.";
        }
        return false;
    }

    const bool ok = ReadInternetHandle(connection, data, errorMessage);
    InternetCloseHandle(connection);
    InternetCloseHandle(internet);
    return ok;
}

bool WriteBytesToFile(const std::wstring& path, const std::vector<char>& bytes, std::string* errorMessage)
{
    HANDLE file = CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) {
        if (errorMessage) {
            *errorMessage = "Could not create temporary update file. Please check folder permissions.";
        }
        return false;
    }

    DWORD bytesWritten = 0;
    const bool ok = WriteFile(file, bytes.data(), static_cast<DWORD>(bytes.size()), &bytesWritten, nullptr) &&
        bytesWritten == bytes.size();
    CloseHandle(file);

    if (!ok) {
        DeleteFileW(path.c_str());
        if (errorMessage) {
            *errorMessage = "Failed to write update data to disk.";
        }
        return false;
    }

    return true;
}

} // namespace

bool HttpGetString(const std::string& url, std::string& output, std::string* errorMessage)
{
    std::vector<char> data;
    if (!DownloadUrlToMemoryImpl(url, data, errorMessage)) {
        return false;
    }
    output.assign(data.begin(), data.end());
    return true;
}

bool DownloadUrlToFile(const std::string& url, const std::filesystem::path& destination, std::string* errorMessage)
{
    std::vector<char> data;
    if (!DownloadUrlToMemoryImpl(url, data, errorMessage)) {
        return false;
    }

    std::ofstream file(destination, std::ios::binary | std::ios::trunc);
    if (!file) {
        if (errorMessage) {
            *errorMessage = "Could not open update destination for writing: " + destination.string();
        }
        return false;
    }
    file.write(data.data(), static_cast<std::streamsize>(data.size()));
    return file.good();
}

bool DownloadUrlToMemory(const std::string& url, std::vector<char>& data, std::string* errorMessage)
{
    return DownloadUrlToMemoryImpl(url, data, errorMessage);
}

int ScoreAssetForCurrentPlatform(const ReleaseAsset& asset)
{
    const std::string name = Lower(asset.name);
    if (Contains(name, "linux") || Contains(name, "appimage") || EndsWith(name, ".tar.gz") || EndsWith(name, ".tgz")) {
        return 0;
    }
    if (!EndsWith(name, ".zip")) {
        return 0;
    }

    int score = 10;
    if (Contains(name, "windows") || Contains(name, "win64") || Contains(name, "win32")) score += 50;
    if (Contains(name, "x64") || Contains(name, "amd64")) score += 10;
    if (Contains(name, "spencer") || Contains(name, "macro") || Contains(name, "suspend")) score += 8;
    return score;
}

bool PlatformAutoApplySupported()
{
    return true;
}

bool ApplyUpdateFromAsset(const ReleaseAsset& asset, const std::string& newVersion, const std::string& localVersion, std::string* errorMessage)
{
    std::vector<char> zipData;
    if (!smu::updater::DownloadAssetToMemory(asset, zipData, errorMessage)) {
        return false;
    }

    std::vector<char> exeData;
    if (!smu::updater::ExtractUpdatePackage(zipData, "suspend.exe", exeData, errorMessage)) {
        return false;
    }

    wchar_t currentExePathArr[MAX_PATH] {};
    GetModuleFileNameW(nullptr, currentExePathArr, MAX_PATH);
    std::wstring currentExePath = currentExePathArr;
    std::wstring currentExeName = PathFindFileNameW(currentExePathArr);

    wchar_t workingDirArr[MAX_PATH] {};
    wcscpy_s(workingDirArr, currentExePathArr);
    PathRemoveFileSpecW(workingDirArr);
    std::wstring workingDir = workingDirArr;

    const std::wstring randomFileName = GenerateRandomHexString();
    const std::wstring tempExePath = workingDir + L"\\" + randomFileName + L".tmp";
    if (!WriteBytesToFile(tempExePath, exeData, errorMessage)) {
        return false;
    }

    std::wstring_convert<std::codecvt_utf8_utf16<wchar_t>> converter;
    const std::wstring wideLocalVersion = converter.from_bytes(localVersion);
    const std::wstring wideNewVersion = converter.from_bytes(newVersion);

    bool shouldRenameFolder = false;
    std::wstring currentFolderPath = workingDir;
    wchar_t folderNameBuffer[MAX_PATH] {};
    wcscpy_s(folderNameBuffer, currentFolderPath.c_str());
    PathStripPathW(folderNameBuffer);
    std::wstring currentFolderName = folderNameBuffer;

    if (!currentFolderName.empty() &&
        currentFolderName.length() > wideLocalVersion.length() &&
        currentFolderName.substr(currentFolderName.length() - wideLocalVersion.length()) == wideLocalVersion) {
        const wchar_t separator = currentFolderName[currentFolderName.length() - wideLocalVersion.length() - 1];
        if (separator == L'-' || separator == L'_' || separator == L' ' || separator == L'V' || separator == L'v') {
            shouldRenameFolder = true;
        }
    }

    wchar_t tempDir[MAX_PATH] {};
    GetTempPathW(MAX_PATH, tempDir);
    const std::wstring batchFilePath = std::wstring(tempDir) + L"updater-" + GenerateRandomHexString() + L".bat";

    std::wstring batchScriptContent;
    if (shouldRenameFolder) {
        const std::wstring newFolderName =
            currentFolderName.substr(0, currentFolderName.length() - wideLocalVersion.length()) + wideNewVersion;

        wchar_t parentOfCurrentArr[MAX_PATH] {};
        wcscpy_s(parentOfCurrentArr, currentFolderPath.c_str());
        PathRemoveFileSpecW(parentOfCurrentArr);

        const std::wstring newFolderPath = std::wstring(parentOfCurrentArr) + L"\\" + newFolderName;
        const std::wstring tempExePathAfterRename = newFolderPath + L"\\" + randomFileName + L".tmp";
        const std::wstring finalExePathInNewFolder = newFolderPath + L"\\" + currentExeName;

        batchScriptContent =
            L"@echo off\n"
            L"pushd \"%~dp0\"\n\n"
            L"echo Updating and renaming folder...\n"
            L"timeout /t 2 /nobreak > NUL\n\n"
            L"move \"" + currentFolderPath + L"\" \"" + newFolderPath + L"\"\n\n"
            L"del /F /Q \"" + finalExePathInNewFolder + L"\"\n"
            L"move \"" + tempExePathAfterRename + L"\" \"" + finalExePathInNewFolder + L"\"\n\n"
            L"start \"\" \"" + finalExePathInNewFolder + L"\"\n"
            L"(goto) 2>nul & del \"%~f0\"";
    } else {
        batchScriptContent =
            L"@echo off\n"
            L"pushd \"%~dp0\"\n\n"
            L"echo Updating in progress...\n"
            L"timeout /t 2 /nobreak > NUL\n"
            L"del /F /Q \"%~1\"\n"
            L"move \"%~2\" \"%~1\"\n\n"
            L"start \"\" \"%~1\"\n"
            L"(goto) 2>nul & del \"%~f0\"";
    }

    HANDLE batchFile = CreateFileW(batchFilePath.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (batchFile == INVALID_HANDLE_VALUE) {
        DeleteFileW(tempExePath.c_str());
        if (errorMessage) {
            *errorMessage = "Could not create the updater script. Please check permissions.";
        }
        return false;
    }

    const std::string batchUtf8 = converter.to_bytes(batchScriptContent);
    DWORD batchBytesWritten = 0;
    WriteFile(batchFile, batchUtf8.c_str(), static_cast<DWORD>(batchUtf8.size()), &batchBytesWritten, nullptr);
    CloseHandle(batchFile);

    std::wstring params;
    if (!shouldRenameFolder) {
        params = L"\"" + currentExePath + L"\" \"" + tempExePath + L"\" \"" + currentExeName + L"\"";
    }

    HINSTANCE result = ShellExecuteW(nullptr, L"open", batchFilePath.c_str(), params.c_str(), nullptr, SW_HIDE);
    if (reinterpret_cast<intptr_t>(result) <= 32) {
        DeleteFileW(tempExePath.c_str());
        if (errorMessage) {
            *errorMessage = "Could not launch the updater script.";
        }
        return false;
    }

    std::exit(0);
}

} // namespace smu::updater::detail

#endif
