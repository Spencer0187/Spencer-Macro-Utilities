// app_main.cpp

#include "app_main.h"

#include "app_assets.h"
#include "app_profile_bridge.h"
#include "app_theme_bridge.h"
#include "app_ui.h"
#include "macro_tutorial_assets.h"
#include "../core/app_state.h"
#include "../core/legacy_globals.h"

#include "imgui.h"
#include "imgui_impl_opengl3.h"
#include "imgui_impl_sdl3.h"

#define SDL_MAIN_HANDLED
#include <SDL3/SDL.h>
#include <SDL3/SDL_main.h>
#include <SDL3/SDL_opengl.h>
#if defined(_WIN32)
#include <SDL3/SDL_properties.h>
#endif

#include <algorithm>
#include <array>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <string>
#include <thread>
#include <atomic>
#include <condition_variable>
#include <vector>

#include "../platform/logging.h"

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <dwmapi.h>
#include "../platform/windows/lagswitch_overlay.h"
#ifndef DWMWA_USE_IMMERSIVE_DARK_MODE
#define DWMWA_USE_IMMERSIVE_DARK_MODE 20
#endif
#elif defined(__linux__)
#include <unistd.h>
#elif defined(__APPLE__)
#include <mach-o/dyld.h>
#endif

namespace smu::app {
namespace {

using FrameClock = std::chrono::steady_clock;
using FrameTimePoint = FrameClock::time_point;

struct WindowSize {
    int width = 0;
    int height = 0;
};

constexpr WindowSize kMinimumRenderSize{1180, 700};
constexpr int kMaximumInitialWindowWidth = 3840;
constexpr int kMaximumInitialWindowHeight = 2160;

FrameTimePoint FrameTimePointFromTicks(std::int64_t ticks)
{
    return FrameTimePoint(FrameClock::duration(ticks));
}

std::int64_t FrameTimePointToTicks(FrameTimePoint timePoint)
{
    return static_cast<std::int64_t>(timePoint.time_since_epoch().count());
}

void ExtendAtomicDeadline(std::atomic<std::int64_t>& deadlineTicks, FrameTimePoint deadline)
{
    const std::int64_t desired = FrameTimePointToTicks(deadline);
    std::int64_t current = deadlineTicks.load(std::memory_order_relaxed);
    while (current < desired &&
        !deadlineTicks.compare_exchange_weak(current, desired, std::memory_order_release, std::memory_order_relaxed)) {
    }
}

std::filesystem::path GetExecutableDirectory()
{
#if defined(_WIN32)
    std::vector<wchar_t> buffer(32768);

    while (true) {
        const DWORD length = GetModuleFileNameW(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
        if (length == 0) {
            return {};
        }

        if (length < buffer.size() - 1) {
            break;
        }

        buffer.resize(buffer.size() * 2);
    }

    return std::filesystem::path(buffer.data()).parent_path();
#elif defined(__linux__)
    std::vector<char> buffer(4096);

    while (true) {
        const ssize_t length = readlink("/proc/self/exe", buffer.data(), buffer.size() - 1);
        if (length < 0) {
            return {};
        }

        if (static_cast<std::size_t>(length) < buffer.size() - 1) {
            buffer[static_cast<std::size_t>(length)] = '\0';
            break;
        }

        buffer.resize(buffer.size() * 2);
    }

    return std::filesystem::path(buffer.data()).parent_path();
#elif defined(__APPLE__)
    std::vector<char> buffer(1024);

    while (true) {
        std::uint32_t size = static_cast<std::uint32_t>(buffer.size());
        if (_NSGetExecutablePath(buffer.data(), &size) == 0) {
            std::error_code ec;
            const std::filesystem::path resolvedPath = std::filesystem::weakly_canonical(buffer.data(), ec);
            return (ec ? std::filesystem::path(buffer.data()) : resolvedPath).parent_path();
        }

        if (size <= buffer.size()) {
            return {};
        }
        buffer.resize(size);
    }
#else
    return {};
#endif
}

constexpr const char kWindowIconWarningId[] = "window_icon_unavailable";
constexpr const char kNativeDarkTitleBarWarningId[] = "native_dark_titlebar_unavailable";
constexpr const char kWindowOpacityWarningId[] = "window_opacity_unavailable";
constexpr const char kWindowAlwaysOnTopWarningId[] = "window_always_on_top_unavailable";

void ApplyWindowIcon(SDL_Window* window)
{
#if defined(__linux__)
    const std::filesystem::path iconPath = FindRuntimeAsset("smu_icon.bmp");
    if (iconPath.empty()) {
        return;
    }

    SDL_Surface* icon = SDL_LoadBMP(iconPath.string().c_str());
    if (!icon) {
        LogWarning(std::string("Failed to load SMU window icon from ") + iconPath.string() + ": " + SDL_GetError(),
            kWindowIconWarningId, true);
        return;
    }

    if (!SDL_SetWindowIcon(window, icon)) {
    }
    SDL_DestroySurface(icon);
#else
    (void)window;
#endif
}

void UpdateWindowMetrics(SDL_Window* window)
{
    auto& state = smu::core::GetAppState();

    int windowWidth = 0;
    int windowHeight = 0;
    if (SDL_GetWindowSize(window, &windowWidth, &windowHeight) && windowWidth > 0 && windowHeight > 0) {
        state.screenWidth = windowWidth;
        state.screenHeight = windowHeight;
    }

    int pixelWidth = 0;
    int pixelHeight = 0;
    if (SDL_GetWindowSizeInPixels(window, &pixelWidth, &pixelHeight) && pixelWidth > 0 && pixelHeight > 0) {
        state.rawWindowWidth = pixelWidth;
        state.rawWindowHeight = pixelHeight;
    } else if (windowWidth > 0 && windowHeight > 0) {
        state.rawWindowWidth = windowWidth;
        state.rawWindowHeight = windowHeight;
    }

    int windowPosX = 0;
    int windowPosY = 0;
    if (SDL_GetWindowPosition(window, &windowPosX, &windowPosY)) {
        state.windowPosX = windowPosX;
        state.windowPosY = windowPosY;
    }
}

WindowSize GetNativeMinimumWindowSize(SDL_Window* window)
{
    (void)window;
    return kMinimumRenderSize;
}

void ApplyWindowMinimumSize(SDL_Window* window)
{
    const WindowSize minimumSize = GetNativeMinimumWindowSize(window);
    SDL_SetWindowMinimumSize(window, minimumSize.width, minimumSize.height);
}

#if defined(_WIN32)
void ApplyDarkTitleBar(SDL_Window* window)
{
    SDL_PropertiesID props = SDL_GetWindowProperties(window);
    if (!props) {
        LogWarning("SDL window properties were unavailable; skipping native dark title bar setup.",
            kNativeDarkTitleBarWarningId, true);
        return;
    }

    HWND hwnd = static_cast<HWND>(SDL_GetPointerProperty(props, SDL_PROP_WINDOW_WIN32_HWND_POINTER, nullptr));
    if (!hwnd) {
        LogWarning("SDL window HWND was unavailable; skipping native dark title bar setup.",
            kNativeDarkTitleBarWarningId, true);
        return;
    }

    const BOOL darkModeEnabled = TRUE;
    HRESULT darkModeResult = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &darkModeEnabled, sizeof(darkModeEnabled));
    if (FAILED(darkModeResult)) {
        LogWarning("Failed to apply immersive dark mode to the native title bar.",
            kNativeDarkTitleBarWarningId, true);
    }

    const COLORREF captionColor = RGB(0, 0, 0);
    HRESULT captionResult = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &captionColor, sizeof(captionColor));
    if (FAILED(captionResult)) {
        LogWarning("Failed to apply native title bar caption color.",
            kNativeDarkTitleBarWarningId, true);
    }
}
#endif

void ClampInitialWindowSizeToDisplay()
{
    auto& state = smu::core::GetAppState();
    state.screenWidth = std::clamp(state.screenWidth, 1, kMaximumInitialWindowWidth);
    state.screenHeight = std::clamp(state.screenHeight, 1, kMaximumInitialWindowHeight);

    const SDL_DisplayID displayId = SDL_GetPrimaryDisplay();
    SDL_Rect usableBounds{};
    if (displayId == 0 || !SDL_GetDisplayUsableBounds(displayId, &usableBounds)) {
        return;
    }

    int usableWidth = usableBounds.w;
    int usableHeight = usableBounds.h;

    if (usableWidth > 1) {
        state.screenWidth = std::min(state.screenWidth, std::max(1, usableWidth - 80));
    }
    if (usableHeight > 1) {
        state.screenHeight = std::min(state.screenHeight, std::max(1, usableHeight - 100));
    }
}

} // namespace

