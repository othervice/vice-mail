// Test tooling for the performance measurement (perf.e2e.mjs), never part of the app: a library
// that the test loads into the desktop-test app it starts (DYLD_INSERT_LIBRARIES; the test build
// has no hardened runtime). Built by perf.e2e.mjs with the system's clang.
//
// A Mac mini that nobody sits at shows the lock screen over every window. WebKit then takes the
// page for hidden: no animation frames, throttled timers, so the frame rate of scrolling cannot be
// measured. This turns off window-occlusion detection for every WKWebView as it is created
// (WebKit's `_setWindowOcclusionDetectionEnabled:`, which exists for such cases): the page counts
// as visible while its window is on screen, covered or not, and renders as it would in front of
// the user. The display must be awake for frames (perf.e2e.mjs runs `caffeinate -d -u`).
#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>
#import <objc/message.h>
#import <objc/runtime.h>

typedef id (*InitWithFrame)(id, SEL, NSRect, WKWebViewConfiguration *);
static InitWithFrame original;

static id initKeepingFrames(id self, SEL cmd, NSRect frame, WKWebViewConfiguration *configuration) {
  id view = original(self, cmd, frame, configuration);
  SEL setter = NSSelectorFromString(@"_setWindowOcclusionDetectionEnabled:");
  if (view != nil && [view respondsToSelector:setter]) {
    ((void (*)(id, SEL, BOOL))objc_msgSend)(view, setter, NO);
    fprintf(stderr, "keep-rendering: window occlusion detection off for %s\n", class_getName([view class]));
  } else {
    fprintf(stderr, "keep-rendering: no _setWindowOcclusionDetectionEnabled: on %s\n", class_getName([view class]));
  }
  return view;
}

__attribute__((constructor)) static void install(void) {
  Method method = class_getInstanceMethod([WKWebView class], @selector(initWithFrame:configuration:));
  if (method == NULL) {
    fprintf(stderr, "keep-rendering: WKWebView has no initWithFrame:configuration:\n");
    return;
  }
  original = (InitWithFrame)method_setImplementation(method, (IMP)initKeepingFrames);
}
