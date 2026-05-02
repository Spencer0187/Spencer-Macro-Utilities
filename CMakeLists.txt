cmake_minimum_required(VERSION 3.23)

cmake_policy(SET CMP0091 NEW)

project(SpencerMacroUtilities LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(SMU_VISUAL_STUDIO_DIR "${CMAKE_CURRENT_SOURCE_DIR}/visual studio")
set(SMU_IMGUI_DIR "${SMU_VISUAL_STUDIO_DIR}/imgui-files")
set(SMU_RESOURCE_DIR "${SMU_VISUAL_STUDIO_DIR}/Resource Files")
set(SMU_WINDIVERT_DIR "${SMU_VISUAL_STUDIO_DIR}/windivert-files")
set(SMU_THIRD_PARTY_DIR "${CMAKE_CURRENT_SOURCE_DIR}/third_party")
set(SMU_PLATFORM_DIR "${CMAKE_CURRENT_SOURCE_DIR}/platform")
set(SMU_APP_DIR "${CMAKE_CURRENT_SOURCE_DIR}/app")
set(SMU_CORE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/core")

option(SMU_USE_SDL_UI "Build the app GUI on SDL3 instead of the legacy Win32/WGL window backend." ON)

if(WIN32)
    enable_language(RC)

    set(SMU_WINDOWS_SOURCES
        "${SMU_IMGUI_DIR}/imgui.cpp"
        "${SMU_IMGUI_DIR}/imgui_demo.cpp"
        "${SMU_IMGUI_DIR}/imgui_draw.cpp"
        "${SMU_IMGUI_DIR}/imgui_impl_opengl3.cpp"
        "${SMU_IMGUI_DIR}/imgui_tables.cpp"
        "${SMU_IMGUI_DIR}/imgui_widgets.cpp"
        "${SMU_APP_DIR}/main_windows.cpp"
        "${SMU_VISUAL_STUDIO_DIR}/macroframework.cpp"
        "${SMU_VISUAL_STUDIO_DIR}/macros.cpp"
        "${SMU_VISUAL_STUDIO_DIR}/network_manager.cpp"
        "${SMU_VISUAL_STUDIO_DIR}/overlay.cpp"
        "${SMU_VISUAL_STUDIO_DIR}/profile_manager.cpp"
        "${SMU_PLATFORM_DIR}/input_backend.cpp"
        "${SMU_PLATFORM_DIR}/logging.cpp"
        "${SMU_PLATFORM_DIR}/network_backend.cpp"
        "${SMU_PLATFORM_DIR}/platform_capabilities.cpp"
        "${SMU_PLATFORM_DIR}/process_backend.cpp"
        "${SMU_PLATFORM_DIR}/window_backend.cpp"
        "${SMU_RESOURCE_DIR}/miniz.c"
        "${SMU_VISUAL_STUDIO_DIR}/theme_manager.cpp"
        "${SMU_VISUAL_STUDIO_DIR}/wine_compatibility_layer.cpp"
        "${SMU_VISUAL_STUDIO_DIR}/suspend1.rc"
    )

    if(SMU_USE_SDL_UI)
        list(APPEND SMU_WINDOWS_SOURCES "${SMU_IMGUI_DIR}/imgui_impl_sdl3.cpp")
    else()
        list(APPEND SMU_WINDOWS_SOURCES "${SMU_IMGUI_DIR}/imgui_impl_win32.cpp")
    endif()

    add_executable(suspend WIN32 ${SMU_WINDOWS_SOURCES})

    target_include_directories(suspend PRIVATE
        "${SMU_VISUAL_STUDIO_DIR}"
        "${SMU_IMGUI_DIR}"
        "${SMU_RESOURCE_DIR}"
        "${SMU_RESOURCE_DIR}/SDL3/include"
        "${SMU_WINDIVERT_DIR}"
    )

    target_compile_definitions(suspend PRIVATE
        UNICODE
        _UNICODE
        _CONSOLE
        "SMU_USE_SDL_UI=$<BOOL:${SMU_USE_SDL_UI}>"
        "$<$<CONFIG:Debug>:_DEBUG>"
        "$<$<NOT:$<CONFIG:Debug>>:NDEBUG>"
        SMU_USE_PLATFORM_ENTRYPOINT=1
    )

    set_property(TARGET suspend PROPERTY
        MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>"
    )

    if(MSVC)
        target_compile_options(suspend PRIVATE
            /W3
            /permissive-
            /Zc:__cplusplus
            "$<$<CONFIG:Release>:/GS->"
            "$<$<CONFIG:Release>:/Gw>"
        )

        target_link_options(suspend PRIVATE
            /SUBSYSTEM:WINDOWS
            "$<$<CONFIG:Debug>:/DEBUG>"
            "$<$<CONFIG:Release>:/OPT:REF>"
            "$<$<CONFIG:Release>:/OPT:ICF>"
        )
    endif()

    if(SMU_USE_SDL_UI)
        set(SMU_SDL3_STATIC_LIB "${SMU_RESOURCE_DIR}/SDL3/lib/SDL3-static.lib")
        if(EXISTS "${SMU_SDL3_STATIC_LIB}")
            target_link_libraries(suspend PRIVATE "${SMU_SDL3_STATIC_LIB}")
        else()
            message(WARNING "SDL3 static library was not found: ${SMU_SDL3_STATIC_LIB}")
        endif()
    endif()

    target_link_libraries(suspend PRIVATE
        opengl32
        user32
        gdi32
        shell32
        ole32
        oleaut32
        uuid
        advapi32
        winmm
        version
        setupapi
        cfgmgr32
        imm32
        shlwapi
        wininet
        Shcore
        dwmapi
        xinput
    )
elseif(UNIX AND NOT APPLE)
    find_package(OpenGL REQUIRED)
    find_package(Threads REQUIRED)
    find_package(X11 QUIET)

    set(SMU_VENDORED_SDL3_DIR "${SMU_THIRD_PARTY_DIR}/SDL")
    if(EXISTS "${SMU_VENDORED_SDL3_DIR}/CMakeLists.txt")
        message(STATUS "Using vendored SDL3 from ${SMU_VENDORED_SDL3_DIR}")

        set(SDL_SHARED ON CACHE BOOL "Build a shared version of the library" FORCE)
        set(SDL_STATIC OFF CACHE BOOL "Build a static version of the library" FORCE)
        set(SDL_TEST_LIBRARY OFF CACHE BOOL "Build the SDL3_test library" FORCE)
        set(SDL_TESTS OFF CACHE BOOL "Build the test directory" FORCE)
        set(SDL_EXAMPLES OFF CACHE BOOL "Build the examples directory" FORCE)
        set(SDL_INSTALL OFF CACHE BOOL "Enable installation of SDL3" FORCE)
        set(SDL_INSTALL_DOCS OFF CACHE BOOL "Install docs for SDL3" FORCE)
        set(SDL_INSTALL_TESTS OFF CACHE BOOL "Install test-cases" FORCE)
        set(SDL_UNINSTALL OFF CACHE BOOL "Enable uninstallation of SDL3" FORCE)

        add_subdirectory("${SMU_VENDORED_SDL3_DIR}" "${CMAKE_CURRENT_BINARY_DIR}/_deps/sdl3-build" EXCLUDE_FROM_ALL)
    else()
        find_package(SDL3 REQUIRED CONFIG)
    endif()

    set(SMU_LINUX_PLATFORM_SOURCES
        "${SMU_PLATFORM_DIR}/input_backend.cpp"
        "${SMU_PLATFORM_DIR}/logging.cpp"
        "${SMU_PLATFORM_DIR}/network_backend.cpp"
        "${SMU_PLATFORM_DIR}/platform_capabilities.cpp"
        "${SMU_PLATFORM_DIR}/process_backend.cpp"
        "${SMU_PLATFORM_DIR}/window_backend.cpp"
        "${SMU_PLATFORM_DIR}/linux/display_server.cpp"
        "${SMU_PLATFORM_DIR}/linux/foreground_x11.cpp"
        "${SMU_PLATFORM_DIR}/linux/input_evdev_uinput.cpp"
        "${SMU_PLATFORM_DIR}/linux/process_proc_cgroup.cpp"
    )

    add_library(smu_platform_linux STATIC ${SMU_LINUX_PLATFORM_SOURCES})

    target_include_directories(smu_platform_linux PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}"
        "${SMU_PLATFORM_DIR}"
        "${SMU_RESOURCE_DIR}"
    )

    target_link_libraries(smu_platform_linux PRIVATE Threads::Threads)

    if(X11_FOUND)
        target_compile_definitions(smu_platform_linux PRIVATE SMU_HAS_X11=1)
        target_link_libraries(smu_platform_linux PRIVATE X11::X11)
    else()
        target_compile_definitions(smu_platform_linux PRIVATE SMU_HAS_X11=0)
        message(WARNING "X11 development files were not found; Linux foreground detection will compile as unsupported.")
    endif()

    set(SMU_LINUX_APP_SOURCES
        "${SMU_IMGUI_DIR}/imgui.cpp"
        "${SMU_IMGUI_DIR}/imgui_demo.cpp"
        "${SMU_IMGUI_DIR}/imgui_draw.cpp"
        "${SMU_IMGUI_DIR}/imgui_impl_opengl3.cpp"
        "${SMU_IMGUI_DIR}/imgui_impl_sdl3.cpp"
        "${SMU_IMGUI_DIR}/imgui_tables.cpp"
        "${SMU_IMGUI_DIR}/imgui_widgets.cpp"
        "${SMU_APP_DIR}/app_context.cpp"
        "${SMU_APP_DIR}/app_main.cpp"
        "${SMU_APP_DIR}/app_ui.cpp"
        "${SMU_APP_DIR}/input_actions.cpp"
        "${SMU_APP_DIR}/main_linux.cpp"
        "${SMU_CORE_DIR}/app_state.cpp"
        "${SMU_CORE_DIR}/macro_state.cpp"
    )

    add_executable(suspend ${SMU_LINUX_APP_SOURCES})

    target_include_directories(suspend PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}"
        "${SMU_APP_DIR}"
        "${SMU_CORE_DIR}"
        "${SMU_IMGUI_DIR}"
    )

    if(TARGET SDL3::SDL3)
        set(SMU_SDL3_TARGET SDL3::SDL3)
    elseif(TARGET SDL3::SDL3-shared)
        set(SMU_SDL3_TARGET SDL3::SDL3-shared)
    else()
        message(FATAL_ERROR "SDL3 package was found, but no usable SDL3 imported target is available.")
    endif()

    target_link_libraries(suspend PRIVATE
        smu_platform_linux
        ${SMU_SDL3_TARGET}
        OpenGL::GL
        Threads::Threads
    )

    # The existing Wine helper remains intentionally unwired into the native
    # Linux backend target while the Win32-heavy app entry point is split:
    # "${SMU_RESOURCE_DIR}/Suspend_Input_Helper_Source/linux_helper.cpp"
else()
    message(FATAL_ERROR "Unsupported platform for the suspend target.")
endif()
