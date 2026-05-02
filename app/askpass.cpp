#include "askpass.h"

#include "../platform/logging.h"

#include <cstring>
#include <string>

#ifndef SMU_USE_SDL_UI
#if defined(__linux__)
#define SMU_USE_SDL_UI 1
#else
#define SMU_USE_SDL_UI 0
#endif
#endif

#if SMU_USE_SDL_UI
#include "imgui.h"
#include "imgui_impl_opengl3.h"
#include "imgui_impl_sdl3.h"

#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>
#include <SDL3/SDL_opengl.h>
#endif

#if defined(__linux__)
#include <cerrno>
#include <cstdlib>
#include <filesystem>
#include <pwd.h>
#include <system_error>
#include <sys/wait.h>
#include <unistd.h>
#endif

namespace smu::app {

#if SMU_USE_SDL_UI
std::string AskPassword(const char* title, const char* prompt)
{
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        return {};
    }

    SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_COMPATIBILITY);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 3);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 0);
    SDL_GL_SetAttribute(SDL_GL_DOUBLEBUFFER, 1);

    constexpr int W = 500;
    constexpr int H = 100;

    SDL_Window* window = SDL_CreateWindow(title, W, H,
        SDL_WINDOW_OPENGL);
    if (!window) {
        SDL_Quit();
        return {};
    }

    SDL_GLContext gl = SDL_GL_CreateContext(window);
    if (!gl) {
        SDL_DestroyWindow(window);
        SDL_Quit();
        return {};
    }

    SDL_GL_MakeCurrent(window, gl);
    SDL_GL_SetSwapInterval(1);

    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGui::GetIO().IniFilename = nullptr;
    ImGui::StyleColorsDark();
    ImGui_ImplSDL3_InitForOpenGL(window, gl);
    ImGui_ImplOpenGL3_Init("#version 130");

    char buf[256] = {};
    std::string result;
    bool done = false;
    bool cancelled = false;
    bool focusNext = true;

    constexpr int FONT_SIZE = 15;
    ImFont* font = nullptr;

    // Use the default font at the desired size
    ImGuiIO& io = ImGui::GetIO();
    font = io.Fonts->AddFontDefault();

    while (!done) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            ImGui_ImplSDL3_ProcessEvent(&event);
            if (event.type == SDL_EVENT_QUIT ||
                event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED) {
                cancelled = true;
                done = true;
            }
        }

        ImGui_ImplOpenGL3_NewFrame();
        ImGui_ImplSDL3_NewFrame();
        ImGui::NewFrame();

        if (font) ImGui::PushFont(font);

        // Fullscreen overlay window
        ImGui::SetNextWindowPos({0, 0});
        ImGui::SetNextWindowSize({(float)W, (float)H});
        ImGui::Begin("##Password", nullptr,
            ImGuiWindowFlags_NoTitleBar |
            ImGuiWindowFlags_NoResize |
            ImGuiWindowFlags_NoMove |
            ImGuiWindowFlags_NoScrollbar |
            ImGuiWindowFlags_NoCollapse);

        ImGui::Spacing();


        ImVec2 textSize = ImGui::CalcTextSize(prompt);
        float textX = ((float)W - textSize.x) * 0.5f;
        ImGui::SetCursorPosX(textX);
        ImGui::TextUnformatted(prompt);

        ImGui::Spacing();

        // Center input field
        float inputWidth = (float)W - 64.0f;
        float inputX = ((float)W - inputWidth) * 0.5f;
        ImGui::SetCursorPosX(inputX);

        if (focusNext) {
            ImGui::SetKeyboardFocusHere();
            focusNext = false;
        }

        ImGuiInputTextFlags flags =
            ImGuiInputTextFlags_Password |
            ImGuiInputTextFlags_EnterReturnsTrue;

        ImGui::SetNextItemWidth(inputWidth);
        if (ImGui::InputText("##pwd", buf, sizeof(buf), flags)) {
            result = buf;
            done = true;
        }

        ImGui::Spacing();

        // Center buttons
        float buttonWidth = 70.0f;
        float spacing = ImGui::GetStyle().ItemSpacing.x;
        float totalButtonsWidth = buttonWidth * 2.0f + spacing;
        float buttonsX = ((float)W - totalButtonsWidth) * 0.5f;
        ImGui::SetCursorPosX(buttonsX);

        if (ImGui::Button("OK", {buttonWidth, 0})) {
            result = buf;
            done = true;
        }
        ImGui::SameLine();
        if (ImGui::Button("Cancel", {buttonWidth, 0})) {
            cancelled = true;
            done = true;
        }

        // Esc for cancel
        if (ImGui::IsKeyPressed(ImGuiKey_Escape)) {
            cancelled = true;
            done = true;
        }

        ImGui::End();

        if (font) ImGui::PopFont(font);

        ImGui::Render();

        glViewport(0, 0, W, H);
        glClearColor(0.08f, 0.09f, 0.10f, 1.0f);
        glClear(GL_COLOR_BUFFER_BIT);
        ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
        SDL_GL_SwapWindow(window);
    }

    // Zero out the buffer immediately after use
    std::memset(buf, 0, sizeof(buf));

    ImGui_ImplOpenGL3_Shutdown();
    ImGui_ImplSDL3_Shutdown();
    ImGui::DestroyContext();
    SDL_GL_DestroyContext(gl);
    SDL_DestroyWindow(window);
    SDL_Quit();

    return cancelled ? std::string{} : result;
}
#else
std::string AskPassword(const char* title, const char* prompt)
{
    (void)title;
    (void)prompt;
    return {};
}
#endif

