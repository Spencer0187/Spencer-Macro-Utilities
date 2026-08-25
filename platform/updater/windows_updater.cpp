#if defined(_WIN32)

#include "updater.h"
#include "asset_selection.h"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <shellapi.h>
#include <wininet.h>
#include <wintrust.h>
#include <wincrypt.h>
#include <Softpub.h>

#include <cstdint>
#include <cstdlib>
#include <cwchar>
#include <filesystem>
#include <fstream>
#include <random>
#include <sstream>
#include <string>
#include <vector>

namespace smu::updater::detail {
namespace {

constexpr std::size_t kMaximumDownloadBytes = 512ULL * 1024ULL * 1024ULL;

std::wstring Utf8ToWide(const std::string& value)
{
    if (value.empty()) {
        return {};
    }

    const int size = MultiByteToWideChar(CP_UTF8, 0, value.c_str(), -1, nullptr, 0);
    if (size <= 0) {
        return {};
    }

    std::wstring result(static_cast<std::size_t>(size), L'\0');
    if (MultiByteToWideChar(CP_UTF8, 0, value.c_str(), -1, result.data(), size) <= 0) {
        return {};
    }
    result.resize(static_cast<std::size_t>(size - 1));
    return result;
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

bool GetCurrentExecutableLocation(
    std::wstring& executablePath,
    std::wstring& containingDirectory,
    std::string* errorMessage)
{
    std::vector<wchar_t> pathBuffer(1024);
    for (;;) {
        const DWORD length = GetModuleFileNameW(
            nullptr,
            pathBuffer.data(),
            static_cast<DWORD>(pathBuffer.size()));
        if (length == 0) {
            if (errorMessage) {
                *errorMessage = "Could not resolve the running SMU executable.";
            }
            return false;
        }
        if (length < pathBuffer.size() - 1) {
            executablePath.assign(pathBuffer.data(), length);
            break;
        }
        if (pathBuffer.size() >= 32768) {
            if (errorMessage) {
                *errorMessage = "The running SMU executable path is too long to update safely.";
            }
            return false;
        }
        pathBuffer.resize(pathBuffer.size() * 2);
    }

    containingDirectory =
        std::filesystem::path(executablePath).parent_path().wstring();
    if (containingDirectory.empty()) {
        if (errorMessage) {
            *errorMessage = "Could not resolve the folder containing SMU.";
        }
        return false;
    }
    return true;
}

bool ExecutableFolderCanBeWritten(
    const std::wstring& containingDirectory,
    std::string* errorMessage)
{
    const std::wstring probePath =
        containingDirectory + L"\\.smu-update-write-test-" + GenerateRandomHexString();
    HANDLE probe = CreateFileW(
        probePath.c_str(),
        GENERIC_WRITE | DELETE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        nullptr,
        CREATE_NEW,
        FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE,
        nullptr);
    if (probe == INVALID_HANDLE_VALUE) {
        if (errorMessage) {
            *errorMessage =
                "SMU cannot update itself because its containing folder is not writable.";
        }
        return false;
    }
    CloseHandle(probe);
    return true;
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
        if (data.size() > kMaximumDownloadBytes - bytesRead) {
            if (errorMessage) {
                *errorMessage = "Updater download exceeded the 512 MiB safety limit.";
            }
            return false;
        }
        data.insert(data.end(), buffer, buffer + bytesRead);
    }
    return !data.empty();
}

bool DownloadUrlToMemoryImpl(const std::string& url, std::vector<char>& data, std::string* errorMessage)
{
    data.clear();
    if (url.rfind("https://", 0) != 0) {
        if (errorMessage) {
            *errorMessage = "Updater refused a non-HTTPS URL.";
        }
        return false;
    }

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

    DWORD statusCode = 0;
    DWORD statusCodeSize = sizeof(statusCode);
    if (!HttpQueryInfoW(
            connection,
            HTTP_QUERY_STATUS_CODE | HTTP_QUERY_FLAG_NUMBER,
            &statusCode,
            &statusCodeSize,
            nullptr) ||
        statusCode < 200 ||
        statusCode >= 300) {
        InternetCloseHandle(connection);
        InternetCloseHandle(internet);
        if (errorMessage) {
            *errorMessage = "Updater server returned a non-success HTTP status.";
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

bool HasTrustedAuthenticodeSignature(
    const std::wstring& path,
    const char* description,
    std::string* errorMessage)
{
    WINTRUST_FILE_INFO fileInfo {};
    fileInfo.cbStruct = sizeof(fileInfo);
    fileInfo.pcwszFilePath = path.c_str();

    WINTRUST_DATA trustData {};
    trustData.cbStruct = sizeof(trustData);
    trustData.dwUIChoice = WTD_UI_NONE;
    trustData.fdwRevocationChecks = WTD_REVOKE_WHOLECHAIN;
    trustData.dwUnionChoice = WTD_CHOICE_FILE;
    trustData.pFile = &fileInfo;
    trustData.dwStateAction = WTD_STATEACTION_VERIFY;
    trustData.dwProvFlags = WTD_REVOCATION_CHECK_CHAIN;

    GUID policy = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    const LONG status = WinVerifyTrust(nullptr, &policy, &trustData);

    trustData.dwStateAction = WTD_STATEACTION_CLOSE;
    WinVerifyTrust(nullptr, &policy, &trustData);

    if (status != ERROR_SUCCESS) {
        if (errorMessage) {
            std::ostringstream message;
            message << description << " does not have a trusted Authenticode signature (0x"
                    << std::hex << static_cast<unsigned long>(status) << ").";
            *errorMessage = message.str();
        }
        return false;
    }
    return true;
}

bool GetAuthenticodeSignerSha256(
    const std::wstring& path,
    const char* description,
    std::vector<unsigned char>& fingerprint,
    std::string* errorMessage)
{
    fingerprint.clear();

    DWORD encodingType = 0;
    DWORD contentType = 0;
    DWORD formatType = 0;
    HCERTSTORE certificateStore = nullptr;
    HCRYPTMSG cryptMessage = nullptr;
    if (!CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            path.c_str(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            &encodingType,
            &contentType,
            &formatType,
            &certificateStore,
            &cryptMessage,
            nullptr)) {
        if (errorMessage) {
            *errorMessage =
                std::string("Could not read the Authenticode signer from ") +
                description + ".";
        }
        return false;
    }

    const auto closeHandles = [&]() {
        if (cryptMessage) {
            CryptMsgClose(cryptMessage);
        }
        if (certificateStore) {
            CertCloseStore(certificateStore, 0);
        }
    };

    DWORD signerInfoSize = 0;
    if (!CryptMsgGetParam(
            cryptMessage,
            CMSG_SIGNER_INFO_PARAM,
            0,
            nullptr,
            &signerInfoSize) ||
        signerInfoSize < sizeof(CMSG_SIGNER_INFO)) {
        closeHandles();
        if (errorMessage) {
            *errorMessage =
                std::string("Could not inspect the Authenticode signer in ") +
                description + ".";
        }
        return false;
    }

    std::vector<unsigned char> signerInfoBytes(signerInfoSize);
    if (!CryptMsgGetParam(
            cryptMessage,
            CMSG_SIGNER_INFO_PARAM,
            0,
            signerInfoBytes.data(),
            &signerInfoSize)) {
        closeHandles();
        if (errorMessage) {
            *errorMessage =
                std::string("Could not decode the Authenticode signer in ") +
                description + ".";
        }
        return false;
    }

    const auto* signerInfo =
        reinterpret_cast<const CMSG_SIGNER_INFO*>(signerInfoBytes.data());
    CERT_INFO signerCertificateInfo {};
    signerCertificateInfo.Issuer = signerInfo->Issuer;
    signerCertificateInfo.SerialNumber = signerInfo->SerialNumber;
    PCCERT_CONTEXT signerCertificate = CertFindCertificateInStore(
        certificateStore,
        X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
        0,
        CERT_FIND_SUBJECT_CERT,
        &signerCertificateInfo,
        nullptr);
    if (!signerCertificate) {
        closeHandles();
        if (errorMessage) {
            *errorMessage =
                std::string("Could not locate the Authenticode signer certificate in ") +
                description + ".";
        }
        return false;
    }

    DWORD fingerprintSize = 0;
    const bool sizeRead = CertGetCertificateContextProperty(
        signerCertificate,
        CERT_SHA256_HASH_PROP_ID,
        nullptr,
        &fingerprintSize);
    if (sizeRead && fingerprintSize > 0) {
        fingerprint.resize(fingerprintSize);
    }
    const bool fingerprintRead =
        sizeRead &&
        !fingerprint.empty() &&
        CertGetCertificateContextProperty(
            signerCertificate,
            CERT_SHA256_HASH_PROP_ID,
            fingerprint.data(),
            &fingerprintSize);

    CertFreeCertificateContext(signerCertificate);
    closeHandles();

    if (!fingerprintRead) {
        fingerprint.clear();
        if (errorMessage) {
            *errorMessage =
                std::string("Could not calculate the Authenticode signer fingerprint for ") +
                description + ".";
        }
        return false;
    }
    fingerprint.resize(fingerprintSize);
    return true;
}

bool HasMatchingAuthenticodeSigner(
    const std::wstring& currentExecutable,
    const std::wstring& downloadedExecutable,
    std::string* errorMessage)
{
    std::vector<unsigned char> currentSigner;
    std::vector<unsigned char> downloadedSigner;
    if (!GetAuthenticodeSignerSha256(
            currentExecutable,
            "the running SMU executable",
            currentSigner,
            errorMessage) ||
        !GetAuthenticodeSignerSha256(
            downloadedExecutable,
            "the downloaded SMU executable",
            downloadedSigner,
            errorMessage)) {
        return false;
    }
    if (currentSigner != downloadedSigner) {
        if (errorMessage) {
            *errorMessage =
                "Downloaded update was signed by a different certificate than the running SMU executable.";
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
    return ScoreWindowsAssetName(asset.name);
}

bool PlatformAutoApplySupported()
{
    std::wstring executablePath;
    std::wstring containingDirectory;
    std::vector<unsigned char> signer;
    return GetCurrentExecutableLocation(
               executablePath,
               containingDirectory,
               nullptr) &&
        ExecutableFolderCanBeWritten(containingDirectory, nullptr) &&
        HasTrustedAuthenticodeSignature(
            executablePath,
            "The running SMU executable",
            nullptr) &&
        GetAuthenticodeSignerSha256(
            executablePath,
            "the running SMU executable",
            signer,
            nullptr);
}

bool ApplyUpdateFromAsset(
    const ReleaseAsset& asset,
    const std::string& latestVersion,
    const std::string&,
    std::string* errorMessage)
{
    std::vector<char> zipData;
    if (!smu::updater::DownloadAssetToMemory(asset, zipData, errorMessage)) {
        return false;
    }

    const std::string targetExeName =
        "Spencer-Macro-Utilities-V" + latestVersion + "-Windows.exe";
    const std::string targetEntryPath =
        "Spencer-Macro-Utilities/" + targetExeName;
    std::vector<char> exeData;
    if (!smu::updater::ExtractUpdatePackageEntry(zipData, targetEntryPath, exeData, errorMessage)) {
        return false;
    }
    if (exeData.size() < 64ULL * 1024ULL || exeData.size() > kMaximumDownloadBytes) {
        if (errorMessage) {
            *errorMessage = "Downloaded Windows executable failed the updater size checks.";
        }
        return false;
    }

    std::wstring currentExePath;
    std::wstring workingDir;
    if (!GetCurrentExecutableLocation(
            currentExePath,
            workingDir,
            errorMessage) ||
        !ExecutableFolderCanBeWritten(workingDir, errorMessage)) {
        return false;
    }

    const std::wstring targetExeNameWide = Utf8ToWide(targetExeName);
    if (targetExeNameWide.empty()) {
        if (errorMessage) {
            *errorMessage = "Could not convert the versioned Windows update filename.";
        }
        return false;
    }
    const std::wstring targetExePath = workingDir + L"\\" + targetExeNameWide;
    if (_wcsicmp(targetExePath.c_str(), currentExePath.c_str()) == 0) {
        if (errorMessage) {
            *errorMessage = "Updater refused to replace the running executable with the same versioned filename.";
        }
        return false;
    }
    if (GetFileAttributesW(targetExePath.c_str()) != INVALID_FILE_ATTRIBUTES) {
        if (errorMessage) {
            *errorMessage =
                "The target update executable already exists: " + targetExeName +
                ". Close or remove that existing file, then retry the update.";
        }
        return false;
    }

    const std::wstring randomFileName = GenerateRandomHexString();
    const std::wstring tempExePath = workingDir + L"\\" + randomFileName + L".tmp";
    if (!WriteBytesToFile(tempExePath, exeData, errorMessage)) {
        return false;
    }
    if (!HasTrustedAuthenticodeSignature(
            tempExePath,
            "Downloaded update",
            errorMessage) ||
        !HasTrustedAuthenticodeSignature(
            currentExePath,
            "The running SMU executable",
            errorMessage) ||
        !HasMatchingAuthenticodeSigner(
            currentExePath,
            tempExePath,
            errorMessage)) {
        DeleteFileW(tempExePath.c_str());
        return false;
    }

    wchar_t tempDir[MAX_PATH] {};
    const DWORD tempDirLength = GetTempPathW(MAX_PATH, tempDir);
    if (tempDirLength == 0 || tempDirLength >= MAX_PATH) {
        DeleteFileW(tempExePath.c_str());
        if (errorMessage) {
            *errorMessage = "Could not resolve the Windows temporary folder.";
        }
        return false;
    }
    const std::wstring batchFilePath = std::wstring(tempDir) + L"updater-" + GenerateRandomHexString() + L".bat";
    const std::wstring backupExePath =
        currentExePath + L".old-" + GenerateRandomHexString();
    const std::wstring batchScriptContent =
        L"@echo off\r\n"
        L"timeout /t 1 /nobreak > NUL\r\n"
        L"set /a SMU_ATTEMPTS=0\r\n"
        L":move_current\r\n"
        L"move /Y \"%~1\" \"%~3\" > NUL\r\n"
        L"if not errorlevel 1 goto current_moved\r\n"
        L"set /a SMU_ATTEMPTS+=1\r\n"
        L"if %SMU_ATTEMPTS% GEQ 20 goto move_current_failed\r\n"
        L"timeout /t 1 /nobreak > NUL\r\n"
        L"goto move_current\r\n"
        L":current_moved\r\n"
        L"move /Y \"%~2\" \"%~4\" > NUL\r\n"
        L"if errorlevel 1 goto install_failed\r\n"
        L"start \"\" \"%~4\"\r\n"
        L"if errorlevel 1 goto launch_failed\r\n"
        L"del /F /Q \"%~3\" > NUL 2>&1\r\n"
        L"goto cleanup\r\n"
        L":move_current_failed\r\n"
        L"del /F /Q \"%~2\" > NUL 2>&1\r\n"
        L"if exist \"%~1\" start \"\" \"%~1\"\r\n"
        L"goto cleanup\r\n"
        L":install_failed\r\n"
        L"move /Y \"%~3\" \"%~1\" > NUL\r\n"
        L"if errorlevel 1 goto install_restore_failed\r\n"
        L"del /F /Q \"%~2\" > NUL 2>&1\r\n"
        L"start \"\" \"%~1\"\r\n"
        L"goto cleanup\r\n"
        L":install_restore_failed\r\n"
        L"del /F /Q \"%~2\" > NUL 2>&1\r\n"
        L"goto cleanup\r\n"
        L":launch_failed\r\n"
        L"del /F /Q \"%~4\" > NUL 2>&1\r\n"
        L"move /Y \"%~3\" \"%~1\" > NUL\r\n"
        L"start \"\" \"%~1\"\r\n"
        L":cleanup\r\n"
        L"(goto) 2>nul & del /F /Q \"%~f0\"";

    HANDLE batchFile = CreateFileW(batchFilePath.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (batchFile == INVALID_HANDLE_VALUE) {
        DeleteFileW(tempExePath.c_str());
        if (errorMessage) {
            *errorMessage = "Could not create the updater script. Please check permissions.";
        }
        return false;
    }

    const int batchUtf8Size = WideCharToMultiByte(
        CP_UTF8,
        0,
        batchScriptContent.c_str(),
        static_cast<int>(batchScriptContent.size()),
        nullptr,
        0,
        nullptr,
        nullptr);
    if (batchUtf8Size <= 0) {
        CloseHandle(batchFile);
        DeleteFileW(batchFilePath.c_str());
        DeleteFileW(tempExePath.c_str());
        if (errorMessage) {
            *errorMessage = "Could not encode the updater script.";
        }
        return false;
    }
    std::string batchUtf8(static_cast<std::size_t>(batchUtf8Size), '\0');
    WideCharToMultiByte(
        CP_UTF8,
        0,
        batchScriptContent.c_str(),
        static_cast<int>(batchScriptContent.size()),
        batchUtf8.data(),
        batchUtf8Size,
        nullptr,
        nullptr);
    DWORD batchBytesWritten = 0;
    const bool batchWritten =
        WriteFile(
            batchFile,
            batchUtf8.data(),
            static_cast<DWORD>(batchUtf8.size()),
            &batchBytesWritten,
            nullptr) &&
        batchBytesWritten == batchUtf8.size();
    CloseHandle(batchFile);
    if (!batchWritten) {
        DeleteFileW(batchFilePath.c_str());
        DeleteFileW(tempExePath.c_str());
        if (errorMessage) {
            *errorMessage = "Could not finish writing the updater script.";
        }
        return false;
    }

    const std::wstring params =
        L"\"" + currentExePath + L"\" \"" + tempExePath + L"\" \"" +
        backupExePath + L"\" \"" + targetExePath + L"\"";
    HINSTANCE result = ShellExecuteW(nullptr, L"open", batchFilePath.c_str(), params.c_str(), nullptr, SW_HIDE);
    if (reinterpret_cast<intptr_t>(result) <= 32) {
        DeleteFileW(batchFilePath.c_str());
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
