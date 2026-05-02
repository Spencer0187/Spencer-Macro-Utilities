#include "askpass.h"

#include "imgui.h"
#include "imgui_impl_opengl3.h"
#include "imgui_impl_sdl3.h"

#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>
#include <SDL3/SDL_opengl.h>

#include <cstring>
#include <string>

namespace smu::app {

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

        if (font) ImGui::PopFont();

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

} // namespace smu::app