#define NOMINMAX
#include "Resource Files/globals.h"
#include "Resource Files/profile_manager.h"
#include "imgui-files/imgui.h"
#include "Resource Files/json.hpp"
#include <fstream>
#include <filesystem>
#include <iostream>
#include <variant>
#include <unordered_map>
#include <shlobj.h>

using namespace Globals;

using json = nlohmann::json;

static std::string TrimNullChars(const char *buffer, size_t size)
{
    size_t length = std::strlen(buffer);
    if (length > size)
		length = size;
    return std::string(buffer, length);
}

using NumericVar = std::variant<int*, float*, unsigned int*>;

// Boolean variables to save
const std::unordered_map<std::string, bool *> bool_vars = {
	{"macrotoggled", &macrotoggled},
	{"shiftswitch", &shiftswitch},
	{"wallhopswitch", &wallhopswitch},
	{"wallhopcamfix", &wallhopcamfix},
	{"unequiptoggle", &unequiptoggle},
	{"isspeedswitch", &isspeedswitch},
	{"isfreezeswitch", &isfreezeswitch},
	{"iswallwalkswitch", &iswallwalkswitch},
	{"isspamswitch", &isspamswitch},
	{"isitemclipswitch", &isitemclipswitch},
	{"autotoggle", &autotoggle},
	{"toggle_jump", &toggle_jump},
	{"toggle_flick", &toggle_flick},
	{"camfixtoggle", &camfixtoggle},
	{"wallwalktoggleside", &wallwalktoggleside},
	{"antiafktoggle", &antiafktoggle},
	{"fasthhj", &fasthhj},
	{"globalzoomin", &globalzoomin},
	{"globalzoominreverse", &globalzoominreverse},
	{"wallesslhjswitch", &wallesslhjswitch},
	{"chatoverride", &chatoverride},
	{"bounceautohold", &bounceautohold},
	{"bouncerealignsideways", &bouncerealignsideways},
	{"bouncesidetoggle", &bouncesidetoggle},
	{"laughmoveswitch", &laughmoveswitch},
	{"freezeoutsideroblox", &freezeoutsideroblox},
	{"takeallprocessids", &takeallprocessids},
	{"ontoptoggle", &ontoptoggle},
	{"bunnyhopsmart", &bunnyhopsmart},
	{"presskeyinroblox", &presskeyinroblox},
	{"unequipinroblox", &unequipinroblox},
	{"doublepressafkkey", &doublepressafkkey},
	{"useoldpaste", &useoldpaste},
	{"floorbouncehhj", &floorbouncehhj},
	{"islagswitchswitch", &islagswitchswitch},
    {"prevent_disconnect", &prevent_disconnect},
	{"lagswitchoutbound", &lagswitchoutbound},
	{"lagswitchinbound", &lagswitchinbound},
	{"lagswitchtargetroblox", &lagswitchtargetroblox},
	{"lagswitchlaginbound", &lagswitchlaginbound},
	{"lagswitchlagoutbound", &lagswitchlagoutbound},
	{"lagswitchlag", &lagswitchlag},
	{"lagswitchusetcp", &lagswitchusetcp},
	{"lagswitch_autounblock", &lagswitch_autounblock},
	{"show_lag_overlay", &show_lag_overlay},
	{"overlay_hide_inactive", &overlay_hide_inactive},
	{"overlay_use_bg", &overlay_use_bg},
};

// Numeric variables to save
const std::unordered_map<std::string, NumericVar> numeric_vars = {
	{"vk_f5", &vk_f5},
	{"vk_f6", &vk_f6},
	{"vk_f8", &vk_f8},
	{"vk_mbutton", &vk_mbutton},
	{"vk_xbutton2", &vk_xbutton2},
	{"vk_leftbracket", &vk_leftbracket},
	{"vk_spamkey", &vk_spamkey},
	{"vk_zkey", &vk_zkey},
	{"vk_dkey", &vk_dkey},
	{"vk_xkey", &vk_xkey},
	{"vk_clipkey", &vk_clipkey},
	{"vk_laughkey", &vk_laughkey},
	{"vk_bouncekey", &vk_bouncekey},
	{"vk_bunnyhopkey", &vk_bunnyhopkey},
	{"vk_shiftkey", &vk_shiftkey},
	{"vk_enterkey", &vk_enterkey},
	{"vk_chatkey", &vk_chatkey},
	{"vk_afkkey", &vk_afkkey},
	{"vk_floorbouncekey", &vk_floorbouncekey},
	{"vk_lagswitchkey", &vk_lagswitchkey},
	{"selected_dropdown", &selected_dropdown},
	{"vk_wallkey", &vk_wallkey},
	{"PreviousWallWalkSide", &PreviousWallWalkSide},
	{"speed_slot", &speed_slot},
	{"desync_slot", &desync_slot},
	{"clip_slot", &clip_slot},
	{"spam_delay", &spam_delay},
	{"real_delay", &real_delay},
	{"wallhop_dx", &wallhop_dx},
	{"wallhop_dy", &wallhop_dy},
	{"PreviousWallWalkValue", &PreviousWallWalkValue},
	{"maxfreezetime", &maxfreezetime},
	{"maxfreezeoverride", &maxfreezeoverride},
	{"RobloxWallWalkValueDelay", &RobloxWallWalkValueDelay},
	{"speed_strengthx", &speed_strengthx},
	{"speedoffsetx", &speedoffsetx},
	{"speed_strengthy", &speed_strengthy},
	{"speedoffsety", &speedoffsety},
	{"clip_delay", &clip_delay},
	{"RobloxPixelValue", &RobloxPixelValue},
	{"PreviousSensValue", &PreviousSensValue},
	{"windowOpacityPercent", &windowOpacityPercent},
	{"AntiAFKTime", &AntiAFKTime},
	{"display_scale", &display_scale},
	{"WindowPosX", &WindowPosX},
	{"WindowPosY", &WindowPosY},
	{"lagswitch_max_duration", &lagswitch_max_duration},
	{"lagswitch_unblock_ms", &lagswitch_unblock_ms},
	{"lagswitchlagdelay", &lagswitchlagdelay},
	{"overlay_x", &overlay_x},
	{"overlay_y", &overlay_y},
	{"overlay_size", &overlay_size},
	{"overlay_bg_r", &overlay_bg_r},
	{"overlay_bg_g", &overlay_bg_g},
	{"overlay_bg_b", &overlay_bg_b},
};

