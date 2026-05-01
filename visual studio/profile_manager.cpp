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
namespace fs = std::filesystem;
using json = nlohmann::json;

// ============================================================================
//  HELPERS
// ============================================================================

inline int distance_to_int(std::ptrdiff_t dist) {
    return static_cast<int>(dist);
}

static std::string TrimNullChars(const char *buffer, size_t size) {
    size_t length = std::strlen(buffer);
    if (length > size) length = size;
    return std::string(buffer, length);
}

using NumericVar = std::variant<int*, float*, unsigned int*>;

// ============================================================================
//  VARIABLE MAPS — Data-driven save/load tables
// ============================================================================

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
	{"HHJFreezeDelayApply", &HHJFreezeDelayApply},
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

const std::unordered_map<std::string, NumericVar> numeric_vars = {
	{"selected_section", &selected_section},
	{"vk_f5", &vk_f5},
	{"vk_f6", &vk_f6},
	{"vk_f8", &vk_f8},
	{"vk_mbutton", &vk_mbutton},
	{"vk_xbutton1", &vk_xbutton1},
	{"vk_xbutton2", &vk_xbutton2},
	{"vk_wallhopjumpkey", &vk_wallhopjumpkey},
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
	{"vk_autohhjkey1", &vk_autohhjkey1},
	{"vk_autohhjkey2", &vk_autohhjkey2},
	{"selected_dropdown", &selected_dropdown},
	{"vk_wallkey", &vk_wallkey},
	{"PreviousWallWalkSide", &PreviousWallWalkSide},
	{"selected_wallhop_instance", &selected_wallhop_instance},
	{"speed_slot", &speed_slot},
	{"desync_slot", &desync_slot},
	{"clip_slot", &clip_slot},
	{"spam_delay", &spam_delay},
	{"real_delay", &real_delay},
	{"wallhop_dx", &wallhop_dx},
	{"wallhop_dy", &wallhop_dy},
	{"wallhop_vertical", &wallhop_vertical},
	{"PreviousWallWalkValue", &PreviousWallWalkValue},
	{"maxfreezetime", &maxfreezetime},
	{"maxfreezeoverride", &maxfreezeoverride},
	{"RobloxWallWalkValueDelay", &RobloxWallWalkValueDelay},
	{"speed_strengthx", &speed_strengthx},
	{"speedoffsetx", &speedoffsetx},
	{"speed_strengthy", &speed_strengthy},
	{"speedoffsety", &speedoffsety},
	{"clip_delay", &clip_delay},
	{"AutoHHJKey1Time", &AutoHHJKey1Time},
	{"AutoHHJKey2Time", &AutoHHJKey2Time},
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
	{"WallhopVerticalChar", {WallhopVerticalChar, sizeof(WallhopVerticalChar)}},
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
	{"AutoHHJKey1TimeChar", {AutoHHJKey1TimeChar, sizeof(AutoHHJKey1TimeChar)}},
	{"AutoHHJKey2TimeChar", {AutoHHJKey2TimeChar, sizeof(AutoHHJKey2TimeChar)}},
	{"FloorBounceDelay1Char", {FloorBounceDelay1Char, sizeof(FloorBounceDelay1Char)}},
	{"FloorBounceDelay2Char", {FloorBounceDelay2Char, sizeof(FloorBounceDelay2Char)}},
	{"FloorBounceDelay3Char", {FloorBounceDelay3Char, sizeof(FloorBounceDelay3Char)}},
};

// ============================================================================
//  FILE I/O — Centralized JSON file read/write with backup safety
// ============================================================================

// Read and parse a JSON file. Returns {data, success, error_message}.
struct JsonFileResult {
	json data;
	bool success;
	std::string error;
};

static JsonFileResult ReadJsonFile(const std::string& filepath) {
	JsonFileResult result;
	result.success = false;

	std::ifstream file(filepath);
	if (!file.is_open()) {
		result.error = "Could not open file: " + filepath;
		return result;
	}

	try {
		file >> result.data;
		result.success = true;
	} catch (const json::parse_error& e) {
		result.error = std::string("JSON parse error: ") + e.what();
	}
	file.close();
	return result;
}

// Write JSON to file. Creates a .bak backup of the previous version.
static bool WriteJsonFile(const std::string& filepath, const json& data) {
	// Create backup of existing file before overwriting
	if (fs::exists(filepath)) {
		std::string backup_path = filepath + ".bak";
		try {
			fs::copy_file(filepath, backup_path, fs::copy_options::overwrite_existing);
		} catch (const fs::filesystem_error&) {
			// Non-fatal — proceed without backup
		}
	}

	std::ofstream outfile(filepath);
	if (!outfile.is_open()) {
		std::cerr << "Error: Could not open file for writing: " << filepath << std::endl;
		return false;
	}
	outfile << data.dump(4);
	outfile.close();
	return true;
}

// ============================================================================
//  FILE RESOLUTION — Single function to find the settings file
//  Search order: current dir → parent dir, with SMC priority over RMC.
//  Automatically renames RMCSettings.json → SMCSettings.json when found.
//  Copies files from parent directory to current directory for future use.
//  May throw std::filesystem::filesystem_error if filesystem queries,
//  renames, or copies fail.
// ============================================================================

std::string ResolveSettingsFilePath() {
	fs::path cwd = fs::current_path();

	// 1. Check current directory: SMCSettings.json (preferred name)
	if (fs::exists(cwd / "SMCSettings.json")) {
		return "SMCSettings.json";
	}

	// 2. Check current directory: RMCSettings.json (legacy name) → rename it
	if (fs::exists(cwd / "RMCSettings.json")) {
		try {
			fs::rename(cwd / "RMCSettings.json", cwd / "SMCSettings.json");
		} catch (const fs::filesystem_error& e) {
			std::cerr << "Warning: Could not rename RMCSettings.json: " << e.what() << std::endl;
			return "RMCSettings.json"; // Use old name if rename fails
		}
		return "SMCSettings.json";
	}

	// 3. Check parent directory (for files that live one folder above the .exe)
	if (cwd.has_parent_path()) {
		fs::path parent = cwd.parent_path();

		// Try SMCSettings.json in parent
		if (fs::exists(parent / "SMCSettings.json")) {
			try {
				fs::copy_file(parent / "SMCSettings.json", cwd / "SMCSettings.json");
				std::cout << "Found settings file in parent directory, copied locally." << std::endl;
			} catch (const fs::filesystem_error&) {}
			return "SMCSettings.json";
		}

		// Try RMCSettings.json in parent → copy as SMCSettings.json
		if (fs::exists(parent / "RMCSettings.json")) {
			try {
				fs::copy_file(parent / "RMCSettings.json", cwd / "SMCSettings.json");
				std::cout << "Found legacy settings file in parent directory, copied locally as SMCSettings.json." << std::endl;
			} catch (const fs::filesystem_error&) {}
			return "SMCSettings.json";
		}
	}

	// 4. Nothing found — return default name for new file creation
	return "SMCSettings.json";
}

// ============================================================================
//  FORMAT DETECTION — Distinguish new profile format from legacy flat format
// ============================================================================

// New format has a _metadata key at the root level.
static bool IsProfileFormat(const json& root) {
	return root.is_object() && root.contains(METADATA_KEY) && root[METADATA_KEY].is_object();
}

