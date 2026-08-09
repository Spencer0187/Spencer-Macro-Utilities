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
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <memory>
#include <string>
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
#include "../platform/macos/termination_handler.h"
#endif

namespace smu::app {
namespace {

using FrameClock = std::chrono::steady_clock;

struct WindowSize {
    int width = 0;
    int height = 0;
};

constexpr WindowSize kMinimumRenderSize{1180, 700};
constexpr int kMaximumInitialWindowWidth = 3840;
constexpr int kMaximumInitialWindowHeight = 2160;

#if defined(_WIN32)
#ifdef WM_COPYGLOBALDATA
constexpr UINT kWmCopyGlobalData = WM_COPYGLOBALDATA;
#else
constexpr UINT kWmCopyGlobalData = 0x0049;
#endif
#ifdef MSGFLT_ALLOW
constexpr DWORD kMsgfltAllow = MSGFLT_ALLOW;
#else
constexpr DWORD kMsgfltAllow = 1;
#endif
#endif

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

#if defined(__linux__)
bool IsWaylandSessionEnvironment()
{
    const char* waylandDisplay = std::getenv("WAYLAND_DISPLAY");
    if (waylandDisplay && waylandDisplay[0] != '\0') {
        return true;
    }

    const char* sessionType = std::getenv("XDG_SESSION_TYPE");
    return sessionType && SDL_strcasecmp(sessionType, "wayland") == 0;
}

void PreferNativeWaylandWhenAvailable()
{
    // Preserve SDL_VIDEO_DRIVER/SDL_VIDEODRIVER and any programmatic hint so
    // users can still explicitly select a backend. Supplying an ordered list
    // bypasses SDL's automatic fifo-v1 performance heuristic, which otherwise
    // chooses XWayland on Wayland compositors that do not expose that protocol.
    const char* configuredDriver = SDL_GetHint(SDL_HINT_VIDEO_DRIVER);
    if (IsWaylandSessionEnvironment() && (!configuredDriver || configuredDriver[0] == '\0')) {
        SDL_SetHint(SDL_HINT_VIDEO_DRIVER, "wayland,x11");
    }
}
#endif

bool IsCurrentSdlVideoDriver(const char* expected)
{
    const char* currentDriver = SDL_GetCurrentVideoDriver();
    return currentDriver && std::strcmp(currentDriver, expected) == 0;
}

struct WindowOpacityController {
    SDL_Window* window = nullptr;
    bool nativeWayland = false;
    bool canUseFramebufferAlpha = false;
    bool useFramebufferAlpha = false;
    bool loggedFramebufferFallback = false;
    float opacity = 1.0f;

    bool apply(float opacityPercent)
    {
        opacity = std::clamp(opacityPercent / 100.0f, 0.2f, 1.0f);
        if (SDL_SetWindowOpacity(window, opacity)) {
            useFramebufferAlpha = false;
            return true;
        }

        if (!nativeWayland || !canUseFramebufferAlpha) {
            return false;
        }

        // Core Wayland compositing supports alpha-bearing surface buffers even
        // when the optional wp_alpha_modifier_v1 protocol is absent. The render
        // loop supplies uniform framebuffer alpha for that portable fallback.
        useFramebufferAlpha = true;
        if (!loggedFramebufferFallback) {
            const char* error = SDL_GetError();
            LogInfo(std::string("Wayland compositor-side window opacity is unavailable; ") +
                "using framebuffer alpha instead" +
                ((error && error[0] != '\0') ? std::string(": ") + error : std::string(".")));
            loggedFramebufferFallback = true;
        }
        SDL_ClearError();
        return true;
    }

