#if defined(_WIN32)

#include "admin_elevation.h"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <shellapi.h>

namespace smu::platform::windows {
namespace {

std::wstring GetCurrentExecutablePath()
{
    wchar_t path[MAX_PATH] = {};
    const DWORD length = GetModuleFileNameW(nullptr, path, MAX_PATH);
    if (length == 0 || length >= MAX_PATH) {
        return {};
    }
    return path;
}

} // namespace

bool IsRunAsAdmin()
{
    BOOL isRunAsAdmin = FALSE;
    PSID adminSid = nullptr;
    SID_IDENTIFIER_AUTHORITY ntAuthority = SECURITY_NT_AUTHORITY;

    if (AllocateAndInitializeSid(
            &ntAuthority,
            2,
            SECURITY_BUILTIN_DOMAIN_RID,
            DOMAIN_ALIAS_RID_ADMINS,
            0, 0, 0, 0, 0, 0,
            &adminSid)) {
        if (!CheckTokenMembership(nullptr, adminSid, &isRunAsAdmin)) {
            isRunAsAdmin = FALSE;
        }
        FreeSid(adminSid);
    }

    return isRunAsAdmin != FALSE;
}

bool RestartAsAdmin()
{
    const std::wstring executablePath = GetCurrentExecutablePath();
    if (executablePath.empty()) {
        return false;
    }

    SHELLEXECUTEINFOW info = {};
    info.cbSize = sizeof(info);
    info.lpVerb = L"runas";
    info.lpFile = executablePath.c_str();
    info.nShow = SW_SHOWNORMAL;

    return ShellExecuteExW(&info) != FALSE;
}

} // namespace smu::platform::windows

#endif