// Char variables to save
const std::vector<std::pair<std::string, std::pair<char*, size_t>>> char_arrays = {
    {"settingsBuffer", {settingsBuffer, sizeof(settingsBuffer)}},
    {"ItemDesyncSlot", {ItemDesyncSlot, sizeof(ItemDesyncSlot)}},
    {"ItemSpeedSlot", {ItemSpeedSlot, sizeof(ItemSpeedSlot)}},
    {"ItemClipSlot", {ItemClipSlot, sizeof(ItemClipSlot)}},
    {"ItemClipDelay", {ItemClipDelay, sizeof(ItemClipDelay)}},
	{"BunnyHopDelayChar", {BunnyHopDelayChar, sizeof(BunnyHopDelayChar)}},
    {"RobloxSensValue", {RobloxSensValue, sizeof(RobloxSensValue)}},
    {"RobloxWallWalkValueChar", {RobloxWallWalkValueChar, sizeof(RobloxWallWalkValueChar)}},
    {"RobloxWallWalkValueDelayChar", {RobloxWallWalkValueDelayChar, sizeof(RobloxWallWalkValueDelayChar)}},
    {"WallhopPixels", {WallhopPixels, sizeof(WallhopPixels)}},
    {"SpamDelay", {SpamDelay, sizeof(SpamDelay)}},
    {"RobloxPixelValueChar", {RobloxPixelValueChar, sizeof(RobloxPixelValueChar)}},
    {"CustomTextChar", {CustomTextChar, sizeof(CustomTextChar)}},
	{"RobloxFPSChar", {RobloxFPSChar, sizeof(RobloxFPSChar)}},
	{"AntiAFKTimeChar", {AntiAFKTimeChar, sizeof(AntiAFKTimeChar)}},
	{"WallhopDelayChar", {WallhopDelayChar, sizeof(WallhopDelayChar)}},
	{"WallhopBonusDelayChar", {WallhopBonusDelayChar, sizeof(WallhopBonusDelayChar)}},
	{"PressKeyDelayChar", {PressKeyDelayChar, sizeof(PressKeyDelayChar)}},
	{"PressKeyBonusDelayChar", {PressKeyBonusDelayChar, sizeof(PressKeyBonusDelayChar)}},
	{"PasteDelayChar", {PasteDelayChar, sizeof(PasteDelayChar)}},
	{"HHJLengthChar", {HHJLengthChar, sizeof(HHJLengthChar)}},
	{"HHJFreezeDelayOverrideChar", {HHJFreezeDelayOverrideChar, sizeof(HHJFreezeDelayOverrideChar)}},
	{"HHJDelay1Char", {HHJDelay1Char, sizeof(HHJDelay1Char)}},
	{"HHJDelay2Char", {HHJDelay2Char, sizeof(HHJDelay2Char)}},
	{"HHJDelay3Char", {HHJDelay3Char, sizeof(HHJDelay3Char)}},
	{"FloorBounceDelay1Char", {FloorBounceDelay1Char, sizeof(FloorBounceDelay1Char)}},
	{"FloorBounceDelay2Char", {FloorBounceDelay2Char, sizeof(FloorBounceDelay2Char)}},
	{"FloorBounceDelay3Char", {FloorBounceDelay3Char, sizeof(FloorBounceDelay3Char)}},
};

std::vector<std::string> GetProfileNames(const std::string& filepath) {
    std::vector<std::string> names;
    std::ifstream file(filepath);
    if (!file.is_open()) {
        return names;
    }
    json root_json;
    try {
        file >> root_json;
        if (root_json.is_object()) {
            for (auto& el : root_json.items()) {
				// Skip Metadata
				if (el.key() == METADATA_KEY) {
                    continue;
				}

                if (el.value().is_object()) { // Is it a profile
                    names.push_back(el.key());
                }
            }
        }
    } catch (const json::parse_error& e) {
        std::cerr << "GetProfileNames: JSON parse error in '" << filepath << "': " << e.what() << std::endl;
    }
    file.close();
    // Sort the names alphabetically
    std::sort(names.begin(), names.end());

    // Check if "default" exists in the names
    auto it = std::find(names.begin(), names.end(), "(default)");
    if (it != names.end()) {
        // Move "default" to the front
        names.erase(it);
        names.insert(names.begin(), "(default)");
    }

    return names;
}

bool WriteJsonToFile(const std::string& filepath, const json& data) {
    std::ofstream outfile(filepath);
    if (outfile.is_open()) {
        outfile << data.dump(4);
        outfile.close();
        return true;
    }
    std::cerr << "Error: Could not open file for writing: " << filepath << std::endl;
    return false;
}

bool DeleteProfileFromFile(const std::string& filepath, const std::string& profile_name) {
    std::ifstream infile(filepath);
    if (!infile.is_open()) return false;
    json root_json;
    try { infile >> root_json; } catch (const json::parse_error&) { infile.close(); return false; }
    infile.close();

	if (profile_name == "(default)") {
		return false;
	}

    if (root_json.is_object() && root_json.contains(profile_name)) {
        root_json.erase(profile_name);
        if (WriteJsonToFile(filepath, root_json)) {
            if (G_CURRENTLY_LOADED_PROFILE_NAME == profile_name) {
                G_CURRENTLY_LOADED_PROFILE_NAME = "";
                std::cout << "Info: Deleted profile '" << profile_name << "' was active." << std::endl;
            }
            return true;
        }
    }
    return false;
}

