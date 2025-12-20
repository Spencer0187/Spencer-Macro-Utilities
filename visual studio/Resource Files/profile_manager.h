#pragma once
#include <string>
#include <vector>


// --- Core Settings Logic ---
void SaveSettings(const std::string& filepath, const std::string& profile_name);
void LoadSettings(const std::string& filepath, const std::string& profile_name);
bool TryLoadLastActiveProfile(const std::string& filepath);

// --- Profile Manipulation ---
std::vector<std::string> GetProfileNames(const std::string& filepath);
bool DeleteProfileFromFile(const std::string& filepath, const std::string& profile_name);
bool RenameProfileInFile(const std::string& filepath, const std::string& old_name, const std::string& new_name);
bool DuplicateProfileInFile(const std::string& filepath, const std::string& source_name, const std::string& new_name);

// --- ImGui UI ---
namespace ProfileUI {
    void DrawProfileManagerUI();
}