// Legacy flat format: an object with recognizable settings keys at root, but NO _metadata.
// Old save files from before the profile system stored settings directly at root level.
static bool IsLegacyFlatFormat(const json& root) {
	if (!root.is_object()) return false;
	if (root.contains(METADATA_KEY)) return false;

	// Check for recognizable settings keys
	for (const auto& [key, _] : bool_vars) {
		if (root.contains(key)) return true;
	}
	for (const auto& [key, _] : numeric_vars) {
		if (root.contains(key)) return true;
	}
	if (root.contains("text")) return true;

	return false;
}

// ============================================================================
//  PROFILE DISCOVERY — Find the best profile to load
//  Priority: exact match → last_active from metadata → any non-default profile.
//  (default) is NEVER returned — it is read-only. Caller creates Profile 1 if needed.
// ============================================================================

static std::string FindBestProfile(const json& root, const std::string& requested_name) {
	// 1. Exact match for the requested profile
	if (!requested_name.empty() && root.contains(requested_name) && root[requested_name].is_object()) {
		return requested_name;
	}

	// 2. Last active profile from metadata (skip (default) — read-only profiles are not valid resume targets)
	if (root.contains(METADATA_KEY) && root[METADATA_KEY].is_object()) {
		const auto& meta = root[METADATA_KEY];
		if (meta.contains(LAST_ACTIVE_PROFILE_KEY) && meta[LAST_ACTIVE_PROFILE_KEY].is_string()) {
			std::string last_active = meta[LAST_ACTIVE_PROFILE_KEY].get<std::string>();
			if (!last_active.empty() && last_active != "(default)" && root.contains(last_active) && root[last_active].is_object()) {
				return last_active;
			}
		}
	}

	// 3. Any non-(default) profile
	std::string last_non_default;
	for (const auto& [key, value] : root.items()) {
		if (key == METADATA_KEY || key == "(default)") continue;
		if (value.is_object()) {
			last_non_default = key;
		}
	}
	if (!last_non_default.empty()) return last_non_default;

	// (default) is intentionally excluded as a fallback — it is read-only.
	// TryLoadLastActiveProfile will create Profile 1 from compile-time defaults.
	return "";
}

// ============================================================================
//  SERIALIZATION — Convert between globals and JSON profile objects
// ============================================================================

static json SerializeProfileData() {
	json data;

	// Sync instances[0] → flat globals so the existing variable maps reflect current state
	if (!wallhop_instances.empty()) {
		auto& w = wallhop_instances[0];
		wallhopswitch  = w.wallhopswitch;    wallhopcamfix  = w.wallhopcamfix;
		toggle_jump    = w.toggle_jump;       toggle_flick   = w.toggle_flick;
		wallhop_dx     = w.wallhop_dx;        wallhop_dy     = w.wallhop_dy;
		wallhop_vertical = w.wallhop_vertical;
		disable_outside_roblox[6] = w.disable_outside_roblox;
		vk_xbutton2    = w.vk_trigger;
		vk_wallhopjumpkey = w.vk_jumpkey;
		strncpy_s(WallhopPixels,         sizeof(WallhopPixels),         w.WallhopPixels,         _TRUNCATE);
		strncpy_s(WallhopVerticalChar,   sizeof(WallhopVerticalChar),   w.WallhopVerticalChar,   _TRUNCATE);
		strncpy_s(WallhopDelayChar,      sizeof(WallhopDelayChar),      w.WallhopDelayChar,      _TRUNCATE);
		strncpy_s(WallhopBonusDelayChar, sizeof(WallhopBonusDelayChar), w.WallhopBonusDelayChar, _TRUNCATE);
		section_toggles[6] = w.section_enabled;
	}
	if (!presskey_instances.empty()) {
		auto& p = presskey_instances[0];
		presskeyinroblox = p.presskeyinroblox;
		disable_outside_roblox[5] = p.presskeyinroblox;
		vk_zkey = p.vk_trigger;   vk_dkey = p.vk_presskey;
		strncpy_s(PressKeyDelayChar,      sizeof(PressKeyDelayChar),      p.PressKeyDelayChar,      _TRUNCATE);
		strncpy_s(PressKeyBonusDelayChar, sizeof(PressKeyBonusDelayChar), p.PressKeyBonusDelayChar, _TRUNCATE);
		section_toggles[5] = p.section_enabled;
	}
	if (!spamkey_instances.empty()) {
		auto& s = spamkey_instances[0];
		isspamswitch   = s.isspamswitch;
		disable_outside_roblox[11] = s.disable_outside_roblox;
		spam_delay     = s.spam_delay;    real_delay     = s.real_delay;
		vk_spamkey     = s.vk_spamkey;   vk_leftbracket = s.vk_trigger;
		strncpy_s(SpamDelay, sizeof(SpamDelay), s.SpamDelay, _TRUNCATE);
		section_toggles[11] = s.section_enabled;
	}

	freezeoutsideroblox = !disable_outside_roblox[0];
	unequipinroblox = disable_outside_roblox[4];

	for (const auto& [key, ptr] : bool_vars) {
		if (ptr) data[key] = *ptr;
	}
	for (const auto& [key, var] : numeric_vars) {
		std::visit([&](auto&& arg) {
			if (arg) data[key] = *arg;
		}, var);
	}
	for (const auto& [key, cfg] : char_arrays) {
		if (cfg.first) data[key] = TrimNullChars(cfg.first, cfg.second);
	}

	if (section_amounts > 0) {
		data["section_toggles"] = std::vector<bool>(section_toggles, section_toggles + section_amounts);
		data["disable_outside_roblox"] = std::vector<bool>(disable_outside_roblox, disable_outside_roblox + section_amounts);
		data["section_order_vector"] = std::vector<int>(section_order, section_order + section_amounts);
	}
	data["text"] = text;
	data["screen_width"] = screen_width;
	data["screen_height"] = screen_height;

	// Serialize extra wallhop instances (index 1+)
	if (wallhop_instances.size() > 1) {
		json whArr = json::array();
		for (size_t j = 1; j < wallhop_instances.size(); ++j) {
			const auto& w = wallhop_instances[j];
			json jw;
			jw["vk_trigger"]           = w.vk_trigger;
			jw["vk_jumpkey"]           = w.vk_jumpkey;
			jw["wallhop_dx"]           = w.wallhop_dx;
			jw["wallhop_dy"]           = w.wallhop_dy;
			jw["wallhop_vertical"]     = w.wallhop_vertical;
			jw["WallhopDelay"]         = w.WallhopDelay;
			jw["WallhopBonusDelay"]    = w.WallhopBonusDelay;
			jw["WallhopPixels"]        = TrimNullChars(w.WallhopPixels,         sizeof(WallhopInstance::WallhopPixels));
			jw["WallhopVerticalChar"]   = TrimNullChars(w.WallhopVerticalChar,   sizeof(WallhopInstance::WallhopVerticalChar));
			jw["WallhopDelayChar"]     = TrimNullChars(w.WallhopDelayChar,      sizeof(WallhopInstance::WallhopDelayChar));
			jw["WallhopBonusDelayChar"]= TrimNullChars(w.WallhopBonusDelayChar, sizeof(WallhopInstance::WallhopBonusDelayChar));
			jw["WallhopDegrees"]       = TrimNullChars(w.WallhopDegrees,        sizeof(WallhopInstance::WallhopDegrees));
			jw["wallhopswitch"]        = w.wallhopswitch;
			jw["toggle_jump"]          = w.toggle_jump;
			jw["toggle_flick"]         = w.toggle_flick;
			jw["wallhopcamfix"]        = w.wallhopcamfix;
			jw["disable_outside_roblox"] = w.disable_outside_roblox;
			jw["section_enabled"]      = w.section_enabled;
			whArr.push_back(jw);
		}
		data["extra_wallhop_instances"] = whArr;
	}
	// Serialize extra presskey instances (index 1+)
	if (presskey_instances.size() > 1) {
		json pkArr = json::array();
		for (size_t j = 1; j < presskey_instances.size(); ++j) {
			const auto& p = presskey_instances[j];
			json jp;
			jp["vk_trigger"]           = p.vk_trigger;
			jp["vk_presskey"]          = p.vk_presskey;
			jp["PressKeyDelay"]        = p.PressKeyDelay;
			jp["PressKeyBonusDelay"]   = p.PressKeyBonusDelay;
			jp["PressKeyDelayChar"]    = TrimNullChars(p.PressKeyDelayChar,      sizeof(PresskeyInstance::PressKeyDelayChar));
			jp["PressKeyBonusDelayChar"]= TrimNullChars(p.PressKeyBonusDelayChar, sizeof(PresskeyInstance::PressKeyBonusDelayChar));
			jp["disable_outside_roblox"] = p.presskeyinroblox;
			jp["presskeyinroblox"]     = p.presskeyinroblox;
			jp["section_enabled"]      = p.section_enabled;
			pkArr.push_back(jp);
		}
		data["extra_presskey_instances"] = pkArr;
	}
	// Serialize extra spamkey instances (index 1+)
	if (spamkey_instances.size() > 1) {
		json skArr = json::array();
		for (size_t j = 1; j < spamkey_instances.size(); ++j) {
			const auto& s = spamkey_instances[j];
			json js;
			js["vk_trigger"]    = s.vk_trigger;
			js["vk_spamkey"]    = s.vk_spamkey;
			js["spam_delay"]    = s.spam_delay;
			js["real_delay"]    = s.real_delay;
			js["SpamDelay"]     = TrimNullChars(s.SpamDelay, sizeof(SpamkeyInstance::SpamDelay));
			js["isspamswitch"]  = s.isspamswitch;
			js["disable_outside_roblox"] = s.disable_outside_roblox;
			js["section_enabled"]= s.section_enabled;
			skArr.push_back(js);
		}
		data["extra_spamkey_instances"] = skArr;
	}

	return data;
}