bool RenameProfileInFile(const std::string& filepath, const std::string& old_name, const std::string& new_name) {
    if (old_name == new_name) return true;
    std::ifstream infile(filepath);
    if (!infile.is_open()) return false;
    json root_json;
    try { infile >> root_json; } catch (const json::parse_error&) { infile.close(); return false; }
    infile.close();

	if (old_name == "(default)" || new_name == "(default)") {
		return false;
	}

    if (root_json.is_object() && root_json.contains(old_name)) {
        if (root_json.contains(new_name)) {
             std::cerr << "Rename Error: Target name '" << new_name << "' already exists in file." << std::endl;
            return false;
        }
		
        json profile_data = root_json[old_name];
        root_json.erase(old_name);
        root_json[new_name] = profile_data;
        if (WriteJsonToFile(filepath, root_json)) {
            if (G_CURRENTLY_LOADED_PROFILE_NAME == old_name) {
                G_CURRENTLY_LOADED_PROFILE_NAME = new_name;
            }
            return true;
        }
    }
    return false;
}

bool DuplicateProfileInFile(const std::string& filepath, const std::string& source_name, const std::string& new_name) {
    std::ifstream infile(filepath);
    if (!infile.is_open()) return false;
    json root_json;
    try { infile >> root_json; } catch (const json::parse_error&) { infile.close(); return false; }
    infile.close();

    if (root_json.is_object() && root_json.contains(source_name)) {
        if (root_json.contains(new_name)) {
            std::cerr << "Duplicate Error: Target name '" << new_name << "' already exists in file." << std::endl;
            return false;
        }
        json profile_data_copy = root_json[source_name];
        root_json[new_name] = profile_data_copy;
        return WriteJsonToFile(filepath, root_json);
    }
    return false;
}

std::string GenerateUniqueProfileName(const std::string& base_name, const std::vector<std::string>& existing_names) {
    std::string candidate_name = base_name + " (Copy)";
    if (std::find(existing_names.begin(), existing_names.end(), candidate_name) == existing_names.end()) {
        return candidate_name;
    }
    int i = 2;
    while (true) {
        candidate_name = base_name + " (Copy " + std::to_string(i) + ")";
        if (std::find(existing_names.begin(), existing_names.end(), candidate_name) == existing_names.end()) {
            return candidate_name;
        }
        i++;
        if (i > 1000) return base_name + " (Copy Error)"; // Safety break
    }
}

std::string GenerateNewDefaultProfileName(const std::vector<std::string>& existing_names) {
    int i = 1;
    while (true) {
        std::string candidate_name = "Profile " + std::to_string(i);
        if (std::find(existing_names.begin(), existing_names.end(), candidate_name) == existing_names.end()) {
            return candidate_name;
        }
        i++;
        if (i > 1000) return "Profile Error"; // Safety break
    }
}

bool TryLoadLastActiveProfile(const std::string& filepath) {

    std::ifstream file;
	bool fileFound = false;
	std::filesystem::path real_filepath;

	// First try current directory
	real_filepath = std::filesystem::current_path() / filepath;
	file.open(real_filepath);
	if (file.is_open()) {
		fileFound = true;
	}
	else {
		// If not found, try parent directory
		std::filesystem::path current_path = std::filesystem::current_path();
		if (current_path.has_parent_path()) {
			real_filepath = current_path.parent_path() / filepath;
			file.open(real_filepath);
			if (file.is_open()) {
				fileFound = true;
				LoadSettings(real_filepath.string(), "(default)");
				std::this_thread::sleep_for(std::chrono::milliseconds(200));
				G_CURRENTLY_LOADED_PROFILE_NAME = "Profile 1";
				SaveSettings(filepath, "Profile 1");

				std::cerr << "Found Parent Save File!" << std::endl;
				return true;
			}
		}
	}

    if (!fileFound) {
        std::cerr << "TryLoadLastActiveProfile: Settings file '" << filepath << "' not found." << std::endl;
        return false;
    }

    json root_json;
    try {
        file >> root_json;
        file.close();
    } catch (const json::parse_error& e) {
        std::cerr << "TryLoadLastActiveProfile: JSON parse error in '" << filepath << "': " << e.what() << std::endl;
        file.close();
        return false;
    }

    if (root_json.is_object() && root_json.contains(METADATA_KEY) && root_json[METADATA_KEY].is_object()) {
        const auto& metadata = root_json[METADATA_KEY];
        if (metadata.contains(LAST_ACTIVE_PROFILE_KEY) && metadata[LAST_ACTIVE_PROFILE_KEY].is_string()) {
            std::string last_active_name = metadata[LAST_ACTIVE_PROFILE_KEY].get<std::string>();

            // Check if this profile actually exists in the file
            if (!last_active_name.empty() && root_json.contains(last_active_name) && root_json[last_active_name].is_object()) {
                LoadSettings(filepath, last_active_name); 
                G_CURRENTLY_LOADED_PROFILE_NAME = last_active_name;
                std::cout << "Successfully loaded last active profile: " << last_active_name << std::endl;
                return true;
            } else {
                std::cerr << "TryLoadLastActiveProfile: Last active profile '" << last_active_name << "' not found or invalid in settings file." << std::endl;
            }
        } else {
            // std::cout << "TryLoadLastActiveProfile: No 'last_active_profile' key found in metadata." << std::endl;
        }
    } else {
		// Import old settings in
		if (root_json.is_object()) {
			LoadSettings(filepath, "Profile 1");
			std::this_thread::sleep_for(std::chrono::milliseconds(200));
			SaveSettings(filepath, "Profile 1");
			G_CURRENTLY_LOADED_PROFILE_NAME = "Profile 1";
		}
    }
    return false;
}