bool SetWorkingDirectoryToExecutablePath()
{
    const std::filesystem::path executableDirectory = GetExecutableDirectory();
    if (executableDirectory.empty()) {
        return false;
    }

    std::error_code ec;
    std::filesystem::current_path(executableDirectory, ec);
    return !ec;
}

int RunSharedApp(AppContext& context, const AppMainConfig& config)
{
    auto& state = smu::core::GetAppState();
    smu::core::ResetRuntimeAppFlags();
    InitializeSharedThemeSystem();
    InitializeSharedProfiles();

    if (state.screenWidth <= 0 || state.screenWidth > kMaximumInitialWindowWidth) {
        state.screenWidth = config.defaultWidth;
    }
    if (state.screenHeight <= 0 || state.screenHeight > kMaximumInitialWindowHeight) {
        state.screenHeight = config.defaultHeight;
    }

    SDL_SetMainReady();
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        LogCritical(std::string("Failed SDL initialization: ") + SDL_GetError());
        return 1;
    }

#if defined(__APPLE__)
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_FLAGS, SDL_GL_CONTEXT_FORWARD_COMPATIBLE_FLAG);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_CORE);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 3);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 2);
#else
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_COMPATIBILITY);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 3);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 0);
#endif
    SDL_GL_SetAttribute(SDL_GL_DOUBLEBUFFER, 1);
    SDL_GL_SetAttribute(SDL_GL_DEPTH_SIZE, 24);
    SDL_GL_SetAttribute(SDL_GL_STENCIL_SIZE, 8);

    SDL_WindowFlags windowFlags = SDL_WINDOW_OPENGL | SDL_WINDOW_RESIZABLE | SDL_WINDOW_HIDDEN;
