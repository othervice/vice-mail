// A virtual HiDPI display for the README screenshots (readme-shots.e2e.mjs), on a Mac without a
// Retina screen (a headless Mac mini has a 1x fallback display). Test tooling only: never part of
// the app. Built by support/hidpi-display.mjs with the system's clang.
//
//   hidpi-display WIDTH HEIGHT   create a display of WIDTH x HEIGHT points at 2x, move the pointer
//                                to its far corner, print its bounds as one JSON line, keep it
//                                until stdin closes, then remove it
//   hidpi-display --list         print the online displays as JSON lines
//
// Uses CoreGraphics' CGVirtualDisplay, a private API (the one display tools such as DeskPad use).
// macOS removes a virtual display only while the physical display is awake: after this process
// ends with the display asleep, the display stays until something wakes it (`caffeinate -u`).
#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>

@interface CGVirtualDisplayDescriptor : NSObject
@property(retain, nonatomic) dispatch_queue_t queue;
@property(retain, nonatomic) NSString *name;
@property(nonatomic) unsigned int maxPixelsHigh;
@property(nonatomic) unsigned int maxPixelsWide;
@property(nonatomic) CGSize sizeInMillimeters;
@property(nonatomic) unsigned int serialNum;
@property(nonatomic) unsigned int productID;
@property(nonatomic) unsigned int vendorID;
@end

@interface CGVirtualDisplayMode : NSObject
- (instancetype)initWithWidth:(NSUInteger)width height:(NSUInteger)height refreshRate:(CGFloat)rate;
@end

@interface CGVirtualDisplaySettings : NSObject
@property(retain, nonatomic) NSArray *modes;
@property(nonatomic) unsigned int hiDPI;
@end

@interface CGVirtualDisplay : NSObject
@property(readonly, nonatomic) CGDirectDisplayID displayID;
- (instancetype)initWithDescriptor:(CGVirtualDisplayDescriptor *)descriptor;
- (BOOL)applySettings:(CGVirtualDisplaySettings *)settings;
@end

// Marks the display as ours, so a leftover one can be recognised (--list).
static const unsigned int VENDOR = 0x5643, PRODUCT = 0x5348, SERIAL = 0x2026;

static void spin(double seconds) {
  NSDate *until = [NSDate dateWithTimeIntervalSinceNow:seconds];
  while ([until timeIntervalSinceNow] > 0) CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, false);
}

static int list(void) {
  CGDirectDisplayID ids[32];
  uint32_t count = 0;
  if (CGGetOnlineDisplayList(32, ids, &count) != kCGErrorSuccess) return 1;
  for (uint32_t i = 0; i < count; i++) {
    CGRect b = CGDisplayBounds(ids[i]);
    CGDisplayModeRef mode = CGDisplayCopyDisplayMode(ids[i]);
    size_t pixels = mode ? CGDisplayModeGetPixelWidth(mode) : 0;
    if (mode) CGDisplayModeRelease(mode);
    printf("{\"id\":%u,\"ours\":%s,\"main\":%s,\"x\":%.0f,\"y\":%.0f,\"width\":%.0f,\"height\":%.0f,\"scale\":%.0f}\n", ids[i],
           CGDisplayVendorNumber(ids[i]) == VENDOR && CGDisplayModelNumber(ids[i]) == PRODUCT ? "true" : "false",
           CGDisplayIsMain(ids[i]) ? "true" : "false", b.origin.x, b.origin.y, b.size.width, b.size.height,
           b.size.width > 0 ? (double)pixels / b.size.width : 0);
  }
  return 0;
}