namespace ProfileUI {
    static bool s_expanded = false;
    static std::vector<std::string> s_profile_names;
    static int s_selected_profile_idx = -1;
    
    static int s_editing_profile_idx = -1;
    static char s_edit_buffer[256] = "";
    static float s_last_click_time = -1.0f;
    static int s_last_clicked_item_idx = -1;

    static bool s_profiles_initialized = false;
    static std::string s_rename_error_msg = "";

	// Delete button
	static int s_delete_button_confirmation_stage = 0; // 0: Normal, 1: Yellow (confirm), 2: Red (deleted)
	static float s_delete_button_stage_timer = 0.0f;
	static int s_target_profile_idx_for_confirm = -1;
	static std::string s_G_LOADED_PROFILE_NAME_at_confirm_start = "";
	bool delete_action_requested_this_frame = false;

    void RefreshProfileListAndSelection() {
        std::string previously_selected_name;
        if (s_selected_profile_idx >= 0 && s_selected_profile_idx < s_profile_names.size()) {
            previously_selected_name = s_profile_names[s_selected_profile_idx];
        }
        
        s_profile_names = GetProfileNames(G_SETTINGS_FILEPATH); // Reads and sorts

        s_selected_profile_idx = -1; // Reset selection
        if (!previously_selected_name.empty()) {
            auto it = std::find(s_profile_names.begin(), s_profile_names.end(), previously_selected_name);
            if (it != s_profile_names.end()) {
                s_selected_profile_idx = std::distance(s_profile_names.begin(), it);
            }
        }
        // If nothing was selected, or previous selection is gone, try to select G_CURRENTLY_LOADED_PROFILE_NAME
        if (s_selected_profile_idx == -1 && !G_CURRENTLY_LOADED_PROFILE_NAME.empty()) {
            auto it = std::find(s_profile_names.begin(), s_profile_names.end(), G_CURRENTLY_LOADED_PROFILE_NAME);
            if (it != s_profile_names.end()) {
                s_selected_profile_idx = std::distance(s_profile_names.begin(), it);
            }
        }
    }


    void InitializeProfiles() {
        if (!s_profiles_initialized) {
            RefreshProfileListAndSelection();
            s_profiles_initialized = true;
        }
    }