#if defined(__APPLE__)
    windowFlags |= SDL_WINDOW_HIGH_PIXEL_DENSITY;
#endif
    ClampInitialWindowSizeToDisplay();

    SDL_Window* window = SDL_CreateWindow(config.title, state.screenWidth, state.screenHeight, windowFlags);
    if (!window) {
        LogCritical(std::string("Failed SDL window creation: ") + SDL_GetError());
        SDL_Quit();
        return 1;
    }

    ApplyWindowMinimumSize(window);
    ApplyWindowIcon(window);
    if (state.windowPosX != 0 || state.windowPosY != 0) {
        SDL_SetWindowPosition(window, state.windowPosX, state.windowPosY);
    }
#if defined(_WIN32)
    ApplyDarkTitleBar(window);
#endif

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
        LogWarning("SDL window opacity could not be applied on this platform.",
            kWindowOpacityWarningId, true);
    }
    if (state.alwaysOnTop && !SDL_SetWindowAlwaysOnTop(window, true)) {
        LogWarning("SDL always-on-top could not be applied on this platform.",
            kWindowAlwaysOnTopWarningId, true);
    }

    SDL_ShowWindow(window);

    context.setAlwaysOnTop = [window](bool enabled) {
        return SDL_SetWindowAlwaysOnTop(window, enabled);
    };
    context.setWindowOpacityPercent = [window](float opacityPercent) {
        const float opacity = std::clamp(opacityPercent / 100.0f, 0.2f, 1.0f);
        return SDL_SetWindowOpacity(window, opacity);
    };
    context.openExternalUrl = [](const char* url) {
        if (url && url[0] != '\0') {
            SDL_OpenURL(url);
        }
    };

    // Create ImGui context on main thread, but initialize backends and run the render loop
    // on a dedicated render thread that owns the GL context. The main thread will poll
    // events and forward them to ImGui once the backend is ready.
    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGuiIO& io = ImGui::GetIO();
    io.IniFilename = nullptr;
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
    SetupSharedFontsAndStyle(io);