    float framebufferAlpha() const
    {
        return useFramebufferAlpha ? opacity : 1.0f;
    }
};

void SetFramebufferAlpha(float alpha)
{
    GLboolean previousColorMask[4] = {GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE};
    GLfloat previousClearColor[4] = {};
    const GLboolean scissorWasEnabled = glIsEnabled(GL_SCISSOR_TEST);
    glGetBooleanv(GL_COLOR_WRITEMASK, previousColorMask);
    glGetFloatv(GL_COLOR_CLEAR_VALUE, previousClearColor);

    glDisable(GL_SCISSOR_TEST);
    glColorMask(GL_FALSE, GL_FALSE, GL_FALSE, GL_TRUE);
    glClearColor(0.0f, 0.0f, 0.0f, std::clamp(alpha, 0.0f, 1.0f));
    glClear(GL_COLOR_BUFFER_BIT);

    glColorMask(previousColorMask[0], previousColorMask[1], previousColorMask[2], previousColorMask[3]);
    glClearColor(previousClearColor[0], previousClearColor[1], previousClearColor[2], previousClearColor[3]);
    if (scissorWasEnabled) {
        glEnable(GL_SCISSOR_TEST);
    }
}

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
    WindowSize minimumSize = kMinimumRenderSize;
#if defined(__APPLE__)
    minimumSize.width += 40;
#endif
    return minimumSize;
}

void ApplyWindowMinimumSize(SDL_Window* window)
{
    const WindowSize minimumSize = GetNativeMinimumWindowSize(window);
    SDL_SetWindowMinimumSize(window, minimumSize.width, minimumSize.height);
}

#if defined(_WIN32)
void AllowElevatedWindowFileDrops(SDL_Window* window)
{
    SDL_PropertiesID props = SDL_GetWindowProperties(window);
    HWND hwnd = static_cast<HWND>(SDL_GetPointerProperty(props, SDL_PROP_WINDOW_WIN32_HWND_POINTER, nullptr));
    if (!hwnd) {
        LogWarning("Could not find Win32 window handle for drag-and-drop message filter.");
        return;
    }

    using ChangeWindowMessageFilterExFn = BOOL(WINAPI*)(HWND, UINT, DWORD, void*);
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    auto changeMessageFilter = user32
        ? reinterpret_cast<ChangeWindowMessageFilterExFn>(GetProcAddress(user32, "ChangeWindowMessageFilterEx"))
        : nullptr;
    if (!changeMessageFilter) {
        LogWarning("ChangeWindowMessageFilterEx is unavailable; elevated Windows drag-and-drop may be blocked.");
        return;
    }

    const std::array<UINT, 3> dropMessages = {WM_DROPFILES, WM_COPYDATA, kWmCopyGlobalData};
    for (UINT message : dropMessages) {
        if (!changeMessageFilter(hwnd, message, kMsgfltAllow, nullptr)) {
            LogWarning("Could not allow elevated Windows drag-and-drop message " + std::to_string(message) + ".");
        }
    }
}

std::filesystem::path Utf8PathFromDropData(const char* path)
{
    if (!path || path[0] == '\0') {
        return {};
    }

    const int requiredChars = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, nullptr, 0);
    if (requiredChars <= 0) {
        return std::filesystem::path(path);
    }

    std::wstring widePath(static_cast<std::size_t>(requiredChars), L'\0');
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, widePath.data(), requiredChars) <= 0) {
        return std::filesystem::path(path);
    }
    widePath.resize(static_cast<std::size_t>(requiredChars - 1));
    return std::filesystem::path(widePath);
}
#else
std::filesystem::path Utf8PathFromDropData(const char* path)
{
    return path ? std::filesystem::path(path) : std::filesystem::path();
}
#endif

bool HandleDroppedFileEvent(const SDL_Event& event)
{
    if (event.type != SDL_EVENT_DROP_FILE || !event.drop.data || event.drop.data[0] == '\0') {
        return false;
    }
    return QueueDroppedScriptImport(Utf8PathFromDropData(event.drop.data));
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
#if defined(__linux__)
    PreferNativeWaylandWhenAvailable();
#endif
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        LogCritical(std::string("Failed SDL initialization: ") + SDL_GetError());
        return 1;
    }
    const bool nativeWayland = IsCurrentSdlVideoDriver("wayland");
    LogInfo(std::string("SDL video driver: ") +
        (SDL_GetCurrentVideoDriver() ? SDL_GetCurrentVideoDriver() : "unknown"));
    SDL_SetEventEnabled(SDL_EVENT_DROP_FILE, true);

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
    if (nativeWayland) {
        SDL_GL_SetAttribute(SDL_GL_ALPHA_SIZE, 8);
    }

    SDL_WindowFlags windowFlags = SDL_WINDOW_OPENGL | SDL_WINDOW_RESIZABLE | SDL_WINDOW_HIDDEN;
#if defined(__linux__)
    if (nativeWayland) {
        windowFlags |= SDL_WINDOW_TRANSPARENT;
    }
#endif
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
#if defined(__APPLE__)
    smu::platform::macos::InstallMacOSTerminationHandler();