static void DeserializeProfileData(const json& settings) {
	try {
		// Load booleans
		for (const auto& [key, ptr] : bool_vars) {
			if (ptr && settings.contains(key) && settings[key].is_boolean()) {
				*ptr = settings[key].get<bool>();
			}
		}

		// Load numerics
		for (const auto& [key, var] : numeric_vars) {
			if (!settings.contains(key)) continue;
			std::visit([&](auto&& arg) {
				using T = std::decay_t<decltype(*arg)>;
				if (arg && settings[key].is_number()) {
					*arg = settings[key].get<T>();
				}
			}, var);
		}

		// Load char arrays
		for (const auto& [key, cfg] : char_arrays) {
			if (cfg.first && settings.contains(key) && settings[key].is_string()) {
				std::string str_val = settings[key].get<std::string>();
				strncpy_s(cfg.first, cfg.second, str_val.c_str(), cfg.second - 1);
				cfg.first[cfg.second - 1] = '\0';
			}
		}

		// Section toggles
		if (settings.contains("section_toggles") && settings["section_toggles"].is_array()) {
			auto toggles = settings["section_toggles"].get<std::vector<bool>>();
			size_t count = std::min(toggles.size(), static_cast<size_t>(section_amounts));
			std::copy(toggles.begin(), toggles.begin() + count, section_toggles);
		}

		bool loaded_disable_outside = false;
		if (settings.contains("disable_outside_roblox") && settings["disable_outside_roblox"].is_array()) {
			auto disableOutside = settings["disable_outside_roblox"].get<std::vector<bool>>();
			size_t count = std::min(disableOutside.size(), static_cast<size_t>(section_amounts));
			std::copy(disableOutside.begin(), disableOutside.begin() + count, disable_outside_roblox);
			loaded_disable_outside = true;
		}

		if (!loaded_disable_outside) {
			disable_outside_roblox[0] = !freezeoutsideroblox;
			disable_outside_roblox[4] = unequipinroblox;
			disable_outside_roblox[5] = presskeyinroblox;
		}

		freezeoutsideroblox = !disable_outside_roblox[0];
		unequipinroblox = disable_outside_roblox[4];
		presskeyinroblox = disable_outside_roblox[5];

		// Section order (with backwards-compatible upgrades for older save files)
		if (settings.contains("section_order_vector") && settings["section_order_vector"].is_array()) {
			auto order = settings["section_order_vector"].get<std::vector<int>>();

			// Inject missing section IDs from older save file versions:
			// Bunnyhop (section 13) was added later — insert at position 7
			if (std::find(order.begin(), order.end(), 13) == order.end() && order.size() >= 7) {
				order.insert(order.begin() + 7, 13);
			}
			// Lagswitch (section 15) was added later — insert at position 4
			if (std::find(order.begin(), order.end(), 15) == order.end() && order.size() >= 5) {
				order.insert(order.begin() + 4, 15);
			}

			// Deduplicate and fill any still-missing section IDs
			std::vector<int> final_order;
			std::vector<bool> id_seen(section_amounts, false);

			for (int id : order) {
				if (id >= 0 && id < section_amounts && !id_seen[id]) {
					final_order.push_back(id);
					id_seen[id] = true;
				}
			}
			for (int i = 0; i < section_amounts; ++i) {
				if (!id_seen[i]) final_order.push_back(i);
			}

			size_t count = std::min(final_order.size(), static_cast<size_t>(section_amounts));
			for (size_t i = 0; i < count; ++i) {
				section_order[i] = final_order[i];
			}
		}

		if (settings.contains("text") && settings["text"].is_string()) {
			text = settings["text"].get<std::string>();
		}
		if (settings.contains("screen_width") && settings["screen_width"].is_number_integer()) {
			screen_width = settings.value("screen_width", screen_width);
		}
		if (settings.contains("screen_height") && settings["screen_height"].is_number_integer()) {
			screen_height = settings.value("screen_height", screen_height);
		}

		// Sync flat globals → instances[0] (for backwards compatibility)
		if (!wallhop_instances.empty()) {
			auto& w = wallhop_instances[0];
			w.wallhopswitch = wallhopswitch;  w.wallhopcamfix = wallhopcamfix;
			w.toggle_jump   = toggle_jump;    w.toggle_flick  = toggle_flick;
			w.wallhop_dx    = wallhop_dx;     w.wallhop_dy    = wallhop_dy;
			w.wallhop_vertical = wallhop_vertical;
			w.disable_outside_roblox = disable_outside_roblox[6];
			w.vk_trigger    = vk_xbutton2;
			w.vk_jumpkey    = vk_wallhopjumpkey;
			strncpy_s(w.WallhopPixels,         sizeof(WallhopInstance::WallhopPixels),         WallhopPixels,         _TRUNCATE);
			strncpy_s(w.WallhopVerticalChar,   sizeof(WallhopInstance::WallhopVerticalChar),   WallhopVerticalChar,   _TRUNCATE);
			strncpy_s(w.WallhopDelayChar,      sizeof(WallhopInstance::WallhopDelayChar),      WallhopDelayChar,      _TRUNCATE);
			strncpy_s(w.WallhopBonusDelayChar, sizeof(WallhopInstance::WallhopBonusDelayChar), WallhopBonusDelayChar, _TRUNCATE);
			w.section_enabled = section_toggles[6];
			try { w.WallhopDelay      = std::stoi(WallhopDelayChar); }      catch (...) {}
			try { w.WallhopBonusDelay = std::stoi(WallhopBonusDelayChar); } catch (...) {}
		}
		if (!presskey_instances.empty()) {
			auto& p = presskey_instances[0];
			p.presskeyinroblox = presskeyinroblox;
			p.vk_trigger = vk_zkey;  p.vk_presskey = vk_dkey;
			strncpy_s(p.PressKeyDelayChar,      sizeof(PresskeyInstance::PressKeyDelayChar),      PressKeyDelayChar,      _TRUNCATE);
			strncpy_s(p.PressKeyBonusDelayChar, sizeof(PresskeyInstance::PressKeyBonusDelayChar), PressKeyBonusDelayChar, _TRUNCATE);
			p.section_enabled = section_toggles[5];
			try { p.PressKeyDelay      = std::stoi(PressKeyDelayChar); }      catch (...) {}
			try { p.PressKeyBonusDelay = std::stoi(PressKeyBonusDelayChar); } catch (...) {}
		}
		if (!spamkey_instances.empty()) {
			auto& s = spamkey_instances[0];
			s.isspamswitch = isspamswitch;
			s.disable_outside_roblox = disable_outside_roblox[11];
			s.spam_delay   = spam_delay;   s.real_delay = real_delay;
			s.vk_spamkey   = vk_spamkey;  s.vk_trigger = vk_leftbracket;
			strncpy_s(s.SpamDelay, sizeof(SpamkeyInstance::SpamDelay), SpamDelay, _TRUNCATE);
			s.section_enabled = section_toggles[11];
		}

		// Load extra instances (index 1+) that were saved in previous sessions.
		// Threads for these will be started by the main loop when it detects g_extra_instances_loaded.
		bool extra_loaded = false;
		if (settings.contains("extra_wallhop_instances") && settings["extra_wallhop_instances"].is_array()) {
			for (const auto& jw : settings["extra_wallhop_instances"]) {
				wallhop_instances.emplace_back();
				auto& w = wallhop_instances.back();
				w.vk_trigger     = jw.value("vk_trigger", static_cast<unsigned int>(VK_XBUTTON2));
				w.vk_jumpkey     = jw.value("vk_jumpkey", static_cast<unsigned int>(VK_SPACE));
				w.wallhop_dx     = jw.value("wallhop_dx", 300);
				w.wallhop_dy     = jw.value("wallhop_dy", -300);
				w.wallhop_vertical = jw.value("wallhop_vertical", 0);
				w.WallhopDelay   = jw.value("WallhopDelay", 17);
				w.WallhopBonusDelay = jw.value("WallhopBonusDelay", 0);
				if (jw.contains("WallhopPixels") && jw["WallhopPixels"].is_string()) {
					std::string s = jw["WallhopPixels"].get<std::string>();
					strncpy_s(w.WallhopPixels, sizeof(WallhopInstance::WallhopPixels), s.c_str(), _TRUNCATE);
				}
				if (jw.contains("WallhopVerticalChar") && jw["WallhopVerticalChar"].is_string()) {
					std::string s = jw["WallhopVerticalChar"].get<std::string>();
					strncpy_s(w.WallhopVerticalChar, sizeof(WallhopInstance::WallhopVerticalChar), s.c_str(), _TRUNCATE);
				}
				if (jw.contains("WallhopDelayChar") && jw["WallhopDelayChar"].is_string()) {
					std::string s = jw["WallhopDelayChar"].get<std::string>();
					strncpy_s(w.WallhopDelayChar, sizeof(WallhopInstance::WallhopDelayChar), s.c_str(), _TRUNCATE);
				}
				if (jw.contains("WallhopBonusDelayChar") && jw["WallhopBonusDelayChar"].is_string()) {
					std::string s = jw["WallhopBonusDelayChar"].get<std::string>();
					strncpy_s(w.WallhopBonusDelayChar, sizeof(WallhopInstance::WallhopBonusDelayChar), s.c_str(), _TRUNCATE);
				}
				if (jw.contains("WallhopDegrees") && jw["WallhopDegrees"].is_string()) {
					std::string s = jw["WallhopDegrees"].get<std::string>();
					strncpy_s(w.WallhopDegrees, sizeof(WallhopInstance::WallhopDegrees), s.c_str(), _TRUNCATE);
				}
				w.wallhopswitch   = jw.value("wallhopswitch", false);
				w.toggle_jump     = jw.value("toggle_jump", true);
				w.toggle_flick    = jw.value("toggle_flick", true);
				w.wallhopcamfix   = jw.value("wallhopcamfix", false);
				w.disable_outside_roblox = jw.value("disable_outside_roblox", disable_outside_roblox[6]);
				w.section_enabled = jw.value("section_enabled", false);
				extra_loaded = true;
			}
		}
		if (settings.contains("extra_presskey_instances") && settings["extra_presskey_instances"].is_array()) {
			for (const auto& jp : settings["extra_presskey_instances"]) {
				presskey_instances.emplace_back();
				auto& p = presskey_instances.back();
				p.vk_trigger        = jp.value("vk_trigger",  static_cast<unsigned int>(0x5A));
				p.vk_presskey       = jp.value("vk_presskey", static_cast<unsigned int>(0x44));
				p.PressKeyDelay     = jp.value("PressKeyDelay", 16);
				p.PressKeyBonusDelay= jp.value("PressKeyBonusDelay", 0);
				if (jp.contains("PressKeyDelayChar") && jp["PressKeyDelayChar"].is_string()) {
					std::string s = jp["PressKeyDelayChar"].get<std::string>();
					strncpy_s(p.PressKeyDelayChar, sizeof(PresskeyInstance::PressKeyDelayChar), s.c_str(), _TRUNCATE);
				}
				if (jp.contains("PressKeyBonusDelayChar") && jp["PressKeyBonusDelayChar"].is_string()) {
					std::string s = jp["PressKeyBonusDelayChar"].get<std::string>();
					strncpy_s(p.PressKeyBonusDelayChar, sizeof(PresskeyInstance::PressKeyBonusDelayChar), s.c_str(), _TRUNCATE);
				}
				p.presskeyinroblox = jp.value("disable_outside_roblox", jp.value("presskeyinroblox", disable_outside_roblox[5]));
				p.section_enabled  = jp.value("section_enabled", false);
				extra_loaded = true;
			}
		}
		if (settings.contains("extra_spamkey_instances") && settings["extra_spamkey_instances"].is_array()) {
			for (const auto& js : settings["extra_spamkey_instances"]) {
				spamkey_instances.emplace_back();
				auto& s = spamkey_instances.back();
				s.vk_trigger    = js.value("vk_trigger",  static_cast<unsigned int>(0xDB));
				s.vk_spamkey    = js.value("vk_spamkey",  static_cast<unsigned int>(VK_LBUTTON));
				s.spam_delay    = js.value("spam_delay",  20.0f);
				s.real_delay    = js.value("real_delay",  1000);
				if (js.contains("SpamDelay") && js["SpamDelay"].is_string()) {
					std::string ss = js["SpamDelay"].get<std::string>();
					strncpy_s(s.SpamDelay, sizeof(SpamkeyInstance::SpamDelay), ss.c_str(), _TRUNCATE);
				}
				s.isspamswitch  = js.value("isspamswitch", false);
				s.disable_outside_roblox = js.value("disable_outside_roblox", disable_outside_roblox[11]);
				s.section_enabled = js.value("section_enabled", false);
				extra_loaded = true;
			}
		}
		if (!wallhop_instances.empty()) {
			if (selected_wallhop_instance < 0) {
				selected_wallhop_instance = 0;
			}
			if (selected_wallhop_instance >= static_cast<int>(wallhop_instances.size())) {
				selected_wallhop_instance = static_cast<int>(wallhop_instances.size()) - 1;
			}
		}
		if (extra_loaded) {
			g_extra_instances_loaded.store(true, std::memory_order_release);
		}
	} catch (const json::exception& e) {
		std::cerr << "Error deserializing profile data: " << e.what() << std::endl;
	}
}

