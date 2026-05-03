#pragma once
#include <string>
#include <cstring>
#include <fstream>
#include <stdio.h>
#include <filesystem>
#include <thread>
#include <chrono>
#include <iostream>
#include <vector>
#include <map>
#include <regex>

#include "json.hpp"
using json = nlohmann::json;

#ifdef _WIN32
    #define WIN32_LEAN_AND_MEAN
    #include <windows.h>
#else
    #include <sys/stat.h>
    #if defined(__linux__)
        #include <fcntl.h>
    #endif
#endif

inline const unsigned long long CAMFIX_PLACEIDS[1] = {
    4597361034,
};

//-- REGEX PATTERNS!! very cool yes yes yes
// Non-UDMUX: Connecting to <ip>:<port>
static const std::regex rcc_only_regex(
    R"(Connecting to ([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+):([0-9]+))"
);

// UDMUX: Connecting to UDMUX server <ip>:<port>, and RCC server <ip>:<port>
static const std::regex udmux_regex(
    R"(Connecting to UDMUX server ([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+):([0-9]+), and RCC server ([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+):([0-9]+))"
);

namespace fs = std::filesystem;

enum state {IN_GAME, IN_LUA_APP, OFFLINE, INVALID, UNCHANGED_FILE};

// Cross-platform file size calculation
inline long long calculate_file_size_stat(const char *filepath) {
    if (!filepath || filepath[0] == '\0') return -1;

#ifdef _WIN32
    WIN32_FILE_ATTRIBUTE_DATA file_info;
    if (GetFileAttributesExA(filepath, GetFileExInfoStandard, &file_info)) {
        LARGE_INTEGER size;
        size.HighPart = file_info.nFileSizeHigh;
        size.LowPart = file_info.nFileSizeLow;
        return size.QuadPart;
    }
    return -1;
#else
    #if defined(__linux__)
        struct statx stx;
        if (statx(AT_FDCWD, filepath, 0, STATX_SIZE, &stx) == 0) {
            return stx.stx_size;
        }
    #endif

    #if defined(_LARGEFILE64_SOURCE) || defined(__USE_LARGEFILE64)
        struct stat64 file_info;
        if (stat64(filepath, &file_info) == 0) {
            return file_info.st_size;
        }
    #else
        struct stat file_info;
        if (stat(filepath, &file_info) == 0) {
            return file_info.st_size;
        }
    #endif

    return -1;
#endif
}

// Cross-platform path separator
inline std::string get_path_separator() {
#ifdef _WIN32
    return "\\";
#else
    return "/";
#endif
}