#endif

    ApplyWindowMinimumSize(window);
    ApplyWindowIcon(window);
    if (state.windowPosX != 0 || state.windowPosY != 0) {
        SDL_SetWindowPosition(window, state.windowPosX, state.windowPosY);
    }
#if defined(_WIN32)
    ApplyDarkTitleBar(window);
    AllowElevatedWindowFileDrops(window);
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

    int framebufferAlphaBits = 0;
    const bool canUseWaylandFramebufferAlpha = nativeWayland &&
        SDL_GL_GetAttribute(SDL_GL_ALPHA_SIZE, &framebufferAlphaBits) && framebufferAlphaBits > 0;

    auto opacityController = std::make_shared<WindowOpacityController>();
    opacityController->window = window;
    opacityController->nativeWayland = nativeWayland;
    opacityController->canUseFramebufferAlpha = canUseWaylandFramebufferAlpha;
    if (!opacityController->apply(state.windowOpacityPercent)) {
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
    context.setWindowOpacityPercent = [opacityController](float opacityPercent) {
        return opacityController->apply(opacityPercent);
    };
    context.openExternalUrl = [](const char* url) {
        if (url && url[0] != '\0') {
            SDL_OpenURL(url);
        }
    };

    // SDL's OpenGL and window APIs are main-thread APIs. Keep the ImGui backends,
    // rendering, buffer swaps, and teardown on this same thread on every platform.
    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGuiIO& io = ImGui::GetIO();
    io.IniFilename = nullptr;
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
    SetupSharedFontsAndStyle(io);

#if defined(__APPLE__)
    SDL_GL_SetSwapInterval(1);
#endif
    if (!ImGui_ImplSDL3_InitForOpenGL(window, glContext)) {
        LogCritical("Failed ImGui initialization: SDL3 backend initialization failed.");
    }
#if defined(__APPLE__)
    if (!ImGui_ImplOpenGL3_Init("#version 150")) {
#else
    if (!ImGui_ImplOpenGL3_Init("#version 130")) {
#endif
        LogCritical("Failed OpenGL initialization: ImGui OpenGL backend initialization failed.");
    }
    LoadMacroTutorialTextures();

    constexpr int kActiveFps = 60;
    constexpr int kIdleFps = 8;
    constexpr auto inputBurstDuration = std::chrono::milliseconds(250);
    const auto activeFrameDuration = std::chrono::duration_cast<FrameClock::duration>(
        std::chrono::duration<double>(1.0 / static_cast<double>(kActiveFps)));
    const auto idleFrameDuration = std::chrono::duration_cast<FrameClock::duration>(
        std::chrono::duration<double>(1.0 / static_cast<double>(kIdleFps)));
    auto activeUntil = FrameClock::now();
    auto nextActiveFrameTime = activeUntil;
    auto nextIdleFrameTime = activeUntil;
    std::uint64_t redrawGeneration = 1;
    std::uint64_t handledRedrawGeneration = 0;

    auto requestActiveRedraw = [&]() {
        ++redrawGeneration;
        activeUntil = std::max(activeUntil, FrameClock::now() + inputBurstDuration);
    };

    auto processEvent = [&](const SDL_Event& event) {
#if defined(__APPLE__)
        ImGui_ImplSDL3_ProcessEvent(&event);
#else
        // Batch wheel events below to preserve the Linux scroll behavior from the
        // former event/render split. All other events go directly to ImGui here.
        if (event.type != SDL_EVENT_MOUSE_WHEEL) {
            ImGui_ImplSDL3_ProcessEvent(&event);
        }
#endif
        HandleDroppedFileEvent(event);
        if (event.type == SDL_EVENT_QUIT || event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED) {
            state.done.store(true, std::memory_order_release);
            state.running.store(false, std::memory_order_release);
            Globals::done.store(true, std::memory_order_release);
            Globals::running.store(false, std::memory_order_release);
#if defined(_WIN32)
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
        }
        return false;
    };

    auto handlePendingEvents = [&](const SDL_Event* firstEvent = nullptr) {
        bool quitRequested = false;
        bool sawEvent = false;
        bool sawWheelInput = false;
        float wheelX = 0.0f;
        float wheelY = 0.0f;
        SDL_Event event{};

        auto flushWheel = [&]() {
            if (!sawWheelInput) {
                return;
            }
            ImGuiIO& eventIo = ImGui::GetIO();
            eventIo.AddMouseSourceEvent(ImGuiMouseSource_Mouse);
            eventIo.AddMouseWheelEvent(wheelX, wheelY);
            sawWheelInput = false;
            wheelX = 0.0f;
            wheelY = 0.0f;
        };

        auto handleEvent = [&](const SDL_Event& currentEvent) {
            sawEvent = true;
#if !defined(__APPLE__)
            if (currentEvent.type == SDL_EVENT_MOUSE_WHEEL) {
                wheelX += -currentEvent.wheel.x;
                wheelY += currentEvent.wheel.y;
                sawWheelInput = true;
                return processEvent(currentEvent);
            }
#endif
            flushWheel();
            return processEvent(currentEvent);
        };

        if (firstEvent) {
            quitRequested = handleEvent(*firstEvent);
        }
        while (SDL_PollEvent(&event)) {
            quitRequested = handleEvent(event) || quitRequested;
        }
        flushWheel();
        if (sawEvent) {
            requestActiveRedraw();
        }
        return quitRequested;
    };

    while (state.running.load(std::memory_order_acquire) && !state.done.load(std::memory_order_acquire)) {
        const auto now = FrameClock::now();
        const bool redrawRequested = redrawGeneration != handledRedrawGeneration;
        const bool activeFrameDue = now < activeUntil && now >= nextActiveFrameTime;
        const bool idleFrameDue = now >= nextIdleFrameTime;

        bool quitRequested = false;
        if (!redrawRequested && !activeFrameDue && !idleFrameDue) {
            const auto wakeDeadline = (now < activeUntil)
                ? std::min(nextActiveFrameTime, nextIdleFrameTime)
                : nextIdleFrameTime;
            const auto waitDuration = std::chrono::ceil<std::chrono::milliseconds>(
                std::max(FrameClock::duration::zero(), wakeDeadline - now));
            const int timeoutMs = std::clamp(static_cast<int>(waitDuration.count()), 1, 1000);

            SDL_Event event{};
            if (SDL_WaitEventTimeout(&event, timeoutMs)) {
                quitRequested = handlePendingEvents(&event);
            }
        } else {
            quitRequested = handlePendingEvents();
        }

        if (quitRequested || !state.running.load(std::memory_order_acquire) || state.done.load(std::memory_order_acquire)) {
            break;
        }

        const auto renderNow = FrameClock::now();
        const bool renderRedrawRequested = redrawGeneration != handledRedrawGeneration;
        const bool renderActiveFrameDue = renderNow < activeUntil && renderNow >= nextActiveFrameTime;
        const bool renderIdleFrameDue = renderNow >= nextIdleFrameTime;
        if (!renderRedrawRequested && !renderActiveFrameDue && !renderIdleFrameDue) {
            continue;
        }

        ImGui_ImplOpenGL3_NewFrame();
        ImGui_ImplSDL3_NewFrame();
        ImGui::NewFrame();
        RenderAppUi(context);

        const ImGuiIO& frameIo = ImGui::GetIO();
        const bool mouseButtonDown = frameIo.MouseDown[0] || frameIo.MouseDown[1] || frameIo.MouseDown[2] ||
            frameIo.MouseDown[3] || frameIo.MouseDown[4];
        const bool uiInteractionActive = mouseButtonDown || ImGui::IsAnyItemActive();

        ImGui::Render();

        glViewport(0, 0, std::max(1, state.rawWindowWidth), std::max(1, state.rawWindowHeight));
        glClearColor(0.08f, 0.09f, 0.10f, 1.0f);
        glClear(GL_COLOR_BUFFER_BIT);
        ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
        if (nativeWayland) {
            // Keep the transparent surface uniformly opaque when SDL can use
            // wp_alpha_modifier_v1, or apply the requested opacity directly to
            // the buffer when that optional compositor protocol is unavailable.
            SetFramebufferAlpha(opacityController->framebufferAlpha());
        }
        SDL_GL_SwapWindow(window);

        handledRedrawGeneration = redrawGeneration;
        const auto nowAfterRender = FrameClock::now();
        if (uiInteractionActive) {
            activeUntil = std::max(activeUntil, nowAfterRender + inputBurstDuration);
        }
        nextActiveFrameTime = nowAfterRender + activeFrameDuration;
        nextIdleFrameTime = nowAfterRender + idleFrameDuration;
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
}

} // namespace smu::app