// ============================================================================
//  THEME SERIALIZATION — Save/Load theme data to/from metadata
// ============================================================================

static auto rnd(float val) -> double { return std::floor(val * 1000.0f + 0.5f) / 1000.0; }

static json ColorToJson(const ImVec4& col) {
	return {rnd(col.x), rnd(col.y), rnd(col.z), rnd(col.w)};
}

static ImVec4 JsonToColor(const json& j, const std::string& key) {
	if (j.contains(key) && j[key].is_array() && j[key].size() == 4) {
		return ImVec4(j[key][0], j[key][1], j[key][2], j[key][3]);
	}
	return ImVec4(0.0f, 0.0f, 0.0f, 1.0f);
}

static json SerializeTheme(const Theme& theme) {
	json t;
	t["name"]             = theme.name;
	t["bg_dark"]          = ColorToJson(theme.bg_dark);
	t["bg_medium"]        = ColorToJson(theme.bg_medium);
	t["bg_light"]         = ColorToJson(theme.bg_light);
	t["accent_primary"]   = ColorToJson(theme.accent_primary);
	t["accent_secondary"] = ColorToJson(theme.accent_secondary);
	t["text_primary"]     = ColorToJson(theme.text_primary);
	t["text_secondary"]   = ColorToJson(theme.text_secondary);
	t["success_color"]    = ColorToJson(theme.success_color);
	t["warning_color"]    = ColorToJson(theme.warning_color);
	t["error_color"]      = ColorToJson(theme.error_color);
	t["border_color"]     = ColorToJson(theme.border_color);
	t["disabled_color"]   = ColorToJson(theme.disabled_color);
	t["window_rounding"]  = rnd(theme.window_rounding);
	t["frame_rounding"]   = rnd(theme.frame_rounding);
	t["button_rounding"]  = rnd(theme.button_rounding);
	return t;
}