#if defined(__linux__)
namespace {

bool PathExists(const std::string& path)
{
    std::error_code ec;
    return !path.empty() && std::filesystem::exists(path, ec) && !ec;
}

std::string GetCurrentUserName()
{
    for (const char* name : {"SMU_REAL_USER", "SUDO_USER", "USER"}) {
        if (const char* user = std::getenv(name)) {
            if (user[0] != '\0' && std::string(user) != "root") {
                return user;
            }
        }
    }

    if (const char* user = std::getenv("USER")) {
        if (user[0] != '\0') {
            return user;
        }
    }

    if (passwd* pwd = getpwuid(getuid())) {
        if (pwd->pw_name) {
            return pwd->pw_name;
        }
    }

    return {};
}

} // namespace

int RunPermissionInstallerWithPkexec(const std::string& scriptPath)
{
    if (!PathExists(scriptPath)) {
        LogWarning("Linux permission installer script was not found at " + scriptPath);
        return 127;
    }

    const std::string targetUser = GetCurrentUserName();
    if (targetUser.empty()) {
        LogWarning("Linux permission installer could not determine the current user.");
        return 1;
    }

    const std::string targetUserEnv = "SMU_TARGET_USER=" + targetUser;
    const pid_t pid = fork();
    if (pid < 0) {
        LogWarning(std::string("Failed to fork pkexec installer: ") + std::strerror(errno));
        return 1;
    }

    if (pid == 0) {
        execlp("pkexec",
            "pkexec",
            "/usr/bin/env",
            targetUserEnv.c_str(),
            scriptPath.c_str(),
            static_cast<char*>(nullptr));
        _exit(errno == ENOENT ? 127 : 126);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        LogWarning(std::string("Failed waiting for pkexec installer: ") + std::strerror(errno));
        return 1;
    }

    if (WIFEXITED(status)) {
        return WEXITSTATUS(status);
    }
    return 1;
}
#else
int RunPermissionInstallerWithPkexec(const std::string& scriptPath)
{
    (void)scriptPath;
    return 127;
}
#endif

std::string BuildPolkitFailureMessage(const std::string& sudoCommand)
{
    std::string message =
        "No graphical polkit authentication agent appears to be available, or authorization was cancelled. "
        "Install/start a polkit agent such as hyprpolkitagent, polkit-kde-agent, or polkit-gnome, ";
    if (!sudoCommand.empty()) {
        message += "or run this manually: " + sudoCommand;
    } else {
        message += "or run the installer manually with sudo.";
    }
    return message;
}

std::string BuildWineSudoCommand(const std::string& helperLinuxPath, const std::string& currentExeName)
{
    std::string command = "start /unix /bin/sh -c \"";
    command += "if command -v zenity >/dev/null; then ";
    command += "zenity --password --title='Authentication Required' --text='Enter your password to run the Input Helper:' | sudo -S -p '' '";
    command += helperLinuxPath;
    command += "' '";
    command += currentExeName;
    command += "';";
    command += "elif command -v kdialog >/dev/null; then ";
    command += "kdialog --password 'Enter your password to run the Input Helper:' | sudo -S -p '' '";
    command += helperLinuxPath;
    command += "' '";
    command += currentExeName;
    command += "';";
    command += "fi";
    command += "\"";
    return command;
}

} // namespace smu::app