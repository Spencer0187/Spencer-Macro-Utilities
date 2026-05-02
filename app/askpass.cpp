#include "askpass.h"

#include "../platform/logging.h"

#include <string>

#if defined(__linux__)
#include <cctype>
#include <cerrno>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fcntl.h>
#include <fstream>
#include <limits.h>
#include <string_view>
#include <system_error>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#include <vector>
#endif

namespace smu::app {

#if defined(__linux__)
namespace {

bool PathExists(const std::string& path)
{
    std::error_code ec;
    return !path.empty() && std::filesystem::exists(path, ec) && !ec;
}

bool FindExecutableInPath(const std::string& name)
{
    if (name.empty()) {
        return false;
    }

    if (name.find('/') != std::string::npos) {
        return access(name.c_str(), X_OK) == 0;
    }

    const char* pathEnv = std::getenv("PATH");
    if (!pathEnv || pathEnv[0] == '\0') {
        return false;
    }

    std::string_view pathList(pathEnv);
    std::size_t start = 0;
    while (start <= pathList.size()) {
        const std::size_t end = pathList.find(':', start);
        std::string_view entry = pathList.substr(start, end == std::string_view::npos ? pathList.size() - start : end - start);
        std::filesystem::path candidate = entry.empty() ? std::filesystem::path(name) : std::filesystem::path(entry) / name;
        if (access(candidate.c_str(), X_OK) == 0) {
            return true;
        }
        if (end == std::string_view::npos) {
            break;
        }
        start = end + 1;
    }

    return false;
}

std::string ShellQuote(const std::string& value)
{
    std::string quoted = "'";
    for (char ch : value) {
        if (ch == '\'') {
            quoted += "'\\''";
        } else {
            quoted += ch;
        }
    }
    quoted += "'";
    return quoted;
}

std::vector<std::string> SplitCommandLine(const std::string& command)
{
    std::vector<std::string> argv;
    std::string current;
    bool inSingleQuotes = false;
    bool inDoubleQuotes = false;
    bool escaping = false;

    for (char ch : command) {
        if (escaping) {
            current += ch;
            escaping = false;
            continue;
        }

        if (ch == '\\' && !inSingleQuotes) {
            escaping = true;
            continue;
        }

        if (ch == '\'' && !inDoubleQuotes) {
            inSingleQuotes = !inSingleQuotes;
            continue;
        }

        if (ch == '"' && !inSingleQuotes) {
            inDoubleQuotes = !inDoubleQuotes;
            continue;
        }

        if (!inSingleQuotes && !inDoubleQuotes && std::isspace(static_cast<unsigned char>(ch))) {
            if (!current.empty()) {
                argv.push_back(current);
                current.clear();
            }
            continue;
        }

        current += ch;
    }

    if (!current.empty()) {
        argv.push_back(current);
    }

    return argv;
}

bool WriteTemporaryScriptFile(const std::string& path, const std::string& contents, std::string* errorMessage)
{
    std::ofstream script(path, std::ios::out | std::ios::trunc);
    if (!script) {
        if (errorMessage) {
            *errorMessage = "Could not create a temporary installer script at " + path + ".";
        }
        return false;
    }

    script << contents;
    script.close();
    if (!script) {
        if (errorMessage) {
            *errorMessage = "Could not finish writing the temporary installer script.";
        }
        return false;
    }

    if (chmod(path.c_str(), 0700) != 0) {
        if (errorMessage) {
            *errorMessage = std::string("Could not mark the temporary installer script executable: ") + std::strerror(errno);
        }
        std::remove(path.c_str());
        return false;
    }

    return true;
}

std::string CreateTemporaryTerminalInstallerScript(const std::string& installerPath, std::string* errorMessage)
{
    const char* runtimeDir = std::getenv("XDG_RUNTIME_DIR");
    std::filesystem::path baseDir =
        (runtimeDir && runtimeDir[0] != '\0') ? std::filesystem::path(runtimeDir) : std::filesystem::path("/tmp");

    std::error_code ec;
    if (!std::filesystem::exists(baseDir, ec) || ec) {
        baseDir = "/tmp";
    }

    const std::string templatePath = (baseDir / "smu-linux-permission-installer-XXXXXX").string();
    std::vector<char> tempPath(templatePath.begin(), templatePath.end());
    tempPath.push_back('\0');

    const int fd = mkstemp(tempPath.data());
    if (fd < 0) {
        if (errorMessage) {
            *errorMessage = std::string("Could not create a temporary installer script: ") + std::strerror(errno);
        }
        return {};
    }
    close(fd);

    const std::string scriptPath(tempPath.data());
    std::string contents;
    contents += "#!/usr/bin/env bash\n";
    contents += "cleanup() {\n";
    contents += "  rm -f -- \"$0\"\n";
    contents += "}\n";
    contents += "trap cleanup EXIT\n";
    contents += "echo \"Spencer Macro Utilities needs permission to access Linux input devices.\"\n";
    contents += "echo\n";
    contents += "sudo " + ShellQuote(installerPath) + "\n";
    contents += "status=$?\n";
    contents += "echo\n";
    contents += "if [[ \"$status\" -eq 0 ]]; then\n";
    contents += "  echo \"Installer completed. Return to SMU and click Retry permission check.\"\n";
    contents += "else\n";
    contents += "  echo \"Installer failed with exit code $status.\"\n";
    contents += "fi\n";
    contents += "echo\n";
    contents += "printf \"Press Enter to close...\"\n";
    contents += "read -r _\n";
    contents += "exit \"$status\"\n";

    if (!WriteTemporaryScriptFile(scriptPath, contents, errorMessage)) {
        return {};
    }

    return scriptPath;
}

bool SpawnDetached(const std::vector<std::string>& argv, std::string* errorMessage)
{
    if (argv.empty()) {
        if (errorMessage) {
            *errorMessage = "No terminal command was provided.";
        }
        return false;
    }

    int pipeFds[2];
    if (pipe(pipeFds) != 0) {
        if (errorMessage) {
            *errorMessage = std::string("Could not create a launch pipe: ") + std::strerror(errno);
        }
        return false;
    }

    const pid_t pid = fork();
    if (pid < 0) {
        close(pipeFds[0]);
        close(pipeFds[1]);
        if (errorMessage) {
            *errorMessage = std::string("Could not fork the terminal installer: ") + std::strerror(errno);
        }
        return false;
    }

    if (pid == 0) {
        close(pipeFds[0]);

        if (setsid() < 0) {
            const int launchErrno = errno;
            (void)write(pipeFds[1], &launchErrno, sizeof(launchErrno));
            _exit(126);
        }

        const pid_t detachedPid = fork();
        if (detachedPid < 0) {
            const int launchErrno = errno;
            (void)write(pipeFds[1], &launchErrno, sizeof(launchErrno));
            _exit(126);
        }

        if (detachedPid > 0) {
            _exit(0);
        }

        if (fcntl(pipeFds[1], F_SETFD, FD_CLOEXEC) != 0) {
            const int launchErrno = errno;
            (void)write(pipeFds[1], &launchErrno, sizeof(launchErrno));
            _exit(126);
        }

        std::vector<char*> execArgv;
        execArgv.reserve(argv.size() + 1);
        for (const std::string& arg : argv) {
            execArgv.push_back(const_cast<char*>(arg.c_str()));
        }
        execArgv.push_back(nullptr);

        execvp(execArgv[0], execArgv.data());

        const int launchErrno = errno;
        (void)write(pipeFds[1], &launchErrno, sizeof(launchErrno));
        _exit(126);
    }

    close(pipeFds[1]);
    int launchErrno = 0;
    const ssize_t bytesRead = read(pipeFds[0], &launchErrno, sizeof(launchErrno));
    close(pipeFds[0]);

    int status = 0;
    (void)waitpid(pid, &status, 0);

    if (bytesRead > 0) {
        if (errorMessage) {
            *errorMessage = std::string("Failed to launch the terminal installer: ") + std::strerror(launchErrno);
        }
        return false;
    }

    return true;
}

std::vector<std::vector<std::string>> BuildTerminalLaunchCommands(const std::string& scriptPath)
{
    std::vector<std::vector<std::string>> commands;
    commands.push_back({"xdg-terminal-exec", "/bin/bash", scriptPath});

    if (const char* terminalEnv = std::getenv("TERMINAL")) {
        if (terminalEnv[0] != '\0') {
            std::vector<std::string> envCommand = SplitCommandLine(terminalEnv);
            if (!envCommand.empty()) {
                envCommand.push_back("/bin/bash");
                envCommand.push_back(scriptPath);
                commands.push_back(std::move(envCommand));
            }
        }
    }

    commands.push_back({"gnome-terminal", "--", "/bin/bash", scriptPath});
    commands.push_back({"kgx", "--", "/bin/bash", scriptPath});
    commands.push_back({"konsole", "-e", "/bin/bash", scriptPath});
    commands.push_back({"xfce4-terminal", "--command", "/bin/bash " + ShellQuote(scriptPath)});
    commands.push_back({"mate-terminal", "--", "/bin/bash", scriptPath});
    commands.push_back({"lxterminal", "-e", "/bin/bash", scriptPath});
    commands.push_back({"kitty", "/bin/bash", scriptPath});
    commands.push_back({"alacritty", "-e", "/bin/bash", scriptPath});
    commands.push_back({"foot", "/bin/bash", scriptPath});
    commands.push_back({"wezterm", "start", "--", "/bin/bash", scriptPath});
    commands.push_back({"xterm", "-e", "/bin/bash", scriptPath});
    return commands;
}

} // namespace

int RunPermissionInstallerWithGraphicalPkexec(const std::string& scriptPath)
{
    if (!PathExists(scriptPath)) {
        LogWarning("Linux permission installer script was not found at " + scriptPath);
        return static_cast<int>(PermissionInstallerResult::NotFound);
    }

    const pid_t pid = fork();
    if (pid < 0) {
        LogWarning(std::string("Failed to fork pkexec installer: ") + std::strerror(errno));
        return static_cast<int>(PermissionInstallerResult::Failed);
    }

    if (pid == 0) {
        execlp(
            "pkexec",
            "pkexec",
            "--disable-internal-agent",
            scriptPath.c_str(),
            static_cast<char*>(nullptr));
        _exit(errno == ENOENT
            ? static_cast<int>(PermissionInstallerResult::NotFound)
            : static_cast<int>(PermissionInstallerResult::LaunchFailed));
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        LogWarning(std::string("Failed waiting for pkexec installer: ") + std::strerror(errno));
        return static_cast<int>(PermissionInstallerResult::Failed);
    }

    if (WIFEXITED(status)) {
        return WEXITSTATUS(status);
    }

    return static_cast<int>(PermissionInstallerResult::Failed);
}

bool LaunchPermissionInstallerInTerminal(const std::string& scriptPath, std::string* errorMessage)
{
    if (!PathExists(scriptPath)) {
        if (errorMessage) {
            *errorMessage = "Linux permission installer script was not found at " + scriptPath + ".";
        }
        return false;
    }

    std::string tempScriptError;
    const std::string tempScriptPath = CreateTemporaryTerminalInstallerScript(scriptPath, &tempScriptError);
    if (tempScriptPath.empty()) {
        if (errorMessage) {
            *errorMessage = tempScriptError;
        }
        return false;
    }

    bool sawTerminalCandidate = false;
    std::string lastLaunchError;
    for (const std::vector<std::string>& command : BuildTerminalLaunchCommands(tempScriptPath)) {
        if (command.empty() || !FindExecutableInPath(command.front())) {
            continue;
        }

        sawTerminalCandidate = true;
        std::string launchError;
        if (SpawnDetached(command, &launchError)) {
            return true;
        }
        lastLaunchError = launchError;
    }

    std::remove(tempScriptPath.c_str());

    if (errorMessage) {
        if (!sawTerminalCandidate) {
            *errorMessage = "No supported terminal emulator was found in PATH.";
        } else if (!lastLaunchError.empty()) {
            *errorMessage = lastLaunchError;
        } else {
            *errorMessage = "A terminal was found, but the installer could not be launched.";
        }
    }
    return false;
}

#else

int RunPermissionInstallerWithGraphicalPkexec(const std::string& scriptPath)
{
    (void)scriptPath;
    return static_cast<int>(PermissionInstallerResult::NotFound);
}

bool LaunchPermissionInstallerInTerminal(const std::string& scriptPath, std::string* errorMessage)
{
    (void)scriptPath;
    if (errorMessage) {
        *errorMessage = "The terminal installer is only available on Linux.";
    }
    return false;
}

#endif

std::string BuildPolkitFailureMessage(const std::string& sudoCommand)
{
    std::string message =
        "Graphical authentication did not start, or permission was denied. "
        "Your desktop may not be running a graphical polkit agent. "
        "Try the terminal installer";
    if (!sudoCommand.empty()) {
        message += ", or run this manually: " + sudoCommand;
    } else {
        message += ", or run the installer manually with sudo.";
    }
    return message;
}

std::string BuildTerminalFailureMessage(const std::string& sudoCommand, const std::string& errorMessage)
{
    std::string message = "SMU could not open a terminal for the Linux permission installer.";
    if (!errorMessage.empty()) {
        message += " " + errorMessage;
    }
    if (!sudoCommand.empty()) {
        message += " Run this manually instead: " + sudoCommand;
    }
    return message;
}

} // namespace smu::app