static Theme DeserializeTheme(const json& t, const std::string& fallback_name) {
	return Theme{
		t.value("name", fallback_name),
		JsonToColor(t, "bg_dark"),
		JsonToColor(t, "bg_medium"),
		JsonToColor(t, "bg_light"),
		JsonToColor(t, "accent_primary"),
		JsonToColor(t, "accent_secondary"),
		JsonToColor(t, "text_primary"),
		JsonToColor(t, "text_secondary"),
		JsonToColor(t, "success_color"),
		JsonToColor(t, "warning_color"),
		JsonToColor(t, "error_color"),
		JsonToColor(t, "border_color"),
		JsonToColor(t, "disabled_color"),
		t.value("window_rounding", 0.0f),
		t.value("frame_rounding", 0.0f),
		t.value("button_rounding", 0.0f)
	};
}

static void SaveMetadataThemes(json& metadata) {
	metadata["current_theme_index"] = Globals::current_theme_index;
	metadata["show_theme_menu"] = Globals::show_theme_menu;
	metadata["custom_theme"] = SerializeTheme(Globals::custom_theme);

	metadata["themes"] = json::object();
	for (const auto& theme : Globals::themes) {
		metadata["themes"][theme.name] = SerializeTheme(theme);
	}
}

static void LoadMetadataThemes(const json& metadata) {
	Globals::current_theme_index = metadata.value("current_theme_index", 0);

	if (metadata.contains("custom_theme")) {
		const auto& ct = metadata["custom_theme"];
		Globals::custom_theme = DeserializeTheme(ct, "Custom Theme");
	}

	if (metadata.contains("themes") && metadata["themes"].is_object()) {
		for (const auto& [key, theme_json] : metadata["themes"].items()) {
			Theme loaded = DeserializeTheme(theme_json, key);

			bool found = false;
			for (auto& existing : Globals::themes) {
				if (existing.name == loaded.name) {
					existing = loaded;
					found = true;
					break;
				}
			}
			if (!found) {
				Globals::themes.push_back(loaded);
			}
		}
	}
}

// ============================================================================
//  METADATA I/O — Save/Load global settings that live in _metadata
// ============================================================================

static void SaveMetadata(json& root) {
	if (!root.contains(METADATA_KEY) || !root[METADATA_KEY].is_object()) {
		root[METADATA_KEY] = json::object();
	}
	auto& meta = root[METADATA_KEY];

	if (!G_CURRENTLY_LOADED_PROFILE_NAME.empty()) {
		meta[LAST_ACTIVE_PROFILE_KEY] = G_CURRENTLY_LOADED_PROFILE_NAME;
	}

	meta["shortdescriptions"] = shortdescriptions;
	meta["DontShowAdminWarning"] = DontShowAdminWarning;

	SaveMetadataThemes(meta);
}

static void LoadMetadata(const json& root) {
	if (!root.contains(METADATA_KEY) || !root[METADATA_KEY].is_object()) return;
	const auto& meta = root[METADATA_KEY];

	if (meta.contains("shortdescriptions") && meta["shortdescriptions"].is_boolean()) {
		shortdescriptions = meta["shortdescriptions"].get<bool>();
	}
	if (meta.contains("DontShowAdminWarning") && meta["DontShowAdminWarning"].is_boolean()) {
		DontShowAdminWarning = meta["DontShowAdminWarning"].get<bool>();
	}

	LoadMetadataThemes(meta);
}

// ============================================================================
//  PROFILE NAMES + UNIQUE NAME GENERATION
// ============================================================================