    void DrawProfileManagerUI() {
        InitializeProfiles();
		ImVec4 current_button_bg_color = ImGui::GetStyle().Colors[ImGuiCol_Button];
		ImVec4 current_button_text_color = ImGui::GetStyle().Colors[ImGuiCol_Text];

        if (ImGui::Button(s_expanded ? "Profiles <" : "Profiles >", ImVec2(270, 0))) {
            s_expanded = !s_expanded;
            if (s_expanded) {
                RefreshProfileListAndSelection(); // Refresh list when opening
                s_rename_error_msg = ""; // Clear previous rename errors
            } else {
                s_editing_profile_idx = -1; // Close editor if dropdown closes
            }
        }


        if (s_expanded) {
            ImVec2 buttonPos = ImGui::GetItemRectMin();
            float menuWidth = ImGui::GetItemRectSize().x;
            
            float list_item_height = ImGui::GetTextLineHeightWithSpacing();
            int num_items_to_show = std::min(10, (int)s_profile_names.size());
            if (num_items_to_show == 0) num_items_to_show = 3; // Min height for empty list
            float buttons_height = ImGui::GetFrameHeightWithSpacing() * 2.0f + ImGui::GetStyle().ItemSpacing.y * 2.0f;
            float list_height = num_items_to_show * list_item_height + ImGui::GetStyle().WindowPadding.y * 2;
            float menuHeight = buttons_height + list_height + ImGui::GetStyle().SeparatorTextAlign.y; // Approx
			int old_s_selected_profile_idx = -1;
            menuHeight = std::min(menuHeight, 300.0f);


            ImGui::SetNextWindowPos(ImVec2(buttonPos.x, buttonPos.y - menuHeight - ImGui::GetStyle().WindowPadding.y + 3));
            ImGui::SetNextWindowSize(ImVec2(menuWidth, menuHeight));

            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(5, 5));
            ImGui::Begin("##ProfilesDropUpMenu", NULL, ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoSavedSettings);

            float actionButtonWidth = (ImGui::GetContentRegionAvail().x - ImGui::GetStyle().ItemSpacing.x * 2) / 3.0f;
            bool profile_is_selected = (s_selected_profile_idx != -1 && s_selected_profile_idx < s_profile_names.size());

			// Save current profile every time s_selected_profile_idx is updated
			if (old_s_selected_profile_idx != s_selected_profile_idx) {
				SaveSettings(G_SETTINGS_FILEPATH, G_CURRENTLY_LOADED_PROFILE_NAME);
			}

			if (profile_is_selected) {
				old_s_selected_profile_idx = s_selected_profile_idx;
			}

            // Buttons
            if (ImGui::Button("Save To", ImVec2(actionButtonWidth, 0))) {
                std::string profileToSave;

                if (profile_is_selected) {
                    profileToSave = s_profile_names[s_selected_profile_idx];
                } else {
                    profileToSave = GenerateNewDefaultProfileName(s_profile_names);
                }

				G_CURRENTLY_LOADED_PROFILE_NAME = profileToSave;
                SaveSettings(G_SETTINGS_FILEPATH, profileToSave);
                RefreshProfileListAndSelection(); // Update list
                // Ensure the saved profile is selected
                auto it = std::find(s_profile_names.begin(), s_profile_names.end(), profileToSave);
                if (it != s_profile_names.end()) s_selected_profile_idx = std::distance(s_profile_names.begin(), it);

                std::cout << "Saved to profile: " << profileToSave << std::endl;
                s_editing_profile_idx = -1; // Ensure editing stops
            }

            ImGui::SameLine();
            if (!profile_is_selected) ImGui::BeginDisabled();

            if (ImGui::Button("Load", ImVec2(actionButtonWidth, 0))) {
                if (profile_is_selected) {
                    LoadSettings(G_SETTINGS_FILEPATH, s_profile_names[s_selected_profile_idx]);
                    G_CURRENTLY_LOADED_PROFILE_NAME = s_profile_names[s_selected_profile_idx];
                    s_editing_profile_idx = -1;
                }
            }

            if (!profile_is_selected) ImGui::EndDisabled();
            ImGui::SameLine();

            if (!profile_is_selected) ImGui::BeginDisabled();

			ImGuiIO &io = ImGui::GetIO();

			// Manage Delete Color
			if (s_delete_button_confirmation_stage != 0) {
				s_delete_button_stage_timer -= io.DeltaTime;
				bool should_reset_state = false;
				if (s_delete_button_stage_timer <= 0.0f) {
					should_reset_state = true; // Timer expired
				}
				if (s_target_profile_idx_for_confirm != -1 && s_selected_profile_idx != s_target_profile_idx_for_confirm) {
					should_reset_state = true;
				}
				if (!s_G_LOADED_PROFILE_NAME_at_confirm_start.empty() && 
					G_CURRENTLY_LOADED_PROFILE_NAME != s_G_LOADED_PROFILE_NAME_at_confirm_start) {
					should_reset_state = true;
				}
				if (should_reset_state) {
					s_delete_button_confirmation_stage = 0;
					s_target_profile_idx_for_confirm = -1;
					s_G_LOADED_PROFILE_NAME_at_confirm_start = "";
					s_delete_button_stage_timer = 0.0f;
				}
			}

			// Update Delete Button Color
			if (profile_is_selected && s_selected_profile_idx == s_target_profile_idx_for_confirm) {
				if (s_delete_button_confirmation_stage == 1) {
					current_button_bg_color = ImVec4(1.0f, 1.0f, 0.0f, 1.0f); // Yellow
					current_button_text_color = ImVec4(0.0f, 0.0f, 0.0f, 1.0f); // Black
				}
			}

			ImGui::PushStyleColor(ImGuiCol_Text, current_button_text_color);
			ImGui::PushStyleColor(ImGuiCol_Button, current_button_bg_color);
			ImGui::PushStyleColor(ImGuiCol_ButtonHovered, current_button_bg_color);
			ImGui::PushStyleColor(ImGuiCol_ButtonActive, current_button_bg_color);

            if (ImGui::Button("Delete", ImVec2(actionButtonWidth, 0))) {
                if (profile_is_selected) {
                    delete_action_requested_this_frame = true;
                }
            }
			ImGui::PopStyleColor(4);


			if (delete_action_requested_this_frame) {
				if (profile_is_selected) {
					if (s_delete_button_confirmation_stage == 0 || s_selected_profile_idx != s_target_profile_idx_for_confirm) {
						s_delete_button_confirmation_stage = 1; // Yellow
						s_delete_button_stage_timer = 0.7f;
						s_target_profile_idx_for_confirm = s_selected_profile_idx;
						s_G_LOADED_PROFILE_NAME_at_confirm_start = G_CURRENTLY_LOADED_PROFILE_NAME;
					} else if (s_delete_button_confirmation_stage == 1 && s_selected_profile_idx == s_target_profile_idx_for_confirm) {
						s_delete_button_confirmation_stage = 2;

						// Delete Profile
						std::string name_to_delete = s_profile_names[s_target_profile_idx_for_confirm];
						if (DeleteProfileFromFile(G_SETTINGS_FILEPATH, name_to_delete)) {
							RefreshProfileListAndSelection();
							// Delete your profile if you deleted your last save file
							std::ifstream infile(G_SETTINGS_FILEPATH);

							if (!infile.is_open()) return;
							json root_json;
							try { infile >> root_json; } catch (const json::parse_error&) { infile.close(); return; }
							infile.close();

							if (root_json.is_object() && root_json.size() == 1) {
								std::filesystem::remove(G_SETTINGS_FILEPATH);
							}
						}
						s_editing_profile_idx = -1;
					}
				}
			}
	
			delete_action_requested_this_frame = false;

            if (!profile_is_selected) ImGui::EndDisabled();

            if (!profile_is_selected) ImGui::BeginDisabled();
            if (ImGui::Button("Duplicate", ImVec2(-FLT_MIN, 0))) {
                if (profile_is_selected) {
                    std::string source_name = s_profile_names[s_selected_profile_idx];
                    std::string new_name = GenerateUniqueProfileName(source_name, s_profile_names);
                    if (DuplicateProfileInFile(G_SETTINGS_FILEPATH, source_name, new_name)) {
						// Load New Profile
                        LoadSettings(G_SETTINGS_FILEPATH, new_name);
                        G_CURRENTLY_LOADED_PROFILE_NAME = new_name;
                        RefreshProfileListAndSelection();

                        auto it = std::find(s_profile_names.begin(), s_profile_names.end(), new_name);
                        if (it != s_profile_names.end()) s_selected_profile_idx = std::distance(s_profile_names.begin(), it);

                    }
                    s_editing_profile_idx = -1;
                }
            }
            if (!profile_is_selected) ImGui::EndDisabled();
            ImGui::Separator();

            // --- Scrollable selectable list ---
            ImGui::BeginChild("##ProfilesOptionsList", ImVec2(0, ImGui::GetContentRegionAvail().y - ImGui::GetStyle().ItemSpacing.y - (s_rename_error_msg.empty() ? 0 : ImGui::GetTextLineHeightWithSpacing())) , true, ImGuiWindowFlags_HorizontalScrollbar);
            {
                for (int i = 0; i < s_profile_names.size(); ++i) {
                    bool is_editing_this_item = (s_editing_profile_idx == i);
                    bool is_selected_this_item = (s_selected_profile_idx == i);

                    bool is_currently_loaded = (!G_CURRENTLY_LOADED_PROFILE_NAME.empty() && G_CURRENTLY_LOADED_PROFILE_NAME == s_profile_names[i]);

                    if (is_currently_loaded) {
                        ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(0.0f, 1.0f, 0.0f, 1.0f)); // Green
                    }

					if (s_profile_names[i] == "(default)") {
						ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(1.0f, 0.54f, 0.0f, 1.0f)); // Orange
					}

                    if (is_editing_this_item) {
                        ImGui::SetKeyboardFocusHere();
                        ImGui::SetNextItemWidth(-FLT_MIN); 

                        if ((ImGui::InputText("##EditProfileName", s_edit_buffer, IM_ARRAYSIZE(s_edit_buffer), ImGuiInputTextFlags_EnterReturnsTrue | ImGuiInputTextFlags_AutoSelectAll)) || (ImGui::IsMouseClicked(0) && !ImGui::IsItemHovered())) {
                            std::string new_name_candidate = s_edit_buffer;
                            s_rename_error_msg = ""; // Clear previous error
                            
							if (new_name_candidate == "(default)") {
								s_rename_error_msg = "Name cannot be default.";
							}

                            if (new_name_candidate.empty()) {
                                s_rename_error_msg = "Name cannot be empty.";
                            } else {
                                bool name_exists_for_another_profile = false;
                                for (int j = 0; j < s_profile_names.size(); ++j) {
                                    if (j != i && s_profile_names[j] == new_name_candidate) {
                                        name_exists_for_another_profile = true;
                                        break;
                                    }
                                }
                                if (name_exists_for_another_profile) {
                                     s_rename_error_msg = "Name already exists.";
                                }
                            }
                            
                            if (s_rename_error_msg.empty()) { // If no error
                                if (RenameProfileInFile(G_SETTINGS_FILEPATH, s_profile_names[i], new_name_candidate)) {
                                    s_profile_names[i] = new_name_candidate; // Update local list immediately
                                    RefreshProfileListAndSelection();
                                    // Re-find the selected index after sort
                                    auto it = std::find(s_profile_names.begin(), s_profile_names.end(), new_name_candidate);
                                    if (it != s_profile_names.end()) s_selected_profile_idx = std::distance(s_profile_names.begin(), it);
                                    
                                    if (G_CURRENTLY_LOADED_PROFILE_NAME == s_profile_names[i] && s_profile_names[i] != new_name_candidate) {
                                        // This case should be handled by RenameProfileInFile updating G_CURRENTLY_LOADED_PROFILE_NAME
                                    }
                                    std::cout << "Renamed profile to: " << new_name_candidate << std::endl;
                                }

                                s_editing_profile_idx = -1; // Exit editing mode on success or file failure
                            }
                            // If s_rename_error_msg is set, stay in editing mode for user to fix.
                        }

                    } else {
                        if (ImGui::Selectable(s_profile_names[i].c_str(), is_selected_this_item, ImGuiSelectableFlags_AllowDoubleClick)) {
                            s_selected_profile_idx = i;
                            if (ImGui::IsMouseDoubleClicked(0)) { // Check for double click on this item
                                s_editing_profile_idx = i;
                                strncpy(s_edit_buffer, s_profile_names[i].c_str(), sizeof(s_edit_buffer) -1);
                                s_edit_buffer[sizeof(s_edit_buffer) - 1] = '\0';
                                s_last_click_time = -1.0f; // Reset double-click state
                                s_rename_error_msg = ""; // Clear rename error when starting new edit
                            } else { // Single click
                                // Handled by s_selected_profile_idx = i;
                                s_last_clicked_item_idx = i; // For your original double click logic if needed
                                s_last_click_time = ImGui::GetTime();
                            }
                        }
                         if (is_selected_this_item) ImGui::SetItemDefaultFocus();
                    }

					if (is_currently_loaded) {
                        ImGui::PopStyleColor();
                    }

					if (s_profile_names[i] == "(default)") {
						ImGui::PopStyleColor();
					}
                }
            }
            ImGui::EndChild(); // ##ProfilesOptionsList

            if (!s_rename_error_msg.empty()) {
                ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(1.0f, 0.0f, 0.0f, 1.0f));
                ImGui::TextWrapped("%s", s_rename_error_msg.c_str());
                ImGui::PopStyleColor();
            }

            ImGui::End(); // ##ProfilesDropUpMenu
            ImGui::PopStyleVar();
        }
    }
}

