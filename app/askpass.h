#pragma once

#include <string>

namespace smu::app {

enum class PermissionInstallerResult {
    Success = 0,
    CancelledOrDenied = 1,
    Failed = 2,
    LaunchFailed = 126,
    NotFound = 127
};

int RunPermissionInstallerWithGraphicalPkexec(const std::string& scriptPath);
bool LaunchPermissionInstallerInTerminal(const std::string& scriptPath, std::string* errorMessage);
std::string BuildPolkitFailureMessage(const std::string& sudoCommand);
std::string BuildTerminalFailureMessage(const std::string& sudoCommand, const std::string& errorMessage);

} // namespace smu::app
