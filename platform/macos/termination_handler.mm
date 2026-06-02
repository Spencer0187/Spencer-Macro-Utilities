#include "termination_handler.h"

#if defined(__APPLE__)

#import <AppKit/AppKit.h>
#include <SDL3/SDL.h>
#include <objc/runtime.h>

#include "../../core/app_state.h"
#include "../../core/legacy_globals.h"
#include "../logging.h"

@class SMUTerminationDelegate;

namespace smu::platform::macos {
namespace {

using TerminateImp = void (*)(id, SEL, id);

SMUTerminationDelegate* g_terminationDelegate = nil;
TerminateImp g_originalTerminate = nullptr;

void RequestMacOSApplicationQuit()
{
    auto& appState = smu::core::GetAppState();
    appState.done.store(true, std::memory_order_release);
    appState.running.store(false, std::memory_order_release);
    Globals::done.store(true, std::memory_order_release);
    Globals::running.store(false, std::memory_order_release);

    if (SDL_EventEnabled(SDL_EVENT_QUIT)) {
        SDL_Event event;
        SDL_zero(event);
        event.type = SDL_EVENT_QUIT;
        SDL_PushEvent(&event);
    }
}

void SMUTerminate(id self, SEL command, id sender)
{
    RequestMacOSApplicationQuit();
    if (g_originalTerminate) {
        g_originalTerminate(self, command, sender);
    }
}

void InstallTerminateHook(NSApplication* application)
{
    Class applicationClass = [application class];
    Method terminateMethod = class_getInstanceMethod(applicationClass, @selector(terminate:));
    if (!terminateMethod || method_getImplementation(terminateMethod) == reinterpret_cast<IMP>(SMUTerminate)) {
        return;
    }

    g_originalTerminate = reinterpret_cast<TerminateImp>(method_getImplementation(terminateMethod));
    method_setImplementation(terminateMethod, reinterpret_cast<IMP>(SMUTerminate));
}

} // namespace
} // namespace smu::platform::macos

@interface SMUTerminationDelegate : NSObject <NSApplicationDelegate>
{
    id<NSApplicationDelegate> previousDelegate_;
}

- (instancetype)initWithPreviousDelegate:(id<NSApplicationDelegate>)previousDelegate;
- (void)handleQuitEvent:(NSAppleEventDescriptor*)event withReplyEvent:(NSAppleEventDescriptor*)replyEvent;

@end

@implementation SMUTerminationDelegate

- (instancetype)initWithPreviousDelegate:(id<NSApplicationDelegate>)previousDelegate
{
    self = [super init];
    if (self) {
        previousDelegate_ = [previousDelegate retain];
    }
    return self;
}

- (void)dealloc
{
    [previousDelegate_ release];
    [super dealloc];
}

- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication*)sender
{
    (void)sender;
    LogInfo("macOS applicationShouldTerminate requested quit.");
    smu::platform::macos::RequestMacOSApplicationQuit();

    return NSTerminateCancel;
}

- (void)handleQuitEvent:(NSAppleEventDescriptor*)event withReplyEvent:(NSAppleEventDescriptor*)replyEvent
{
    (void)event;
    (void)replyEvent;
    LogInfo("macOS Apple Event requested quit.");
    smu::platform::macos::RequestMacOSApplicationQuit();
}

- (BOOL)application:(NSApplication*)application openFile:(NSString*)filename
{
    if ([previousDelegate_ respondsToSelector:@selector(application:openFile:)]) {
        return [previousDelegate_ application:application openFile:filename];
    }
    return NO;
}

- (BOOL)applicationSupportsSecureRestorableState:(NSApplication*)application
{
    if (@available(macOS 12.0, *)) {
        if ([previousDelegate_ respondsToSelector:@selector(applicationSupportsSecureRestorableState:)]) {
            return [previousDelegate_ applicationSupportsSecureRestorableState:application];
        }
    }
    return YES;
}

- (BOOL)respondsToSelector:(SEL)selector
{
    return [super respondsToSelector:selector] || [previousDelegate_ respondsToSelector:selector];
}

- (id)forwardingTargetForSelector:(SEL)selector
{
    if ([previousDelegate_ respondsToSelector:selector]) {
        return previousDelegate_;
    }
    return [super forwardingTargetForSelector:selector];
}

@end

namespace smu::platform::macos {

void InstallMacOSTerminationHandler()
{
    @autoreleasepool {
        NSApplication* application = [NSApplication sharedApplication];
        id<NSApplicationDelegate> currentDelegate = [application delegate];
        if (!application || currentDelegate == g_terminationDelegate) {
            return;
        }

        InstallTerminateHook(application);
        [g_terminationDelegate release];
        g_terminationDelegate = [[SMUTerminationDelegate alloc] initWithPreviousDelegate:currentDelegate];
        [[NSAppleEventManager sharedAppleEventManager]
            setEventHandler:g_terminationDelegate
                andSelector:@selector(handleQuitEvent:withReplyEvent:)
              forEventClass:kCoreEventClass
                 andEventID:kAEQuitApplication];
        [application setDelegate:g_terminationDelegate];
        LogInfo("Installed macOS AppKit quit handlers.");
    }
}

} // namespace smu::platform::macos

#endif