void SaveSettings(const std::string& filepath, const std::string& profile_name) {
    json current_profile_data; // JSON object for the settings of the current profile

	if (profile_name.empty() || profile_name == "(default)") {
		return;
    }

	std::string profile_name_modified = profile_name;

	// Secret code to save a default profile
	if (profile_name == "SAVE_DEFAULT_90493") {
		profile_name_modified = "(default)";
	}

    // Save booleans
    for (const auto& [key, ptr] : bool_vars) {
        if (ptr) current_profile_data[key] = *ptr;
    }

    // Save numerics
    for (const auto& [key, var] : numeric_vars) {
        std::visit([&](auto&& arg) {
            if (arg) current_profile_data[key] = *arg;
        }, var);
    }

    // Save char arrays
    for (const auto& [key, cfg] : char_arrays) {
        if (cfg.first) current_profile_data[key] = TrimNullChars(cfg.first, cfg.second);
    }

    // Special cases
    if (section_amounts > 0) {
        current_profile_data["section_toggles"] = std::vector<bool>(section_toggles, section_toggles + section_amounts);
        current_profile_data["section_order_vector"] = std::vector<int>(section_order, section_order + section_amounts);
    }

    current_profile_data["text"] = text;
    current_profile_data["screen_width"] = screen_width;
    current_profile_data["screen_height"] = screen_height;

    // --- Profile Handling ---
    json root_json_output = json::object(); // This will be written to the file

    std::ifstream infile(filepath);
    if (infile.is_open()) {
        try {
            json existing_file_data;
            infile >> existing_file_data;
            infile.close();

            if (existing_file_data.is_object() && !existing_file_data.empty()) {
                bool is_already_profile_format = false;
                // Check if any top-level key looks like a profile name (e.g., starts with "Profile ")
                // This is a heuristic; adjust if your profile names have a different pattern.
                for (const auto& item : existing_file_data.items()) {
                    if (item.key().rfind("(default)", 0) == 0 && item.value().is_object()) {
                        is_already_profile_format = true;
                        break;
                    }
                }

                if (is_already_profile_format) {
                    root_json_output = existing_file_data; // It's already in the new profile format
                } else {
                    // It's an object, but not in "Profile X" format.
                    // Assume it's an old flat format if it contains recognizable settings.
                    // This check helps differentiate an old config from an empty or unrelated JSON object.
                    if ((!bool_vars.empty() && !bool_vars.begin()->first.empty() && existing_file_data.contains(bool_vars.begin()->first)) ||
                        (!numeric_vars.empty() && !numeric_vars.begin()->first.empty() && existing_file_data.contains(numeric_vars.begin()->first)))
                    {
                        std::cout << "Old format detected during save. Converting. Old data will be under 'Profile 1'." << std::endl;
                        root_json_output["Profile 1"] = existing_file_data; // Convert old data to "Profile 1"
                    } else {
                        if (!existing_file_data.empty()) {
                             std::cerr << "Warning: Existing settings file '" << filepath << "' contains an unrecognized JSON object structure. "
                                       << "It will be treated as a base for new profile data." << std::endl;
                             root_json_output = existing_file_data; // Keep it if it was some other map
                        }
                        // If existing_file_data was empty, root_json_output is already a new empty object.
                    }
                }
            }
            // If file was empty or not a JSON object, existing_file_data might be null or non-object.
            // In such cases, root_json_output remains a fresh json::object().
        } catch (const json::parse_error& e) {
            std::cerr << "JSON parse error reading '" << filepath << "' for save: " << e.what() << ". Starting with a new profile structure." << std::endl;
            infile.close(); // Ensure file is closed on error
            // root_json_output will be a fresh json::object()
        }
    }
    // If file didn't exist, infile.is_open() was false, root_json_output is a fresh json::object().

    // Add/update the current profile's settings into the root JSON
    root_json_output[profile_name_modified] = current_profile_data;

    if (!G_CURRENTLY_LOADED_PROFILE_NAME.empty()) {
        if (!root_json_output.contains(METADATA_KEY) || !root_json_output[METADATA_KEY].is_object()) {
            root_json_output[METADATA_KEY] = json::object(); // Create metadata object
        }
        root_json_output[METADATA_KEY][LAST_ACTIVE_PROFILE_KEY] = G_CURRENTLY_LOADED_PROFILE_NAME;
    }

	if (profile_name != "SAVE_DEFAULT_90493") {
		// Global variables that don't change across profiles go here saved when not saving as (default)
		root_json_output[METADATA_KEY]["shortdescriptions"] = shortdescriptions;
		root_json_output[METADATA_KEY]["DontShowAdminWarning"] = DontShowAdminWarning;
	}

    // Write the root JSON (which now contains all profiles) to file
    std::ofstream outfile(filepath);
    if (outfile.is_open()) {
        outfile << root_json_output.dump(4); // pretty print with 4 spaces
        outfile.close();
    } else {
        std::cerr << "Error: Could not open settings file for writing: " << filepath << std::endl;
    }
}

