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
set(SMU_PLATFORM_DIR "${CMAKE_CURRENT_SOURCE_DIR}/platform")

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
    add_custom_target(suspend
        COMMAND "${CMAKE_COMMAND}" -E echo
                "Linux scaffold only: add Linux platform sources before building suspend."
    )

    # Future Linux platform files should be added here once the Win32-heavy
    # application code is split behind platform interfaces. The existing helper
    # source is intentionally not wired into the app target by this scaffold:
    # "${SMU_RESOURCE_DIR}/Suspend_Input_Helper_Source/linux_helper.cpp"
else()
    message(FATAL_ERROR "Unsupported platform for the suspend target.")
endif()
