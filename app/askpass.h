#pragma once
#include <string>

namespace smu::app {
    std::string AskPassword(const char* title, const char* prompt);
    int RunPermissionInstallerWithPkexec(const std::string& scriptPath);
    std::string BuildPolkitFailureMessage(const std::string& sudoCommand);
    std::string BuildWineSudoCommand(const std::string& helperLinuxPath, const std::string& currentExeName);
}