void LoadSettings(const std::string& filepath, const std::string& profile_name) {

	if (profile_name == "") {
		return;
	}

	std::ifstream file;
	bool fileFound = false;
	std::filesystem::path real_filepath;

	// First try current directory
	real_filepath = std::filesystem::current_path() / filepath;
	file.open(real_filepath);
	if (file.is_open()) {
		fileFound = true;
	}
	else {
		// If not found, try parent directory
		std::filesystem::path current_path = std::filesystem::current_path();
		if (current_path.has_parent_path()) {
			real_filepath = current_path.parent_path() / filepath;
			file.open(real_filepath);
			if (file.is_open()) {
				fileFound = true;
			}
		}
	}

	if (!fileFound) {
		std::cerr << "Info: Settings file '" << filepath 
				 << "' not found in current or parent directory. Using default values. Saving profile 1." << std::endl;
		return;
	}

	json root_file_json;
	try {
		file >> root_file_json;
		file.close();
	} catch (const json::parse_error& e) {
		std::cerr << "JSON parse error in '" << real_filepath.string() << "': " << e.what() << std::endl;
		file.close();
		return;
	}

	// Make default profile if it doesn't exist
    if ((!root_file_json.contains("(default)") && (profile_name == "SAVE_DEFAULT_90493"))) {
        SaveSettings(filepath, "SAVE_DEFAULT_90493");
    }

    json settings_to_load; // This will hold the JSON data for the specific profile
    bool profile_data_extracted = false;

    // Try to load as new format (profile-based)
    if (root_file_json.is_object() && root_file_json.contains(profile_name)) {
        if (root_file_json.at(profile_name).is_object()) {
            settings_to_load = root_file_json.at(profile_name);
            profile_data_extracted = true;
        } else {
            std::cerr << "Warning: Profile '" << profile_name << "' in '" << filepath << "' is not a valid settings object." << std::endl;
        }
    }

    // Backwards Compatibility: If profile wasn't found AND we are trying to load the designated "legacy" profile name
    if (!profile_data_extracted && profile_name == "Profile 1") { // "Profile 1" is the designated default for old files
        bool looks_like_multi_profile_file = false;
        if (root_file_json.is_object()) {
            for (const auto& item : root_file_json.items()) {
                // Heuristic: if any key starts with "Profile " and its value is an object.
                if (item.key().rfind("Profile ", 0) == 0 && item.value().is_object()) {
                    looks_like_multi_profile_file = true;
                    break;
                }
            }
        }

        // If it doesn't look like a multi-profile file and it contains recognizable settings keys at the root,
        // then treat the root_file_json itself as the settings for "Profile 1".
        if (!looks_like_multi_profile_file && root_file_json.is_object() &&
            (root_file_json.contains("text") || // A common key from old format
             (!bool_vars.empty() && !bool_vars.begin()->first.empty() && root_file_json.contains(bool_vars.begin()->first)) ||
             (!numeric_vars.empty() && !numeric_vars.begin()->first.empty() && root_file_json.contains(numeric_vars.begin()->first))
            ))
        {
            settings_to_load = root_file_json;
            profile_data_extracted = true;
            std::cout << "Info: Loaded legacy format file as '" << profile_name << "'." << std::endl;
        }
    }

    if (!profile_data_extracted) {
		if (profile_name != "SAVE_DEFAULT_90493") {
			std::cerr << "Warning: Profile '" << profile_name << "' not found or data is invalid in '" << filepath << "'. Using current/default values." << std::endl;
		}
        return;
    }
    
    // --- Actual loading  ---
    try {
        // Load booleans
        for (const auto& [key, ptr] : bool_vars) {
            if (ptr && settings_to_load.contains(key) && settings_to_load[key].is_boolean()) {
                *ptr = settings_to_load[key].get<bool>();
            }
        }

        // Load numerics
        for (const auto& [key, var] : numeric_vars) {
            if (!settings_to_load.contains(key)) continue;
            std::visit([&](auto&& arg) {
                using T = std::decay_t<decltype(*arg)>;
                if (arg && settings_to_load[key].is_number()) {
                    *arg = settings_to_load[key].get<T>();
                }
            }, var);
        }

        // Load char arrays
        for (const auto& [key, cfg] : char_arrays) {
            if (cfg.first && settings_to_load.contains(key) && settings_to_load[key].is_string()) {
                std::string str_val = settings_to_load[key].get<std::string>();
                strncpy(cfg.first, str_val.c_str(), cfg.second -1); // Leave space for null terminator
                cfg.first[cfg.second - 1] = '\0'; // Ensure null-termination
            }
        }

		if (settings_to_load.contains("section_toggles") && settings_to_load["section_toggles"].is_array()) {
            auto toggles = settings_to_load["section_toggles"].get<std::vector<bool>>();
            size_t count = std::min(toggles.size(), static_cast<size_t>(section_amounts));
            // Copy loaded values. Any new sections (indices >= count) keep their default values set at the top of the file.
            std::copy(toggles.begin(), toggles.begin() + count, section_toggles);
        }

		if (settings_to_load.contains("section_order_vector") && settings_to_load["section_order_vector"].is_array()) {
            auto order = settings_to_load["section_order_vector"].get<std::vector<int>>();
                
            // 1. HACKY SOLUTION: Add in Bunnyhop Location to older save files
            // We apply this to 'order' BEFORE processing it further
            if (std::find(order.begin(), order.end(), 13) == order.end() && order.size() >= 6) {
                    if (order.size() >= 7) {
                        order.insert(order.begin() + 7, 13); // Insert at index 6 (7th element)
                    }
            }
			// Add in Lagswitch location to older save files
			if (std::find(order.begin(), order.end(), 15) == order.end() && order.size() >= 4) {
				if (order.size() >= 5) {
					order.insert(order.begin() + 4, 15); // Insert at index 4 (5th element)
				}
			}
        
            // Filter duplicates and fill missing IDs
            std::vector<int> final_order;
            std::vector<bool> id_exists(section_amounts, false);

            // Add loaded items ONLY if they are valid and NOT duplicates
            for (int id : order) {
                if (id >= 0 && id < section_amounts) {
                    if (!id_exists[id]) {
                        final_order.push_back(id);
                        id_exists[id] = true;
                    }
                }
            }

            // Find any new IDs (like 15) that weren't in the file and append them
            for (int i = 0; i < section_amounts; ++i) {
                if (!id_exists[i]) {
                    final_order.push_back(i);
                }
            }

            // Update the global array correctly
            size_t count = std::min(final_order.size(), static_cast<size_t>(section_amounts));
            for (size_t i = 0; i < count; ++i) {
                section_order[i] = final_order[i];
            }
        }

        if (settings_to_load.contains("text") && settings_to_load["text"].is_string()) {
            text = settings_to_load["text"].get<std::string>();
        }
        if (settings_to_load.contains("screen_width") && settings_to_load["screen_width"].is_number_integer()) {
            screen_width = settings_to_load.value("screen_width", screen_width);
        }
        if (settings_to_load.contains("screen_height") && settings_to_load["screen_height"].is_number_integer()) {
            screen_height = settings_to_load.value("screen_height", screen_height);
        }

		if (settings_to_load.contains("RobloxFPSChar") && settings_to_load["RobloxFPSChar"].is_string()) {
            RobloxFPS = std::stoi(RobloxFPSChar);
        }

		// Load global variables in metadata (applies across all profiles)
		if (root_file_json.contains(METADATA_KEY) && root_file_json[METADATA_KEY].is_object()) {
			const auto& metadata = root_file_json[METADATA_KEY];

			if (metadata.contains("shortdescriptions") && metadata["shortdescriptions"].is_boolean()) {
				shortdescriptions = metadata["shortdescriptions"].get<bool>();
			}

			if (metadata.contains("DontShowAdminWarning") && metadata["DontShowAdminWarning"].is_boolean()) {
				DontShowAdminWarning = metadata["DontShowAdminWarning"].get<bool>();
			}
		}

    } catch (const json::exception& e) {
        std::cerr << "Load error processing profile '" << profile_name << "': " << e.what() << '\n';
    }
}