std::vector<std::string> GetProfileNames(const std::string& filepath) {
	std::vector<std::string> names;

	auto result = ReadJsonFile(filepath);
	if (!result.success || !result.data.is_object()) return names;

	for (const auto& [key, value] : result.data.items()) {
		if (key == METADATA_KEY) continue;
		if (value.is_object()) {
			names.push_back(key);
		}
	}

	std::sort(names.begin(), names.end());

	// Always put (default) first
	auto it = std::find(names.begin(), names.end(), "(default)");
	if (it != names.end()) {
		names.erase(it);
		names.insert(names.begin(), "(default)");
	}

	return names;
}

std::string GenerateUniqueProfileName(const std::string& base_name, const std::vector<std::string>& existing_names) {
	std::string candidate = base_name + " (Copy)";
	if (std::find(existing_names.begin(), existing_names.end(), candidate) == existing_names.end()) {
		return candidate;
	}
	for (int i = 2; i <= 1000; ++i) {
		candidate = base_name + " (Copy " + std::to_string(i) + ")";
		if (std::find(existing_names.begin(), existing_names.end(), candidate) == existing_names.end()) {
			return candidate;
		}
	}
	return base_name + " (Copy Error)";
}

static std::string GenerateNewProfileName(const std::vector<std::string>& existing_names) {
	for (int i = 1; i <= 1000; ++i) {
		std::string candidate = "Profile " + std::to_string(i);
		if (std::find(existing_names.begin(), existing_names.end(), candidate) == existing_names.end()) {
			return candidate;
		}
	}
	return "Profile Error";
}

// ============================================================================
//  CORE API: SaveSettings
//  Saves the current global state as a named profile in the settings file.
//  (default) is read-only — this function will not overwrite it.
// ============================================================================

void SaveSettings(const std::string& filepath, const std::string& profile_name) {
	if (profile_name.empty() || profile_name == "(default)") return;

	json profile_data = SerializeProfileData();

	// Read existing file to preserve other profiles, or start fresh
	json root = json::object();
	if (fs::exists(filepath)) {
		auto file_result = ReadJsonFile(filepath);
		if (file_result.success && file_result.data.is_object()) {
			if (IsProfileFormat(file_result.data)) {
				root = file_result.data;
			} else if (IsLegacyFlatFormat(file_result.data)) {
				// Migrate old flat-format data under "Profile 1"
				root["Profile 1"] = file_result.data;
				std::cout << "Old format detected during save. Migrating old data to 'Profile 1'." << std::endl;
			} else {
				// Unknown structure — preserve as-is and add our profile on top
				root = file_result.data;
			}
		}
	}

	root[profile_name] = profile_data;
	SaveMetadata(root);
	WriteJsonFile(filepath, root);
}

// ============================================================================
//  CORE API: SaveDefaultProfile
//  Writes current default values as the read-only (default) profile.
//  Called once at startup so new settings fields always have a fallback.
// ============================================================================

void SaveDefaultProfile(const std::string& filepath) {
	json default_data = SerializeProfileData();

	json root = json::object();
	if (fs::exists(filepath)) {
		auto file_result = ReadJsonFile(filepath);
		if (file_result.success && file_result.data.is_object()) {
			root = file_result.data;
		}
	}

	root["(default)"] = default_data;

	if (!root.contains(METADATA_KEY) || !root[METADATA_KEY].is_object()) {
		root[METADATA_KEY] = json::object();
	}

	WriteJsonFile(filepath, root);
}

// ============================================================================
//  CORE API: LoadSettings
//  Loads a named profile from the settings file into global state.
//  Falls back through: exact match → last_active → any profile → legacy flat.
// ============================================================================

void LoadSettings(std::string filepath, std::string profile_name) {
	if (profile_name.empty()) return;

	auto file_result = ReadJsonFile(filepath);
	if (!file_result.success) {
		std::cerr << "Info: Settings file '" << filepath << "' not found or corrupt. Using defaults." << std::endl;
		return;
	}

	json& root = file_result.data;

	// Always load global metadata (themes, etc.) regardless of which profile we load
	LoadMetadata(root);

	// Determine which profile to actually load
	json settings_to_load;
	std::string actual_profile;
	bool found = false;

	if (IsProfileFormat(root) || (root.is_object() && !IsLegacyFlatFormat(root))) {
		// New-format or unknown-but-structured file: use FindBestProfile
		actual_profile = FindBestProfile(root, profile_name);
		if (!actual_profile.empty() && root.contains(actual_profile)) {
			settings_to_load = root[actual_profile];
			found = true;
		}
	} else if (IsLegacyFlatFormat(root)) {
		// Old flat format: treat root as profile data directly
		settings_to_load = root;
		actual_profile = "Profile 1";
		found = true;
		std::cout << "Info: Loaded legacy flat-format file as 'Profile 1'." << std::endl;
	}

	if (!found) {
		std::cerr << "Warning: Profile '" << profile_name << "' not found in '" << filepath << "'." << std::endl;
		return;
	}

	DeserializeProfileData(settings_to_load);
}

// ============================================================================
//  CORE API: TryLoadLastActiveProfile
//  Called at startup. Finds the settings file, loads the last active profile.
//  Handles legacy format conversion and creates "Profile 1" if nothing exists.
// ============================================================================

bool TryLoadLastActiveProfile(std::string filepath) {
	// File must exist (ResolveSettingsFilePath is called before this)
	auto file_result = ReadJsonFile(filepath);
	if (!file_result.success) {
		std::cerr << "TryLoadLastActiveProfile: Could not read '" << filepath << "'." << std::endl;
		return false;
	}

	json& root = file_result.data;

	// Always load global metadata first
	LoadMetadata(root);

	// Handle legacy flat format: convert in-place
	if (IsLegacyFlatFormat(root)) {
		DeserializeProfileData(root);
		G_CURRENTLY_LOADED_PROFILE_NAME = "Profile 1";
		SaveSettings(filepath, "Profile 1");
		std::cout << "Converted legacy settings to 'Profile 1'." << std::endl;
		return true;
	}

	if (!root.is_object()) return false;

	// Find the best profile to load
	std::string best = FindBestProfile(root, "");
	if (best.empty()) {
		// No profiles at all — create "Profile 1" with current defaults
		G_CURRENTLY_LOADED_PROFILE_NAME = "Profile 1";
		SaveSettings(filepath, "Profile 1");
		std::cout << "No profiles found. Created 'Profile 1'." << std::endl;
		return true;
	}

	// Load the found profile
	if (root.contains(best) && root[best].is_object()) {
		DeserializeProfileData(root[best]);
		G_CURRENTLY_LOADED_PROFILE_NAME = best;
		std::cout << "Loaded profile: " << best << std::endl;
		return true;
	}

	return false;
}

// ============================================================================
//  CORE API: PromoteDefaultProfileIfDirty
//  Called on application quit. If current profile is (default), always save to
//  a new "Profile N" — (default) is read-only and must never hold user state.
// ============================================================================

std::string PromoteDefaultProfileIfDirty(const std::string& filepath) {
	if (G_CURRENTLY_LOADED_PROFILE_NAME != "(default)") return "";

	auto names = GetProfileNames(filepath);
	std::string new_name = GenerateNewProfileName(names);

	G_CURRENTLY_LOADED_PROFILE_NAME = new_name;
	SaveSettings(filepath, new_name);

	std::cout << "Auto-saved (default) edits as '" << new_name << "'." << std::endl;
	return new_name;
}