#if defined(__APPLE__)
    SDL_GL_SetSwapInterval(1);
    if (!ImGui_ImplSDL3_InitForOpenGL(window, glContext)) {
        LogCritical("Failed ImGui initialization: SDL3 backend initialization failed.");
    }
    if (!ImGui_ImplOpenGL3_Init("#version 150")) {
        LogCritical("Failed OpenGL initialization: ImGui OpenGL backend initialization failed.");
    }
    LoadMacroTutorialTextures();

    while (state.running.load(std::memory_order_acquire) && !state.done.load(std::memory_order_acquire)) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            ImGui_ImplSDL3_ProcessEvent(&event);
            if (event.type == SDL_EVENT_QUIT || event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED) {
                state.done.store(true, std::memory_order_release);
                state.running.store(false, std::memory_order_release);
                Globals::done.store(true, std::memory_order_release);
                Globals::running.store(false, std::memory_order_release);
            }
            if (event.type == SDL_EVENT_WINDOW_RESIZED ||
                event.type == SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED ||
                event.type == SDL_EVENT_WINDOW_DISPLAY_SCALE_CHANGED ||
                event.type == SDL_EVENT_WINDOW_MOVED) {
                if (event.type == SDL_EVENT_WINDOW_DISPLAY_SCALE_CHANGED ||
                    event.type == SDL_EVENT_WINDOW_MOVED) {
                    ApplyWindowMinimumSize(window);
                }
                UpdateWindowMetrics(window);
            }
        }

        if (!state.running.load(std::memory_order_acquire) || state.done.load(std::memory_order_acquire)) {
            break;
        }

        ImGui_ImplOpenGL3_NewFrame();
        ImGui_ImplSDL3_NewFrame();
        ImGui::NewFrame();
        RenderAppUi(context);
        ImGui::Render();

        glViewport(0, 0, std::max(1, state.rawWindowWidth), std::max(1, state.rawWindowHeight));
        glClearColor(0.08f, 0.09f, 0.10f, 1.0f);
        glClear(GL_COLOR_BUFFER_BIT);
        ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
        SDL_GL_SwapWindow(window);
    }

    UnloadMacroTutorialTextures();
    ImGui_ImplOpenGL3_Shutdown();
    ImGui_ImplSDL3_Shutdown();
    UpdateWindowMetrics(window);
    ShutdownSharedProfiles();
    ResetFloatingUiWindowState();
    ImGui::DestroyContext();
    SDL_GL_DestroyContext(glContext);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
