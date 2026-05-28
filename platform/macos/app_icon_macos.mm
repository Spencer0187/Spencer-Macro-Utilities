#include "app_icon_macos.h"

#if defined(__APPLE__)

#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

namespace smu::platform::macos {

bool ApplyApplicationIconFromBundle()
{
    @autoreleasepool {
        NSString* iconPath = [[NSBundle mainBundle] pathForResource:@"smu_icon" ofType:@"icns"];
        if (!iconPath) {
            return false;
        }

        NSImage* icon = [[NSImage alloc] initWithContentsOfFile:iconPath];
        if (!icon) {
            return false;
        }

        [[NSApplication sharedApplication] setApplicationIconImage:icon];
        [icon release];
        return true;
    }
}

} // namespace smu::platform::macos

#endif