// ============================================================================
//  PROFILE CRUD — Delete, Rename, Duplicate
// ============================================================================

bool DeleteProfileFromFile(const std::string& filepath, const std::string& profile_name) {
	if (profile_name == "(default)") return false;

	auto file_result = ReadJsonFile(filepath);
	if (!file_result.success || !file_result.data.is_object()) return false;

	json& root = file_result.data;
	if (!root.contains(profile_name)) return false;

	root.erase(profile_name);

	if (WriteJsonFile(filepath, root)) {
		if (G_CURRENTLY_LOADED_PROFILE_NAME == profile_name) {
			G_CURRENTLY_LOADED_PROFILE_NAME = "";
			std::cout << "Info: Deleted active profile '" << profile_name << "'." << std::endl;
		}
		return true;
	}
	return false;
}

bool RenameProfileInFile(const std::string& filepath, const std::string& old_name, const std::string& new_name) {
	if (old_name == new_name) return true;
	if (old_name == "(default)" || new_name == "(default)") return false;

	auto file_result = ReadJsonFile(filepath);
	if (!file_result.success || !file_result.data.is_object()) return false;

	json& root = file_result.data;
	if (!root.contains(old_name)) return false;
	if (root.contains(new_name)) {
		std::cerr << "Rename Error: Target name '" << new_name << "' already exists." << std::endl;
		return false;
	}

	json profile_data = root[old_name];
	root.erase(old_name);
	root[new_name] = profile_data;

	if (WriteJsonFile(filepath, root)) {
		if (G_CURRENTLY_LOADED_PROFILE_NAME == old_name) {
			G_CURRENTLY_LOADED_PROFILE_NAME = new_name;
		}
		return true;
	}
	return false;
}

bool DuplicateProfileInFile(const std::string& filepath, const std::string& source_name, const std::string& new_name) {
	auto file_result = ReadJsonFile(filepath);
	if (!file_result.success || !file_result.data.is_object()) return false;

	json& root = file_result.data;
	if (!root.contains(source_name)) return false;
	if (root.contains(new_name)) {
		std::cerr << "Duplicate Error: Target name '" << new_name << "' already exists." << std::endl;
		return false;
	}

	root[new_name] = root[source_name];
	return WriteJsonFile(filepath, root);
}