inline namespace logzz {
    inline state current_state = OFFLINE;
    inline state last_state = UNCHANGED_FILE;
    inline std::string logs_folder_path;
    inline std::string local_storage_folder_path;
    inline unsigned long long current_place_ID = 0;
    inline unsigned long long current_universe_ID = 0;
    inline unsigned long long current_user_ID = 0;
    inline std::string current_username;
    inline std::string current_display_name;
    inline long long last_file_size = -1;
    inline unsigned int camfix_proofs;
    inline bool game_uses_camfix_final;
    inline int game_uses_camfix_percentage;
    inline std::map<unsigned long long, int> calculated_placeIDs;
    inline bool server_uses_udmux;
    inline std::string server_udmux_address;
    inline std::string server_rcc_address;
    inline std::string udmux_port;
    inline std::string rcc_port;

    inline state loop_handle() {
        logzz::last_state = logzz::current_state;

        // Validate logs folder path
        if (logs_folder_path.empty()) {
            current_state = INVALID;
            return INVALID;
        }

        // Check if directory exists
        std::error_code ec;
        if (!fs::exists(logs_folder_path, ec)) {
            current_state = INVALID;
            return INVALID;
        }

        if (ec) {
            current_state = INVALID;
            return INVALID;
        }

        // Check if it's actually a directory
        if (!fs::is_directory(logs_folder_path, ec)) {
            current_state = INVALID;
            return INVALID;
        }

        if (ec) {
            current_state = INVALID;
            return INVALID;
        }

        // Get most recent log file with error handling
        std::string most_recent_log_file;
        try {
            bool found_file = false;
            fs::file_time_type newest_time;

            for (const auto & entry : fs::directory_iterator(logs_folder_path, ec)) {
                if (ec) {
                    current_state = INVALID;
                    return INVALID;
                }

                if (entry.is_regular_file(ec) && !ec) {
                    auto file_time = entry.last_write_time(ec);
                    if (ec) continue;

                    if (!found_file || file_time > newest_time) {
                        most_recent_log_file = entry.path().string();
                        newest_time = file_time;
                        found_file = true;
                    }
                }
            }

            if (!found_file || most_recent_log_file.empty()) {
                current_state = OFFLINE;
                return OFFLINE;
            }
        } catch (const fs::filesystem_error& e) {
            printf("[logzz] Filesystem error: %s\n", e.what());
            current_state = INVALID;
            return INVALID;
        } catch (const std::exception& e) {
            printf("[logzz] Unexpected error: %s\n", e.what());
            current_state = INVALID;
            return INVALID;
        }

        // Check file size
        long long current_file_size = calculate_file_size_stat(most_recent_log_file.c_str());
        if (current_file_size < 0) {
            current_state = INVALID;
            return INVALID;
        }

        if (current_file_size == last_file_size) {
            return UNCHANGED_FILE;
        }

        last_file_size = current_file_size;

        // Open and read log file
        std::ifstream log_file(most_recent_log_file);
        if (!log_file.is_open()) {
            current_state = INVALID;
            return INVALID;
        }

        std::smatch connecting_to_match;

        std::string last_place_id_string;
        std::string last_universe_id_string;
        std::string last_udmux_server_address;
        std::string last_rcc_server_address;
        std::string last_udmux_port;
        std::string last_rcc_port;

        int last_universe_id_line = 0;
        int last_place_id_line = 0;
        int in_lua_app_line = 0;
        int left_roblox_line = 0;

        // Cam fix detection variables
        int thistoweruses_line = 0;
        int setpartcollisiongroup_line = 0;
        int clientobjects_line = 0;
        int localpartsscripterror_line = 0;
        int workspaceobby_line = 0;
        int towerword_line = 0;
        int playerscripts_line = 0;
        int connectingtoudmux_line = 0;
        int connectingtoline = 0;

        std::string current_line;
        int line = 0;

        try {
            while (std::getline(log_file, current_line)) {
                line++;

                // Parse joining line
                size_t join_pos = current_line.find("Joining");
                if (join_pos != std::string::npos) {
                    size_t placeid_index = join_pos + 58;
                    if (placeid_index < current_line.size()) {
                        size_t place_id_end = current_line.find(' ', placeid_index);
                        if (place_id_end != std::string::npos) {
                            last_place_id_string = current_line.substr(placeid_index, place_id_end - placeid_index);
                            last_place_id_line = line;
                        }
                    }
                }

                // Parse universe ID
                size_t gjlt_pos = current_line.find("FLog::GameJoinLoadTime");
                if (gjlt_pos != std::string::npos &&
                    current_line.find("Report game_join_loadtime") != std::string::npos)
                {
                    size_t univ_pos = current_line.find("universeid:");
                    if (univ_pos != std::string::npos) {
                        univ_pos += std::string("universeid:").size();
                        size_t end_pos = current_line.find_first_of(", ", univ_pos);
                        if (end_pos == std::string::npos) {
                            end_pos = current_line.size();
                        }
                        last_universe_id_string = current_line.substr(univ_pos, end_pos - univ_pos);
                        try {
                            current_universe_ID = std::stoull(last_universe_id_string);
                        } catch (...) {
                            current_universe_ID = 0;
                        }
                        last_universe_id_line = line;
                    }
                }

                // Parse state changes
                if (current_line.find("returnToLuaApp") != std::string::npos) {
                    in_lua_app_line = line;
                }
                if (current_line.find("setStage: (stage:None)") != std::string::npos) {
                    left_roblox_line = line;
                }

                // CAMFIX DETECTION
                if (current_line.find("This tower uses") != std::string::npos) {
                    thistoweruses_line = line;
                }
                if (current_line.find("Warning: SetPartCollisionGroup is deprecated") != std::string::npos) {
                    setpartcollisiongroup_line = line;
                }
                if (current_line.find("ClientParts") != std::string::npos ||
                    current_line.find("ClientObject") != std::string::npos ||
                    current_line.find("ClientSidedObject") != std::string::npos ||
                    current_line.find("ClientObjectScript") != std::string::npos) {
                    clientobjects_line = line;
                }
                if (current_line.find("LocalPartScript") != std::string::npos) {
                    localpartsscripterror_line = line;
                }
                if (current_line.find("PlayerScript") != std::string::npos) {
                    playerscripts_line = line;
                }
                if (current_line.find("Workspace.Obby") != std::string::npos) {
                    workspaceobby_line = line;
                }
                if (current_line.find("tower") != std::string::npos) {
                    towerword_line = line;
                }
                if (current_line.find("tower") != std::string::npos) {
                    towerword_line = line;
                }




                // server IP fetching
                if (current_line.find("Connecting to") != std::string::npos) {

                    // Case 1: UDMUX + RCC
                    if (std::regex_search(current_line, connecting_to_match, udmux_regex)) {

                        last_udmux_server_address = connecting_to_match[1].str(); // UDMUX IP
                        last_rcc_server_address   = connecting_to_match[3].str(); // RCC IP

                        // ports
                        last_udmux_port = connecting_to_match[2].str();
                        last_rcc_port = connecting_to_match[4].str();
                        connectingtoudmux_line = line;
                    }

                    // Case 2: RCC only (non-UDMUX)
                    else if (std::regex_search(current_line, connecting_to_match, rcc_only_regex)) {

                        last_udmux_server_address.clear(); // no UDMUX
                        last_rcc_server_address = connecting_to_match[1].str();

                        last_rcc_port = connecting_to_match[2].str();
                        connectingtoline = line;
                    }
                }


            }
        } catch (const std::exception& e) {
            printf("[logzz] Error reading log file: %s\n", e.what());
            log_file.close();
            current_state = INVALID;
            return INVALID;
        }

        log_file.close();

        // Determine current state
        if (left_roblox_line > last_place_id_line && left_roblox_line > in_lua_app_line) {
            current_state = OFFLINE;
        } else if (in_lua_app_line > last_place_id_line) {
            current_state = IN_LUA_APP;
        } else if (in_lua_app_line < last_place_id_line) {
            current_state = IN_GAME;
            if (!last_place_id_string.empty()) {
                try {
                    current_place_ID = std::stoull(last_place_id_string);
                } catch (const std::exception& e) {
                    printf("[logzz] Error parsing place ID: %s\n", e.what());
                    current_place_ID = 0;
                }
            } else {
                current_place_ID = 0;
            }
        } else {
            current_state = OFFLINE;
        }

        // CAMFIX DETECTION
        if (current_state == IN_GAME && current_place_ID > 0) {
            if (calculated_placeIDs.find(current_place_ID) != calculated_placeIDs.end() &&
                calculated_placeIDs[current_place_ID] > 0) {
                return current_state;
            }

            int score = 0;

            if (thistoweruses_line > last_place_id_line) score += 200;
            if (setpartcollisiongroup_line > last_place_id_line) score += 20;
            if (clientobjects_line > last_place_id_line) score += 100;
            if (localpartsscripterror_line > last_place_id_line) score += 10;
            if (workspaceobby_line > last_place_id_line) score += 20;
            if (towerword_line > last_place_id_line) score += 20;
            if (playerscripts_line > last_place_id_line) score += 30;

            calculated_placeIDs[current_place_ID] = (int)((score / 300.0f) * 100);
        }

        // ips
        if (connectingtoudmux_line > last_place_id_line) {
            server_uses_udmux = true;
            server_udmux_address = last_udmux_server_address;
            server_rcc_address = last_rcc_server_address;
            rcc_port = last_rcc_port;
            udmux_port = last_udmux_port;
        } else if (connectingtoline > last_place_id_line) {
            server_uses_udmux = false;
            server_rcc_address = last_rcc_server_address;
            rcc_port = last_rcc_port;
        }
        return current_state;
    }

    // Returns the place name associated with a universe ID.
    // Cross-platform. Reads local_storage_folder_path/appStorage.json.
    inline std::string find_name_for_universe(uint64_t target_universe_id)
    {
        std::string file_path = local_storage_folder_path + get_path_separator() + "appStorage.json";
        std::ifstream f(file_path);
        if (!f.is_open()) {
            return "";
        }

        json root;
        try {
            f >> root;
        } catch (...) {
            return "";
        }

        if (!root.contains("DiscoveryClientFallbackCache")) return "";

        // Get the string value
        std::string cache_str = root["DiscoveryClientFallbackCache"].get<std::string>();

        // Parse the string as JSON
        json cache_json;
        try {
            cache_json = json::parse(cache_str);
        } catch (...) {
            return "";
        }

        // Navigate into "data" -> "contentMetadata" -> "Game"
        if (!cache_json.contains("data")) return "";
        const auto& data = cache_json["data"];
        if (!data.contains("contentMetadata")) return "";
        const auto& contentMetadata = data["contentMetadata"];
        if (!contentMetadata.contains("Game")) return "";
        const auto& games = contentMetadata["Game"];

        // Keys are universe IDs as strings
        std::string key = std::to_string(target_universe_id);
        if (!games.contains(key)) return "";

        const auto& entry = games[key];
        if (!entry.contains("name") || !entry["name"].is_string()) return "";

        return entry["name"].get<std::string>();
    }

    // Loads user information from appStorage.json
    // Updates current_user_ID, current_username, and current_display_name
    inline bool load_user_info()
    {
        std::string file_path = local_storage_folder_path + get_path_separator() + "appStorage.json";
        std::ifstream f(file_path);
        if (!f.is_open()) {
            printf("[logzz] Could not open appStorage.json\n");
            return false;
        }

        json root;
        try {
            f >> root;
        } catch (...) {
            printf("[logzz] Failed to parse appStorage.json\n");
            return false;
        }

        // Extract UserId
        if (root.contains("UserId")) {
            try {
                if (root["UserId"].is_string()) {
                    current_user_ID = std::stoull(root["UserId"].get<std::string>());
                } else if (root["UserId"].is_number()) {
                    current_user_ID = root["UserId"].get<uint64_t>();
                }
            } catch (...) {
                printf("[logzz] Failed to parse UserId\n");
                current_user_ID = 0;
            }
        }

        // Extract Username
        if (root.contains("Username") && root["Username"].is_string()) {
            current_username = root["Username"].get<std::string>();
        }

        // Extract DisplayName
        if (root.contains("DisplayName") && root["DisplayName"].is_string()) {
            current_display_name = root["DisplayName"].get<std::string>();
        }

        return true;
    }
}
