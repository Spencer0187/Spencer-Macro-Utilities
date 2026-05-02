#include "app_main.h"

#include "app_ui.h"
#include "../core/app_state.h"

#include "imgui.h"
#include "imgui_impl_opengl3.h"
#include "imgui_impl_sdl3.h"

#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>
#include <SDL3/SDL_main.h>
#include <SDL3/SDL_opengl.h>

#include <algorithm>
#include <chrono>
#include <string>
#include <thread>

#include "../platform/logging.h"

namespace smu::app {
namespace {

void UpdateWindowMetrics(SDL_Window* window)
{
    auto& state = smu::core::GetAppState();

    int pixelWidth = 0;
    int pixelHeight = 0;
    if (SDL_GetWindowSizeInPixels(window, &pixelWidth, &pixelHeight) && pixelWidth > 0 && pixelHeight > 0) {
        state.screenWidth = pixelWidth;
        state.screenHeight = pixelHeight;
    } else {
        int windowWidth = 0;
        int windowHeight = 0;
        if (SDL_GetWindowSize(window, &windowWidth, &windowHeight) && windowWidth > 0 && windowHeight > 0) {
            state.screenWidth = windowWidth;
            state.screenHeight = windowHeight;
        }
    }

    state.rawWindowWidth = state.screenWidth;
    state.rawWindowHeight = state.screenHeight;
}

} // namespace

int RunSharedApp(AppContext& context, const AppMainConfig& config)
{
    auto& state = smu::core::GetAppState();
    smu::core::ResetRuntimeAppFlags();

    if (state.screenWidth <= 0 || state.screenWidth > 15360) {
        state.screenWidth = config.defaultWidth;
    }
    if (state.screenHeight <= 0 || state.screenHeight > 8640) {
        state.screenHeight = config.defaultHeight;
    }

    SDL_SetMainReady();
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        LogCritical(std::string("Failed SDL initialization: ") + SDL_GetError());
        return 1;
    }

    SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_COMPATIBILITY);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 3);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 0);
    SDL_GL_SetAttribute(SDL_GL_DOUBLEBUFFER, 1);
    SDL_GL_SetAttribute(SDL_GL_DEPTH_SIZE, 24);
    SDL_GL_SetAttribute(SDL_GL_STENCIL_SIZE, 8);

    SDL_Window* window = SDL_CreateWindow(config.title, state.screenWidth, state.screenHeight,
        SDL_WINDOW_OPENGL | SDL_WINDOW_RESIZABLE | SDL_WINDOW_HIDDEN);
    if (!window) {
        LogCritical(std::string("Failed SDL window creation: ") + SDL_GetError());
        SDL_Quit();
        return 1;
    }

    SDL_SetWindowMinimumSize(window, 1147, 780);
    if (state.windowPosX != 0 || state.windowPosY != 0) {
        SDL_SetWindowPosition(window, state.windowPosX, state.windowPosY);
    }

    SDL_GLContext glContext = SDL_GL_CreateContext(window);
    if (!glContext) {
        LogCritical(std::string("Failed OpenGL initialization: SDL_GL_CreateContext failed: ") + SDL_GetError());
        SDL_DestroyWindow(window);
        SDL_Quit();
        return 1;
    }

    if (!SDL_GL_MakeCurrent(window, glContext)) {
        LogCritical(std::string("Failed OpenGL initialization: SDL_GL_MakeCurrent failed: ") + SDL_GetError());
        SDL_GL_DestroyContext(glContext);
        SDL_DestroyWindow(window);
        SDL_Quit();
        return 1;
    }

    SDL_GL_SetSwapInterval(0);
    UpdateWindowMetrics(window);

    const float opacity = std::clamp(state.windowOpacityPercent / 100.0f, 0.2f, 1.0f);
    if (!SDL_SetWindowOpacity(window, opacity)) {
        LogWarning("SDL window opacity could not be applied on this platform.");
    }
    if (state.alwaysOnTop && !SDL_SetWindowAlwaysOnTop(window, true)) {
        LogWarning("SDL always-on-top could not be applied on this platform.");
    }

    SDL_ShowWindow(window);

    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGuiIO& io = ImGui::GetIO();
    io.IniFilename = nullptr;
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
    ImGui::StyleColorsDark();

    if (!ImGui_ImplSDL3_InitForOpenGL(window, glContext)) {
        LogCritical("Failed ImGui initialization: SDL3 backend initialization failed.");
    }
    if (!ImGui_ImplOpenGL3_Init("#version 130")) {
        LogCritical("Failed OpenGL initialization: ImGui OpenGL backend initialization failed.");
    }

    constexpr int targetFps = 60;
    const auto targetFrameDuration = std::chrono::duration_cast<std::chrono::steady_clock::duration>(
        std::chrono::duration<double>(1.0 / static_cast<double>(targetFps)));
    auto nextFrameTime = std::chrono::steady_clock::now();

    while (state.running.load(std::memory_order_acquire) && !state.done.load(std::memory_order_acquire)) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            ImGui_ImplSDL3_ProcessEvent(&event);
            if (event.type == SDL_EVENT_QUIT || event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED) {
                state.done.store(true, std::memory_order_release);
                state.running.store(false, std::memory_order_release);
            }
            if (event.type == SDL_EVENT_WINDOW_RESIZED ||
                event.type == SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED ||
                event.type == SDL_EVENT_WINDOW_MOVED) {
                UpdateWindowMetrics(window);
            }
        }

        ImGui_ImplOpenGL3_NewFrame();
        ImGui_ImplSDL3_NewFrame();
        ImGui::NewFrame();

        RenderAppUi(context);

        ImGui::Render();
        UpdateWindowMetrics(window);
        glViewport(0, 0, state.screenWidth, state.screenHeight);
        glClearColor(0.08f, 0.09f, 0.10f, 1.0f);
        glClear(GL_COLOR_BUFFER_BIT);
        ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
        SDL_GL_SwapWindow(window);

        nextFrameTime += targetFrameDuration;
        const auto nowAfterRender = std::chrono::steady_clock::now();
        if (nextFrameTime <= nowAfterRender) {
            nextFrameTime = nowAfterRender + targetFrameDuration;
        }
        std::this_thread::sleep_until(nextFrameTime);
    }

    ImGui_ImplOpenGL3_Shutdown();
    ImGui_ImplSDL3_Shutdown();
    ImGui::DestroyContext();
    SDL_GL_DestroyContext(glContext);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}

} // namespace smu::app
