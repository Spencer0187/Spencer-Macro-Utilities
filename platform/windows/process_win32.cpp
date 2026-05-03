#include "process_win32.h"

#if defined(_WIN32)

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <tlhelp32.h>

#include <memory>
#include <optional>
#include <string>
#include <vector>

namespace smu::platform::windows {
namespace {

using NtSuspendProcessFn = LONG(WINAPI*)(HANDLE);
using NtResumeProcessFn = LONG(WINAPI*)(HANDLE);

NtSuspendProcessFn GetSuspendProcessFn()
{
    static NtSuspendProcessFn fn = [] {
        const HMODULE ntdll = GetModuleHandleA("ntdll.dll");
        return ntdll ? reinterpret_cast<NtSuspendProcessFn>(GetProcAddress(ntdll, "NtSuspendProcess")) : nullptr;
    }();
    return fn;
}

NtResumeProcessFn GetResumeProcessFn()
{
    static NtResumeProcessFn fn = [] {
        const HMODULE ntdll = GetModuleHandleA("ntdll.dll");
        return ntdll ? reinterpret_cast<NtResumeProcessFn>(GetProcAddress(ntdll, "NtResumeProcess")) : nullptr;
    }();
    return fn;
}

std::wstring Utf8ToWide(const std::string& value)
{
    if (value.empty()) {
        return {};
    }

    const int sizeNeeded = MultiByteToWideChar(CP_UTF8, 0, value.c_str(), static_cast<int>(value.size()), nullptr, 0);
    if (sizeNeeded <= 0) {
        return {};
    }

    std::wstring wide(static_cast<std::size_t>(sizeNeeded), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, value.c_str(), static_cast<int>(value.size()), wide.data(), sizeNeeded);
    return wide;
}

std::vector<DWORD> FindProcessIdsByName(const std::string& targetName, bool findAll)
{
    if (targetName.empty()) {
        return {};
    }

    const std::wstring targetNameW = Utf8ToWide(targetName);
    if (targetNameW.empty()) {
        return {};
    }

    std::vector<DWORD> pids;
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) {
        return pids;
    }

    PROCESSENTRY32 entry = {};
    entry.dwSize = sizeof(PROCESSENTRY32);

    if (findAll) {
        if (Process32First(snapshot, &entry)) {
            do {
                if (_wcsicmp(entry.szExeFile, targetNameW.c_str()) == 0) {
                    pids.push_back(entry.th32ProcessID);
                }
            } while (Process32Next(snapshot, &entry));
        }
        CloseHandle(snapshot);
        return pids;
    }

    DWORD selectedPid = 0;
    ULONGLONG newestCreationTime = 0;
    bool foundAny = false;
    if (Process32First(snapshot, &entry)) {
        do {
            if (_wcsicmp(entry.szExeFile, targetNameW.c_str()) != 0) {
                continue;
            }

            HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, entry.th32ProcessID);
            if (!process) {
                continue;
            }

            FILETIME creation = {};
            FILETIME exitTime = {};
            FILETIME kernel = {};
            FILETIME user = {};
            if (GetProcessTimes(process, &creation, &exitTime, &kernel, &user)) {
                const ULONGLONG creationTime =
                    (static_cast<ULONGLONG>(creation.dwHighDateTime) << 32) | creation.dwLowDateTime;
                if (!foundAny || creationTime > newestCreationTime) {
                    newestCreationTime = creationTime;
                    selectedPid = entry.th32ProcessID;
                    foundAny = true;
                }
            }
            CloseHandle(process);
        } while (Process32Next(snapshot, &entry));
    }

    CloseHandle(snapshot);
    if (foundAny) {
        pids.push_back(selectedPid);
    }
    return pids;
}

class Win32ProcessBackend final : public smu::platform::ProcessBackend {
public:
    bool init(std::string* errorMessage = nullptr) override
    {
        if (errorMessage) {
            errorMessage->clear();
        }
        return true;
    }

    void shutdown() override {}

    std::optional<PlatformPid> findProcess(const std::string& executableName) const override
    {
        return findMainProcess(executableName);
    }

    std::vector<PlatformPid> findAllProcesses(const std::string& executableName) const override
    {
        const std::vector<DWORD> nativePids = FindProcessIdsByName(executableName, true);
        std::vector<PlatformPid> pids;
        pids.reserve(nativePids.size());
        for (DWORD pid : nativePids) {
            pids.push_back(static_cast<PlatformPid>(pid));
        }
        return pids;
    }

    std::optional<PlatformPid> findMainProcess(const std::string& executableName) const override
    {
        const std::vector<DWORD> nativePids = FindProcessIdsByName(executableName, false);
        if (nativePids.empty()) {
            return std::nullopt;
        }
        return static_cast<PlatformPid>(nativePids.front());
    }

    bool suspend(PlatformPid pid) override
    {
        auto suspendProcess = GetSuspendProcessFn();
        if (!suspendProcess) {
            return false;
        }

        HANDLE process = OpenProcess(PROCESS_SUSPEND_RESUME | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, static_cast<DWORD>(pid));
        if (!process) {
            return false;
        }

        const LONG result = suspendProcess(process);
        CloseHandle(process);
        return result >= 0;
    }

    bool resume(PlatformPid pid) override
    {
        auto resumeProcess = GetResumeProcessFn();
        if (!resumeProcess) {
            return false;
        }

        HANDLE process = OpenProcess(PROCESS_SUSPEND_RESUME | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, static_cast<DWORD>(pid));
        if (!process) {
            return false;
        }

        const LONG result = resumeProcess(process);
        CloseHandle(process);
        return result >= 0;
    }

    bool isForegroundProcess(PlatformPid pid) const override
    {
        HWND foreground = GetForegroundWindow();
        if (!foreground) {
            return false;
        }

        DWORD foregroundPid = 0;
        GetWindowThreadProcessId(foreground, &foregroundPid);
        return foregroundPid == static_cast<DWORD>(pid);
    }
};

} // namespace

std::shared_ptr<smu::platform::ProcessBackend> CreateWindowsProcessBackend()
{
    return std::make_shared<Win32ProcessBackend>();
}

} // namespace smu::platform::windows

#endif
