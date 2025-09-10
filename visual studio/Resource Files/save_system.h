#include <wininet.h>
#include <fstream>
#include <codecvt>
#include "miniz.h"

// Library for HTTP (To get version data from my github page)
#pragma comment(lib, "wininet.lib")

static std::string Trim(const std::string &str)
{ // Trim a string
    auto start = str.begin();
    while (start != str.end() && std::isspace(*start)) {
        ++start;
    }

    auto end = str.end();
    do {
        --end;
    } while (end != start && std::isspace(*end));

    return std::string(start, end + 1);
}

static size_t OutputReleaseVersion(void *contents, size_t size, size_t nmemb, std::string *output)
{
    size_t totalSize = size * nmemb;
    output->append((char*)contents, totalSize);
    return totalSize;
}


// Generic function to get the content of a URL as a string.
static std::string GetStringFromUrl(const wchar_t* url)
{
    DWORD timeout = 500;
    
    HINTERNET hInternet = InternetOpen(L"Spencer-Macro-Utilities-Updater", INTERNET_OPEN_TYPE_DIRECT, NULL, NULL, 0);
    if (!hInternet) return "";

    InternetSetOption(hInternet, INTERNET_OPTION_CONNECT_TIMEOUT, &timeout, sizeof(timeout));
    InternetSetOption(hInternet, INTERNET_OPTION_RECEIVE_TIMEOUT, &timeout, sizeof(timeout));

    HINTERNET hConnect = InternetOpenUrl(hInternet, url, NULL, 0, INTERNET_FLAG_RELOAD | INTERNET_FLAG_NO_CACHE_WRITE, 0);
    if (!hConnect) {
        InternetCloseHandle(hInternet);
        return "";
    }

    char buffer[4096];
    DWORD bytesRead;
    std::string response;

    while (InternetReadFile(hConnect, buffer, sizeof(buffer), &bytesRead) && bytesRead > 0) {
        response.append(buffer, bytesRead);
    }

    InternetCloseHandle(hConnect);
    InternetCloseHandle(hInternet);

    return response;
}

static std::string GetRemoteVersion()
{
    return GetStringFromUrl(
	    L"https://raw.githubusercontent.com/Spencer0187/Spencer-Macro-Utilities/main/version");
}

static std::string GetRemoteUpdateUrlTemplate()
{
    return GetStringFromUrl(
	    L"https://raw.githubusercontent.com/Spencer0187/Spencer-Macro-Utilities/main/.github/autoupdaterurl");
}

// Generate file name for update
static std::wstring GenerateRandomHexString()
{
    std::random_device rd;
    std::mt19937 gen(rd());
    std::uniform_int_distribution<> distrib(0, 15);

    std::wstringstream wss;
    for (int i = 0; i < 16; ++i) {
	wss << std::hex << distrib(gen);
    }
    return wss.str();
}

bool DownloadToMemory(const std::wstring& url, std::vector<char>& data) {
    HINTERNET hInternet = InternetOpen(L"Spencer-Macro-Utilities-Updater", INTERNET_OPEN_TYPE_DIRECT, NULL, NULL, 0);
    if (!hInternet) {
        return false;
    }

    HINTERNET hConnect = InternetOpenUrl(hInternet, url.c_str(), NULL, 0, INTERNET_FLAG_RELOAD | INTERNET_FLAG_NO_CACHE_WRITE, 0);
    if (!hConnect) {
        InternetCloseHandle(hInternet);
        return false;
    }

    char buffer[4096];
    DWORD bytesRead;
    
    while (InternetReadFile(hConnect, buffer, sizeof(buffer), &bytesRead) && bytesRead > 0) {
        data.insert(data.end(), buffer, buffer + bytesRead);
    }

    InternetCloseHandle(hConnect);
    InternetCloseHandle(hInternet);
    
    return !data.empty();
}

bool ExtractFileFromMemory(const std::vector<char>& zipBuffer, const std::string& fileNameToExtract, std::vector<char>& extractedData) {
    mz_zip_archive zip_archive;
    mz_zip_zero_struct(&zip_archive);

    if (!mz_zip_reader_init_mem(&zip_archive, zipBuffer.data(), zipBuffer.size(), 0)) {
        return false;
    }

    int file_index = mz_zip_reader_locate_file(&zip_archive, fileNameToExtract.c_str(), NULL, 0);
    if (file_index < 0) {
        mz_zip_reader_end(&zip_archive);
        return false; // File not found in archive
    }

    size_t uncompressed_size = 0;
    void* pBuffer = mz_zip_reader_extract_to_heap(&zip_archive, file_index, &uncompressed_size, 0);
    if (!pBuffer) {
        mz_zip_reader_end(&zip_archive);
        return false; // Failed to extract
    }
    
    extractedData.assign(static_cast<char*>(pBuffer), static_cast<char*>(pBuffer) + uncompressed_size);
    
    mz_free(pBuffer);
    mz_zip_reader_end(&zip_archive);

    return true;
}