#else
    std::atomic<bool> renderRunning{true};
    std::atomic<bool> backendReady{false};
    std::atomic<int> renderWidth{state.rawWindowWidth > 0 ? state.rawWindowWidth : state.screenWidth};
    std::atomic<int> renderHeight{state.rawWindowHeight > 0 ? state.rawWindowHeight : state.screenHeight};
    std::atomic<std::uint64_t> redrawGeneration{1};
    std::atomic<std::int64_t> activeUntilTicks{FrameTimePointToTicks(FrameClock::now())};
    std::mutex backendMutex;
    std::condition_variable backendCv;
    std::mutex framePacingMutex;
    std::mutex imguiMutex;
    std::condition_variable framePacingCv;
    SDL_GLContext renderGlContext = nullptr;

    std::thread renderThread([&]() {
        renderGlContext = SDL_GL_CreateContext(window);
        if (!renderGlContext) {
            LogCritical(std::string("Failed OpenGL initialization: SDL_GL_CreateContext failed: ") + SDL_GetError());
            backendReady.store(true);
            backendCv.notify_one();
            return;
        }

        if (!SDL_GL_MakeCurrent(window, renderGlContext)) {
            LogCritical(std::string("Failed OpenGL initialization: SDL_GL_MakeCurrent failed: ") + SDL_GetError());
            SDL_GL_DestroyContext(renderGlContext);
            renderGlContext = nullptr;
            backendReady.store(true);
            backendCv.notify_one();
            return;
        }

        SDL_GL_SetSwapInterval(0);

        if (!ImGui_ImplSDL3_InitForOpenGL(window, renderGlContext)) {
            LogCritical("Failed ImGui initialization: SDL3 backend initialization failed.");
        }
#if defined(__APPLE__)
        constexpr const char* glslVersion = "#version 150";
#else
        constexpr const char* glslVersion = "#version 130";
#endif
        if (!ImGui_ImplOpenGL3_Init(glslVersion)) {
            LogCritical("Failed OpenGL initialization: ImGui OpenGL backend initialization failed.");
        }
        LoadMacroTutorialTextures();

        backendReady.store(true);
        backendCv.notify_one();

        constexpr int kActiveFps = 60;
        constexpr int kIdleFps = 8;
        constexpr auto inputBurstDuration = std::chrono::milliseconds(250);
        const auto activeFrameDuration = std::chrono::duration_cast<FrameClock::duration>(
            std::chrono::duration<double>(1.0 / static_cast<double>(kActiveFps)));
        const auto idleFrameDuration = std::chrono::duration_cast<FrameClock::duration>(
            std::chrono::duration<double>(1.0 / static_cast<double>(kIdleFps)));
        auto nextActiveFrameTime = FrameClock::now();
        auto nextIdleFrameTime = nextActiveFrameTime;
        std::uint64_t handledRedrawGeneration = 0;

        auto loadActiveUntil = [&]() {
            return FrameTimePointFromTicks(activeUntilTicks.load(std::memory_order_acquire));
        };

        while (renderRunning.load() && state.running.load(std::memory_order_acquire) && !state.done.load(std::memory_order_acquire)) {
            const std::uint64_t observedRedrawGeneration = redrawGeneration.load(std::memory_order_acquire);
            const auto now = FrameClock::now();
            const auto activeUntil = loadActiveUntil();
            const bool redrawRequested = observedRedrawGeneration != handledRedrawGeneration;
            const bool activeFrameDue = now < activeUntil && now >= nextActiveFrameTime;
            const bool idleFrameDue = now >= nextIdleFrameTime;

            if (!redrawRequested && !activeFrameDue && !idleFrameDue) {
                const auto wakeDeadline = (now < activeUntil)
                    ? std::min(nextActiveFrameTime, nextIdleFrameTime)
                    : nextIdleFrameTime;

                std::unique_lock<std::mutex> lk(framePacingMutex);
                framePacingCv.wait_until(lk, wakeDeadline, [&]() {
                    return !renderRunning.load(std::memory_order_acquire) ||
                        !state.running.load(std::memory_order_acquire) ||
                        state.done.load(std::memory_order_acquire) ||
                        redrawGeneration.load(std::memory_order_acquire) != handledRedrawGeneration;
                });
                continue;
            }

            const int w = std::max(1, renderWidth.load());
            const int h = std::max(1, renderHeight.load());
            glViewport(0, 0, w, h);
            glClearColor(0.08f, 0.09f, 0.10f, 1.0f);
            glClear(GL_COLOR_BUFFER_BIT);

            bool uiInteractionActive = false;
            {
                std::lock_guard<std::mutex> imguiLock(imguiMutex);
                ImGui_ImplOpenGL3_NewFrame();
                ImGui_ImplSDL3_NewFrame();
                ImGui::NewFrame();

                RenderAppUi(context);

                const ImGuiIO& frameIo = ImGui::GetIO();
                const bool mouseButtonDown = frameIo.MouseDown[0] || frameIo.MouseDown[1] || frameIo.MouseDown[2] ||
                    frameIo.MouseDown[3] || frameIo.MouseDown[4];
                uiInteractionActive = mouseButtonDown || ImGui::IsAnyItemActive();

                ImGui::Render();
                ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
            }
            SDL_GL_SwapWindow(window);

            handledRedrawGeneration = observedRedrawGeneration;
            const auto nowAfterRender = FrameClock::now();
            if (uiInteractionActive) {
                ExtendAtomicDeadline(activeUntilTicks, nowAfterRender + inputBurstDuration);
            }
            nextActiveFrameTime = nowAfterRender + activeFrameDuration;
            nextIdleFrameTime = nowAfterRender + idleFrameDuration;
        }

        // Shutdown GL/ImGui resources on the render thread that owns the context.
        UnloadMacroTutorialTextures();
        ImGui_ImplOpenGL3_Shutdown();
        ImGui_ImplSDL3_Shutdown();
        // Make no context current before destroying
        SDL_GL_MakeCurrent(window, nullptr);
        if (renderGlContext) {
            SDL_GL_DestroyContext(renderGlContext);
            renderGlContext = nullptr;
        }
    });

    // Wait for backend to be ready before processing events / forwarding to ImGui.
    {
        std::unique_lock<std::mutex> lk(backendMutex);
        backendCv.wait(lk, [&]() { return backendReady.load() || !state.running.load(); });
    }

    constexpr auto inputBurstDuration = std::chrono::milliseconds(250);
    auto requestActiveRedraw = [&]() {
        redrawGeneration.fetch_add(1, std::memory_order_release);
        ExtendAtomicDeadline(activeUntilTicks, FrameClock::now() + inputBurstDuration);
        framePacingCv.notify_one();
    };

    auto processEvent = [&](const SDL_Event& event) {
        if (event.type == SDL_EVENT_QUIT || event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED) {
            state.done.store(true, std::memory_order_release);
            state.running.store(false, std::memory_order_release);
            Globals::done.store(true, std::memory_order_release);
            Globals::running.store(false, std::memory_order_release);
#if defined(_WIN32)
            // Destroy the Win32 overlay promptly so it doesn't linger if shutdown is delayed.
            smu::platform::windows::CleanupLagswitchOverlay();
#endif
            return true;
        }

        if (event.type == SDL_EVENT_WINDOW_RESIZED ||
            event.type == SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED ||
            event.type == SDL_EVENT_WINDOW_DISPLAY_SCALE_CHANGED ||
            event.type == SDL_EVENT_WINDOW_MOVED) {
            if (event.type == SDL_EVENT_WINDOW_DISPLAY_SCALE_CHANGED ||
                event.type == SDL_EVENT_WINDOW_MOVED) {
                ApplyWindowMinimumSize(window);
            }
            UpdateWindowMetrics(window);
            renderWidth.store(state.rawWindowWidth > 0 ? state.rawWindowWidth : state.screenWidth);
            renderHeight.store(state.rawWindowHeight > 0 ? state.rawWindowHeight : state.screenHeight);
        }

        if (event.type == SDL_EVENT_MOUSE_WHEEL) {
            return false;
        }

        {
            std::lock_guard<std::mutex> imguiLock(imguiMutex);
            // Forward non-wheel input to ImGui while holding the shared context lock.
            ImGui_ImplSDL3_ProcessEvent(&event);
        }
        requestActiveRedraw();
        return false;
    };

    // Event loop runs on the main thread; the render thread wakes when new work arrives.
    while (state.running.load(std::memory_order_acquire) && !state.done.load(std::memory_order_acquire)) {
        SDL_Event event;
        if (!SDL_WaitEventTimeout(&event, 1000)) {
            continue;
        }

        bool quitRequested = false;
        bool sawWheelInput = false;
        bool sawOtherInput = false;
        float wheelX = 0.0f;
        float wheelY = 0.0f;

        auto flushWheel = [&]() {
            if (!sawWheelInput) {
                return;
            }
            std::lock_guard<std::mutex> imguiLock(imguiMutex);
            ImGuiIO& io = ImGui::GetIO();
            io.AddMouseSourceEvent(ImGuiMouseSource_Mouse);
            io.AddMouseWheelEvent(wheelX, wheelY);
            sawWheelInput = false;
            wheelX = 0.0f;
            wheelY = 0.0f;
        };

        auto handleEvent = [&](const SDL_Event& currentEvent) {
            if (currentEvent.type == SDL_EVENT_MOUSE_WHEEL) {
                wheelX += -currentEvent.wheel.x;
                wheelY += currentEvent.wheel.y;
                sawWheelInput = true;
                return processEvent(currentEvent);
            }
            sawOtherInput = true;
            flushWheel();
            return processEvent(currentEvent);
        };

        quitRequested = handleEvent(event);
        while (SDL_PollEvent(&event)) {
            quitRequested = handleEvent(event) || quitRequested;
        }
        flushWheel();
        if (sawWheelInput || sawOtherInput) {
            requestActiveRedraw();
        }
        if (quitRequested) {
            break;
        }
    }

    // Signal render thread to exit and join.
    renderRunning.store(false);
    framePacingCv.notify_one();
    if (renderThread.joinable()) {
        renderThread.join();
    }

    UpdateWindowMetrics(window);
    ShutdownSharedProfiles();
    ResetFloatingUiWindowState();

    // ImGui context was created on main thread; destroy it here after render thread cleaned up backends.
    ImGui::DestroyContext();
    SDL_GL_DestroyContext(glContext);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
#endif
}

} // namespace smu::app
