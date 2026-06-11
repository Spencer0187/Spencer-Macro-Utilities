#include "../file_dialog.h"

#if defined(__APPLE__)

#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

#include <filesystem>
#include <string>

namespace smu::platform {
namespace {

NSString* ToNSString(const std::string& value)
{
    return [NSString stringWithUTF8String:value.c_str()];
}

NSString* ToFileExtension(const std::string& extension)
{
    std::string normalized = extension;
    if (!normalized.empty() && normalized.front() == '.') {
        normalized.erase(normalized.begin());
    }
    return normalized.empty() ? nil : ToNSString(normalized);
}

NSMutableArray<NSString*>* BuildAllowedFileTypes(const FileDialogOptions& options)
{
    NSMutableArray<NSString*>* fileTypes = [NSMutableArray arrayWithCapacity:options.extensions.size()];
    for (const std::string& extension : options.extensions) {
        if (NSString* fileType = ToFileExtension(extension)) {
            [fileTypes addObject:fileType];
        }
    }
    return fileTypes.count > 0 ? fileTypes : nil;
}

NSURL* InitialDirectoryUrl(const FileDialogOptions& options)
{
    if (options.initialDirectory.empty()) {
        return nil;
    }

    std::error_code ec;
    if (!std::filesystem::is_directory(options.initialDirectory, ec) || ec) {
        return nil;
    }

    NSString* path = ToNSString(options.initialDirectory.string());
    return path ? [NSURL fileURLWithPath:path isDirectory:YES] : nil;
}

} // namespace

FileDialogResult OpenNativeFileDialog(const FileDialogOptions& options)
{
    @autoreleasepool {
        if (![NSThread isMainThread]) {
            return {FileDialogResultType::Error, {}, "macOS file dialog must be opened from the main thread."};
        }

        NSOpenPanel* panel = [NSOpenPanel openPanel];
        panel.canChooseFiles = YES;
        panel.canChooseDirectories = NO;
        panel.allowsMultipleSelection = NO;
        panel.resolvesAliases = YES;
        panel.title = ToNSString(options.title.empty() ? "Import SMU Script" : options.title);

        if (NSURL* initialDirectory = InitialDirectoryUrl(options)) {
            panel.directoryURL = initialDirectory;
        }

        if (NSMutableArray<NSString*>* fileTypes = BuildAllowedFileTypes(options)) {
            panel.allowedFileTypes = fileTypes;
            panel.allowsOtherFileTypes = YES;
        }

        const NSInteger response = [panel runModal];
        if (response == NSModalResponseOK) {
            NSURL* url = panel.URL;
            NSString* path = url ? url.path : nil;
            const char* utf8Path = path ? path.UTF8String : nullptr;
            if (!utf8Path || utf8Path[0] == '\0') {
                return {FileDialogResultType::Error, {}, "macOS file dialog did not return a selected path."};
            }
            return {FileDialogResultType::Selected, std::filesystem::path(utf8Path), {}};
        }

        return {FileDialogResultType::Cancelled, {}, {}};
    }
}

} // namespace smu::platform

#endif