void PerformUpdate(const std::string& newVersion, const std::string& localVersion) {
    // 1. Get remote update URL template and construct the final download URL
    std::string urlTemplateAnsi = GetRemoteUpdateUrlTemplate();
    if (urlTemplateAnsi.empty()) {
        MessageBox(NULL, L"Failed to retrieve update URL configuration.", L"Update Error", MB_OK | MB_ICONERROR);
        return;
    }

    // --- FIX 1: Replace ALL instances of the placeholder ---
    std::string placeholder = "{VERSION}";
    size_t start_pos = 0;
    while((start_pos = urlTemplateAnsi.find(placeholder, start_pos)) != std::string::npos) {
        urlTemplateAnsi.replace(start_pos, placeholder.length(), newVersion);
        start_pos += newVersion.length();
    }
    
    std::wstring_convert<std::codecvt_utf8_utf16<wchar_t>> converter;
    std::wstring downloadUrl = converter.from_bytes(urlTemplateAnsi);

    // 2. Download the zip file into a memory buffer
    std::vector<char> zipData;
    if (!DownloadToMemory(downloadUrl, zipData)) {
        MessageBox(NULL, L"Failed to download the update. Please check your internet connection.", L"Update Error", MB_OK | MB_ICONERROR);
        return;
    }

    // 3. Extract the target executable from the memory buffer into another memory buffer
    std::vector<char> exeData;
    const std::string exeNameInZip = "suspend.exe"; // The name of the EXE inside the .zip file
    if (!ExtractFileFromMemory(zipData, exeNameInZip, exeData)) {
        MessageBox(NULL, L"Failed to extract the update from the downloaded package. It may be corrupt.", L"Update Error", MB_OK | MB_ICONERROR);
        return;
    }

    // 4. Define paths and generate a random temporary filename
    wchar_t currentExePathArr[MAX_PATH];
    GetModuleFileNameW(NULL, currentExePathArr, MAX_PATH);
    std::wstring currentExePath = currentExePathArr;
    std::wstring currentExeName = PathFindFileNameW(currentExePathArr);

    wchar_t workingDirArr[MAX_PATH];
    wcscpy_s(workingDirArr, currentExePathArr);
    PathRemoveFileSpecW(workingDirArr);
    std::wstring workingDir = workingDirArr;

    std::wstring randomFileName = GenerateRandomHexString();
    std::wstring tempExePath = workingDir + L"\\" + randomFileName + L".tmp";

    // 5. Write the in-memory EXE data to the temporary file on disk
    HANDLE hFile = CreateFileW(tempExePath.c_str(), GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (hFile == INVALID_HANDLE_VALUE) {
        MessageBox(NULL, L"Could not create temporary update file. Please check folder permissions.", L"Update Error", MB_OK | MB_ICONERROR);
        return;
    }
    DWORD bytesWritten;
    if (!WriteFile(hFile, exeData.data(), exeData.size(), &bytesWritten, NULL) || bytesWritten != exeData.size()) {
        CloseHandle(hFile);
        DeleteFileW(tempExePath.c_str());
        MessageBox(NULL, L"Failed to write update data to temporary file.", L"Update Error", MB_OK | MB_ICONERROR);
        return;
    }
    CloseHandle(hFile);

    // 6. Generate the appropriate batch script (conditionally)
    std::wstring batchScriptContent;
    std::wstring wLocalVersion = converter.from_bytes(localVersion);
    std::wstring wNewVersion = converter.from_bytes(newVersion);

    // --- START: Re-integrated folder rename logic ---
    bool shouldRenameFolder = false;
    std::wstring currentFolderPath = workingDir;
    wchar_t folderNameBuffer[MAX_PATH];
    wcscpy_s(folderNameBuffer, currentFolderPath.c_str());
    PathStripPathW(folderNameBuffer);
    std::wstring currentFolderName = folderNameBuffer;
    
    // Check if the folder name ends with the local version string
    if (!currentFolderName.empty() && currentFolderName.length() > wLocalVersion.length() && 
        currentFolderName.substr(currentFolderName.length() - wLocalVersion.length()) == wLocalVersion) {
        // Check for a common separator before the version string to be safer
        wchar_t separator = currentFolderName[currentFolderName.length() - wLocalVersion.length() - 1];
        if (separator == L'-' || separator == L'_' || separator == L' ' || separator == 'V' || separator == 'v') {
            shouldRenameFolder = true;
        }
    }
    // --- END: Re-integrated folder rename logic ---

    wchar_t tempDir[MAX_PATH];
    GetTempPathW(MAX_PATH, tempDir);
    std::wstring batchFilePath = std::wstring(tempDir) + L"updater-" + GenerateRandomHexString() + L".bat";

    if (shouldRenameFolder) {
        // --- BATCH SCRIPT GENERATION: WITH FOLDER RENAME ---
        std::wstring newFolderName = currentFolderName.substr(0, currentFolderName.length() - wLocalVersion.length()) + wNewVersion;
        
        wchar_t parentOfCurrentArr[MAX_PATH];
        wcscpy_s(parentOfCurrentArr, currentFolderPath.c_str());
        PathRemoveFileSpecW(parentOfCurrentArr);
        
        std::wstring newFolderPath = std::wstring(parentOfCurrentArr) + L"\\" + newFolderName;
        std::wstring newExePathInRenamedFolder = newFolderPath + L"\\" + currentExeName;

		std::wstring tempExePathAfterRename = newFolderPath + L"\\" + randomFileName + L".tmp";
		std::wstring finalExePathInNewFolder = newFolderPath + L"\\" + currentExeName;
		// --- BATCH SCRIPT GENERATION: RENAME ---
		batchScriptContent =
			L"@echo off\n"
			L"pushd \"%~dp0\"\n\n"
			L"echo Updating and renaming folder...\n"
			L"timeout /t 2 /nobreak > NUL\n\n"
			// Use 'move' with the full destination path to rename the folder.
			L"move \"" + currentFolderPath + L"\" \"" + newFolderPath + L"\"\n\n"
			// 2. Delete the old executable (which is now at its new path).
			L"del /F /Q \"" + finalExePathInNewFolder + L"\"\n"
			// Rename the .tmp file to the final executable name.
			L"move \"" + tempExePathAfterRename + L"\" \"" + finalExePathInNewFolder + L"\"\n\n"
			// 4. Relaunch the application from its final location.
			L"start \"\" \"" + finalExePathInNewFolder + L"\"\n"
			// 5. Self-delete the batch script.
			L"(goto) 2>nul & del \"%~f0\"";
    } else {
        // --- BATCH SCRIPT GENERATION: STANDARD (NO FOLDER RENAME) ---
        batchScriptContent =
            L"@echo off\n"
			L"pushd \"%~dp0\"\n\n"
            L"echo Updating in progress...\n"
            L"timeout /t 2 /nobreak > NUL\n"
            // Delete the original executable: %1 is current exe path
            L"del /F /Q \"%~1\"\n"
            // Rename the new temporary file (%2) to the original executable's name (%3)
			L"move \"%~2\" \"%~1\"\n\n"
            // Relaunch the newly updated application
            L"start \"\" \"%~1\"\n"
            // Self-delete the batch script
            L"(goto) 2>nul & del \"%~f0\"";
    }

    // 7. Write and execute the batch script
    HANDLE hBatchFile = CreateFileW(batchFilePath.c_str(), GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (hBatchFile != INVALID_HANDLE_VALUE) {
        std::string mbBatchScript = converter.to_bytes(batchScriptContent);
        DWORD batchBytesWritten;
        WriteFile(hBatchFile, mbBatchScript.c_str(), mbBatchScript.length(), &batchBytesWritten, NULL);
        CloseHandle(hBatchFile);

        // Prepare parameters for ShellExecute. Each parameter must be quoted.
        // For the standard case, we pass parameters. For the rename case, we don't, as the paths are hardcoded.
        std::wstring params = L"";
        if (!shouldRenameFolder) {
            params = L"\"" + currentExePath + L"\" \"" + tempExePath + L"\" \"" + currentExeName + L"\"";
        }
        
        ShellExecuteW(NULL, L"open", batchFilePath.c_str(), params.c_str(), NULL, SW_HIDE);
        exit(0); // Exit the current application immediately
    } else {
        MessageBox(NULL, L"Could not create the updater script. Please check permissions.", L"Update Error", MB_OK | MB_ICONERROR);
        DeleteFileW(tempExePath.c_str());
    }
}