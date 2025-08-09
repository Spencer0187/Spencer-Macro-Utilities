cmake_minimum_required(VERSION 3.5)

project(imgui)

add_library(imgui)
target_sources(imgui PRIVATE 
	"${CMAKE_CURRENT_SOURCE_DIR}/libs/imgui/imgui.cpp"
	"${CMAKE_CURRENT_SOURCE_DIR}/libs/imgui/imgui_demo.cpp"
	"${CMAKE_CURRENT_SOURCE_DIR}/libs/imgui/imgui_draw.cpp"
	"${CMAKE_CURRENT_SOURCE_DIR}/libs/imgui/imgui_tables.cpp"
	"${CMAKE_CURRENT_SOURCE_DIR}/libs/imgui/imgui_widgets.cpp"
	"${CMAKE_CURRENT_SOURCE_DIR}/libs/imgui/backends/imgui_impl_dx11.cpp"
	"${CMAKE_CURRENT_SOURCE_DIR}/libs/imgui/backends/imgui_impl_win32.cpp"
)

target_include_directories(imgui PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/libs/imgui")

project(suspend)

set (CMAKE_CXX_STANDARD 23)
set (CMAKE_SYSTEM_VERSION 10.0.22621.0)

add_subdirectory(libs/miniz)

file(GLOB_RECURSE RESOURCE_FILES ${CMAKE_CURRENT_SOURCE_DIR}/res/*.rc)
file(GLOB_RECURSE SOURCE_FILES ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)

add_executable(suspend WIN32 ${SOURCE_FILES} ${RESOURCE_FILES})

if(MSVC)

	target_compile_definitions("${CMAKE_PROJECT_NAME}" PUBLIC _CRT_SECURE_NO_WARNINGS)

endif()

target_include_directories(suspend PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/include/)

add_link_options(
    $<$<NOT:$<CONFIG:DEBUG>>:/INCREMENTAL:NO>
)
target_link_libraries(suspend PRIVATE imgui miniz)