// ============================================================================
//  UI — ImGui Profile Manager (unchanged behavior, cleaned up structure)
// ============================================================================

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

	// Delete button confirmation state
	static int s_delete_button_confirmation_stage = 0;
	static float s_delete_button_stage_timer = 0.0f;
	static int s_target_profile_idx_for_confirm = -1;
	static std::string s_G_LOADED_PROFILE_NAME_at_confirm_start = "";
	bool delete_action_requested_this_frame = false;

	void RefreshProfileListAndSelection() {
		std::string previously_selected_name;
		if (s_selected_profile_idx >= 0 && s_selected_profile_idx < (int)s_profile_names.size()) {
			previously_selected_name = s_profile_names[s_selected_profile_idx];
		}

		s_profile_names = GetProfileNames(G_SETTINGS_FILEPATH);

		s_selected_profile_idx = -1;
		if (!previously_selected_name.empty()) {
			auto it = std::find(s_profile_names.begin(), s_profile_names.end(), previously_selected_name);
			if (it != s_profile_names.end()) {
				s_selected_profile_idx = distance_to_int(std::distance(s_profile_names.begin(), it));
			}
		}
		if (s_selected_profile_idx == -1 && !G_CURRENTLY_LOADED_PROFILE_NAME.empty()) {
			auto it = std::find(s_profile_names.begin(), s_profile_names.end(), G_CURRENTLY_LOADED_PROFILE_NAME);
			if (it != s_profile_names.end()) {
				s_selected_profile_idx = distance_to_int(std::distance(s_profile_names.begin(), it));
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

		if (ImGui::Button(s_expanded ? "Profiles <-" : "Profiles ->", ImVec2(270, 0))) {
			s_expanded = !s_expanded;
			if (s_expanded) {
				RefreshProfileListAndSelection();
				s_rename_error_msg = "";
			} else {
				s_editing_profile_idx = -1;
			}
		}

		if (s_expanded) {
			ImVec2 buttonPos = ImGui::GetItemRectMin();
			float menuWidth = ImGui::GetItemRectSize().x;

			float list_item_height = ImGui::GetTextLineHeightWithSpacing();
			int num_items_to_show = std::min(10, (int)s_profile_names.size());
			if (num_items_to_show == 0) num_items_to_show = 3;
			float buttons_height = ImGui::GetFrameHeightWithSpacing() * 2.0f + ImGui::GetStyle().ItemSpacing.y * 2.0f;
			float list_height = num_items_to_show * list_item_height + ImGui::GetStyle().WindowPadding.y * 2;
			float menuHeight = buttons_height + list_height + ImGui::GetStyle().SeparatorTextAlign.y;
			int old_s_selected_profile_idx = -1;
			menuHeight = std::min(menuHeight, 300.0f);

			ImGui::SetNextWindowPos(ImVec2(buttonPos.x, buttonPos.y - menuHeight - ImGui::GetStyle().WindowPadding.y + 3));
			ImGui::SetNextWindowSize(ImVec2(menuWidth, menuHeight));

			ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(5, 5));
			ImGui::Begin("##ProfilesDropUpMenu", NULL, ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoSavedSettings);

			float actionButtonWidth = (ImGui::GetContentRegionAvail().x - ImGui::GetStyle().ItemSpacing.x * 2) / 3.0f;
			bool profile_is_selected = (s_selected_profile_idx != -1 && s_selected_profile_idx < (int)s_profile_names.size());

			// Auto-save current profile when selection changes
			if (old_s_selected_profile_idx != s_selected_profile_idx) {
				SaveSettings(G_SETTINGS_FILEPATH, G_CURRENTLY_LOADED_PROFILE_NAME);
			}
			if (profile_is_selected) {
				old_s_selected_profile_idx = s_selected_profile_idx;
			}

			// --- "Save To" Button ---
			if (ImGui::Button("Save To", ImVec2(actionButtonWidth, 0))) {
				std::string profileToSave;

				if (profile_is_selected) {
					profileToSave = s_profile_names[s_selected_profile_idx];
				} else {
					profileToSave = GenerateNewProfileName(s_profile_names);
				}

				// If trying to save to (default), create a new profile instead
				if (profileToSave == "(default)") {
					profileToSave = GenerateNewProfileName(s_profile_names);
				}

				G_CURRENTLY_LOADED_PROFILE_NAME = profileToSave;
				SaveSettings(G_SETTINGS_FILEPATH, profileToSave);
				RefreshProfileListAndSelection();
				auto it = std::find(s_profile_names.begin(), s_profile_names.end(), profileToSave);
				if (it != s_profile_names.end()) {
					s_selected_profile_idx = distance_to_int(std::distance(s_profile_names.begin(), it));
				}
				s_editing_profile_idx = -1;
			}

			// --- "Load" Button ---
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

			// --- "Delete" Button ---
			if (!profile_is_selected) ImGui::BeginDisabled();

			ImGuiIO &io = ImGui::GetIO();

			// Manage delete confirmation timer
			if (s_delete_button_confirmation_stage != 0) {
				s_delete_button_stage_timer -= io.DeltaTime;
				bool should_reset = false;
				if (s_delete_button_stage_timer <= 0.0f) should_reset = true;
				if (s_target_profile_idx_for_confirm != -1 && s_selected_profile_idx != s_target_profile_idx_for_confirm) should_reset = true;
				if (!s_G_LOADED_PROFILE_NAME_at_confirm_start.empty() &&
					G_CURRENTLY_LOADED_PROFILE_NAME != s_G_LOADED_PROFILE_NAME_at_confirm_start) should_reset = true;

				if (should_reset) {
					s_delete_button_confirmation_stage = 0;
					s_target_profile_idx_for_confirm = -1;
					s_G_LOADED_PROFILE_NAME_at_confirm_start = "";
					s_delete_button_stage_timer = 0.0f;
				}
			}

			if (profile_is_selected && s_selected_profile_idx == s_target_profile_idx_for_confirm) {
				if (s_delete_button_confirmation_stage == 1) {
					current_button_bg_color = ImVec4(1.0f, 1.0f, 0.0f, 1.0f);
					current_button_text_color = ImVec4(0.0f, 0.0f, 0.0f, 1.0f);
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
						s_delete_button_confirmation_stage = 1;
						s_delete_button_stage_timer = 0.7f;
						s_target_profile_idx_for_confirm = s_selected_profile_idx;
						s_G_LOADED_PROFILE_NAME_at_confirm_start = G_CURRENTLY_LOADED_PROFILE_NAME;
					} else if (s_delete_button_confirmation_stage == 1 && s_selected_profile_idx == s_target_profile_idx_for_confirm) {
						s_delete_button_confirmation_stage = 2;

						std::string name_to_delete = s_profile_names[s_target_profile_idx_for_confirm];
						if (DeleteProfileFromFile(G_SETTINGS_FILEPATH, name_to_delete)) {
							RefreshProfileListAndSelection();
							// If only metadata remains, remove the file entirely
							auto check = ReadJsonFile(G_SETTINGS_FILEPATH);
							if (check.success && check.data.is_object() && check.data.size() == 1) {
								fs::remove(G_SETTINGS_FILEPATH);
							}
						}
						s_editing_profile_idx = -1;
					}
				}
			}

			delete_action_requested_this_frame = false;

			if (!profile_is_selected) ImGui::EndDisabled();

			// --- "Duplicate" Button ---
			if (!profile_is_selected) ImGui::BeginDisabled();
			if (ImGui::Button("Duplicate", ImVec2(-FLT_MIN, 0))) {
				if (profile_is_selected) {
					std::string source_name = s_profile_names[s_selected_profile_idx];
					if (source_name == "(default)") {
						// Duplicate (default): capture live globals, not stale file data
						std::string new_name = GenerateNewProfileName(s_profile_names);
						G_CURRENTLY_LOADED_PROFILE_NAME = new_name;
						SaveSettings(G_SETTINGS_FILEPATH, new_name);
						RefreshProfileListAndSelection();
						auto it = std::find(s_profile_names.begin(), s_profile_names.end(), new_name);
						if (it != s_profile_names.end()) {
							s_selected_profile_idx = distance_to_int(std::distance(s_profile_names.begin(), it));
						}
					} else {
						std::string new_name = GenerateUniqueProfileName(source_name, s_profile_names);
						if (DuplicateProfileInFile(G_SETTINGS_FILEPATH, source_name, new_name)) {
							LoadSettings(G_SETTINGS_FILEPATH, new_name);
							G_CURRENTLY_LOADED_PROFILE_NAME = new_name;
							RefreshProfileListAndSelection();
							auto it = std::find(s_profile_names.begin(), s_profile_names.end(), new_name);
							if (it != s_profile_names.end()) {
								s_selected_profile_idx = distance_to_int(std::distance(s_profile_names.begin(), it));
							}
						}
					}
					s_editing_profile_idx = -1;
				}
			}
			if (!profile_is_selected) ImGui::EndDisabled();
			ImGui::Separator();

			// --- Scrollable selectable profile list ---
			ImGui::BeginChild("##ProfilesOptionsList", ImVec2(0, ImGui::GetContentRegionAvail().y - ImGui::GetStyle().ItemSpacing.y - (s_rename_error_msg.empty() ? 0 : ImGui::GetTextLineHeightWithSpacing())), true, ImGuiWindowFlags_HorizontalScrollbar);
			{
				for (int i = 0; i < (int)s_profile_names.size(); ++i) {
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
							s_rename_error_msg = "";

							if (new_name_candidate == "(default)") {
								s_rename_error_msg = "Name cannot be default.";
							}
							if (new_name_candidate.empty()) {
								s_rename_error_msg = "Name cannot be empty.";
							} else {
								for (int j = 0; j < (int)s_profile_names.size(); ++j) {
									if (j != i && s_profile_names[j] == new_name_candidate) {
										s_rename_error_msg = "Name already exists.";
										break;
									}
								}
							}

							if (s_rename_error_msg.empty()) {
								if (RenameProfileInFile(G_SETTINGS_FILEPATH, s_profile_names[i], new_name_candidate)) {
									s_profile_names[i] = new_name_candidate;
									RefreshProfileListAndSelection();
									auto it = std::find(s_profile_names.begin(), s_profile_names.end(), new_name_candidate);
									if (it != s_profile_names.end()) {
										s_selected_profile_idx = distance_to_int(std::distance(s_profile_names.begin(), it));
									}
									std::cout << "Renamed profile to: " << new_name_candidate << std::endl;
								}
								s_editing_profile_idx = -1;
							}
						}
					} else {
						if (ImGui::Selectable(s_profile_names[i].c_str(), is_selected_this_item, ImGuiSelectableFlags_AllowDoubleClick)) {
							s_selected_profile_idx = i;
							if (ImGui::IsMouseDoubleClicked(0) && s_profile_names[i] != "(default)") {
								s_editing_profile_idx = i;
								strncpy_s(s_edit_buffer, sizeof(s_edit_buffer), s_profile_names[i].c_str(), sizeof(s_edit_buffer) - 1);
								s_edit_buffer[sizeof(s_edit_buffer) - 1] = '\0';
								s_last_click_time = -1.0f;
								s_rename_error_msg = "";
							} else {
								s_last_clicked_item_idx = i;
								s_last_click_time = static_cast<float>(ImGui::GetTime());
							}
						}
						if (is_selected_this_item) ImGui::SetItemDefaultFocus();
					}

					if (is_currently_loaded) ImGui::PopStyleColor();
					if (s_profile_names[i] == "(default)") ImGui::PopStyleColor();
				}
			}
			ImGui::EndChild();

			if (!s_rename_error_msg.empty()) {
				ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(1.0f, 0.0f, 0.0f, 1.0f));
				ImGui::TextWrapped("%s", s_rename_error_msg.c_str());
				ImGui::PopStyleColor();
			}

			ImGui::End();
			ImGui::PopStyleVar();
		}
	}
}