int main(int argc, const char **argv) {
  @autoreleasepool {
    if (argc == 2 && strcmp(argv[1], "--list") == 0) return list();
    if (argc != 3) {
      fprintf(stderr, "usage: hidpi-display WIDTH HEIGHT | --list\n");
      return 64;
    }
    NSUInteger width = (NSUInteger)atoi(argv[1]), height = (NSUInteger)atoi(argv[2]);
    if (width < 640 || height < 480 || width > 4096 || height > 4096) {
      fprintf(stderr, "hidpi-display: size out of range\n");
      return 64;
    }
    CGVirtualDisplayDescriptor *descriptor = [CGVirtualDisplayDescriptor new];
    descriptor.queue = dispatch_get_main_queue();
    descriptor.name = @"Screenshot display";
    descriptor.maxPixelsWide = (unsigned int)(width * 2);
    descriptor.maxPixelsHigh = (unsigned int)(height * 2);
    // About 220 dpi, like a Retina panel.
    descriptor.sizeInMillimeters = CGSizeMake(width * 2 * 25.4 / 220, height * 2 * 25.4 / 220);
    descriptor.vendorID = VENDOR;
    descriptor.productID = PRODUCT;
    descriptor.serialNum = SERIAL;
    CGVirtualDisplay *display = [[CGVirtualDisplay alloc] initWithDescriptor:descriptor];
    if (!display || display.displayID == 0) {
      fprintf(stderr, "hidpi-display: no virtual display (one from an earlier run may still exist)\n");
      return 2;
    }
    CGVirtualDisplaySettings *settings = [CGVirtualDisplaySettings new];
    settings.hiDPI = 1;
    settings.modes = @[ [[CGVirtualDisplayMode alloc] initWithWidth:width height:height refreshRate:60] ];
    if (![display applySettings:settings]) {
      fprintf(stderr, "hidpi-display: the display refused its settings\n");
      return 3;
    }
    CGDirectDisplayID id = display.displayID;
    for (int i = 0; i < 100 && !(CGDisplayIsOnline(id) && CGDisplayPixelsWide(id) > 0); i++) spin(0.05);

    // The HiDPI variant of the mode: `width` points backed by twice as many pixels.
    NSDictionary *options = @{(__bridge NSString *)kCGDisplayShowDuplicateLowResolutionModes : @YES};
    CFArrayRef modes = CGDisplayCopyAllDisplayModes(id, (__bridge CFDictionaryRef)options);
    CGDisplayModeRef chosen = NULL;
    for (CFIndex i = 0; modes && i < CFArrayGetCount(modes); i++) {
      CGDisplayModeRef mode = (CGDisplayModeRef)CFArrayGetValueAtIndex(modes, i);
      if (CGDisplayModeGetWidth(mode) == width && CGDisplayModeGetPixelWidth(mode) == width * 2) chosen = mode;
    }
    if (!chosen || CGDisplaySetDisplayMode(id, chosen, NULL) != kCGErrorSuccess) {
      fprintf(stderr, "hidpi-display: no %lux%lu HiDPI mode\n", (unsigned long)width, (unsigned long)height);
      if (modes) CFRelease(modes);
      return 4;
    }
    if (modes) CFRelease(modes);
    spin(1.0);

    CGRect b = CGDisplayBounds(id);
    // The pointer to the far corner, away from the window: WebKit would show its hover states.
    CGWarpMouseCursorPosition(CGPointMake(b.origin.x + b.size.width - 2, b.origin.y + b.size.height - 2));
    printf("{\"id\":%u,\"x\":%.0f,\"y\":%.0f,\"width\":%.0f,\"height\":%.0f}\n", id, b.origin.x, b.origin.y, b.size.width,
           b.size.height);
    fflush(stdout);

    // Stay until the caller closes stdin (or ends), then release the display while running.
    __block BOOL done = NO;
    dispatch_source_t input = dispatch_source_create(DISPATCH_SOURCE_TYPE_READ, STDIN_FILENO, 0, dispatch_get_main_queue());
    dispatch_source_set_event_handler(input, ^{
      char buffer[256];
      if (read(STDIN_FILENO, buffer, sizeof buffer) <= 0) done = YES;
    });
    dispatch_resume(input);
    while (!done) CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, false);
    dispatch_source_cancel(input);
    display = nil;
    spin(1.0);
  }
  return 0;
}
