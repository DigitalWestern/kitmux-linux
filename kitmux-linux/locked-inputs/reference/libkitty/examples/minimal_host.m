// Minimal libkitty host: one AppKit window, one shell session, using only
// the public API in libkitty.h. This is the whole embedding story:
//   engine init -> render init -> session create -> pump/draw loop.
// A real host would drive pump from a DispatchSource on kitty_session_fd()
// instead of a timer; the timer keeps this example small.
#define GL_SILENCE_DEPRECATION
#import <Cocoa/Cocoa.h>
#import <OpenGL/gl3.h>
#include "libkitty.h"

static kitty_engine *g_engine;

@interface TermView : NSOpenGLView
@property (nonatomic) kitty_session *session;
@end

static void on_damage(void *u) { [(__bridge TermView *)u setNeedsDisplay:YES]; }
static void on_title(void *u, const char *t) {
    [(__bridge TermView *)u window].title = [NSString stringWithUTF8String:t];
}
static void on_child_exit(void *u, int status) {
    (void)u; (void)status;
    [NSApp terminate:nil];
}

@implementation TermView

- (instancetype)initWithFrame:(NSRect)frame {
    NSOpenGLPixelFormatAttribute attrs[] = {
        NSOpenGLPFAOpenGLProfile, NSOpenGLProfileVersion4_1Core,
        NSOpenGLPFAColorSize, 24, NSOpenGLPFAAlphaSize, 8,
        NSOpenGLPFADoubleBuffer, NSOpenGLPFAAccelerated, 0
    };
    NSOpenGLPixelFormat *pf = [[NSOpenGLPixelFormat alloc] initWithAttributes:attrs];
    self = [super initWithFrame:frame pixelFormat:pf];
    if (self) self.wantsBestResolutionOpenGLSurface = YES;
    return self;
}

- (BOOL)acceptsFirstResponder { return YES; }

- (NSSize)backingPixels { return [self convertRectToBacking:self.bounds].size; }

- (void)prepareOpenGL {
    [super prepareOpenGL];
    [[self openGLContext] makeCurrentContext];
    char err[512];
    CGFloat scale = [[self window] backingScaleFactor] ?: 1.0;
    if (!kitty_render_init(g_engine, scale, NULL, NULL, err, sizeof err)) {
        fprintf(stderr, "render init failed: %s\n", err);
        [NSApp terminate:nil];
        return;
    }
    kitty_session_callbacks cbs = {
        .userdata = (__bridge void *)self,
        .on_damage = on_damage, .on_title = on_title, .on_child_exit = on_child_exit,
    };
    self.session = kitty_session_create(g_engine, 24, 80, NULL, &cbs, err, sizeof err);
    if (!self.session) {
        fprintf(stderr, "session create failed: %s\n", err);
        [NSApp terminate:nil];
        return;
    }
    NSSize px = [self backingPixels];
    kitty_session_set_viewport(self.session, (int)px.width, (int)px.height);
}

- (void)reshape {
    [super reshape];
    if (!self.session) return;
    [[self openGLContext] makeCurrentContext];
    NSSize px = [self backingPixels];
    kitty_session_set_viewport(self.session, (int)px.width, (int)px.height);
    [self setNeedsDisplay:YES];
}

- (void)drawRect:(NSRect)dirty {
    [[self openGLContext] makeCurrentContext];
    NSSize px = [self backingPixels];
    glViewport(0, 0, (GLsizei)px.width, (GLsizei)px.height);
    glClearColor(0, 0, 0, 1);
    glClear(GL_COLOR_BUFFER_BIT);
    if (self.session) kitty_session_draw(self.session);
    [[self openGLContext] flushBuffer];
}

- (void)keyDown:(NSEvent *)event {
    if (!self.session || event.characters.length == 0) return;
    unichar c0 = [event.characters characterAtIndex:0];
    if (c0 >= NSUpArrowFunctionKey && c0 <= NSModeSwitchFunctionKey) return; // raw chars only in v0
    const char *utf8 = event.characters.UTF8String;
    if (utf8 && *utf8) kitty_session_write(self.session, (const uint8_t *)utf8, strlen(utf8));
}

- (void)tick {
    if (self.session) kitty_session_pump(self.session);  // on_damage schedules the redraw
}
@end

int main(void) {
    char err[512];
    kitty_engine_config cfg = {
        .kitty_src_path = getenv("KITTY_SRC"),
        .libkitty_py_path = getenv("LIBKITTY_PY"),
    };
    g_engine = kitty_engine_init(&cfg, err, sizeof err);
    if (!g_engine) { fprintf(stderr, "engine init failed: %s\n", err); return 1; }

    @autoreleasepool {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        NSRect frame = NSMakeRect(200, 200, 900, 560);
        NSWindow *win = [[NSWindow alloc]
            initWithContentRect:frame
                      styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable
                        backing:NSBackingStoreBuffered defer:NO];
        win.title = @"libkitty minimal host";
        TermView *view = [[TermView alloc] initWithFrame:frame];
        win.contentView = view;
        [win makeKeyAndOrderFront:nil];
        [win makeFirstResponder:view];
        [NSApp activateIgnoringOtherApps:YES];

        [NSTimer scheduledTimerWithTimeInterval:1.0/30.0 repeats:YES block:^(NSTimer *unused) { (void)unused; [view tick]; }];
        if (getenv("MINIMAL_HOST_AUTOQUIT"))
            [NSTimer scheduledTimerWithTimeInterval:3.0 repeats:NO block:^(NSTimer *unused) { (void)unused; [NSApp terminate:nil]; }];

        [NSApp run];
        // Not reached: [NSApp terminate:] exits the process directly. A real
        // app would call kitty_session_close/kitty_engine_shutdown from
        // applicationWillTerminate:. The child still dies cleanly here: the
        // PTY master closes at process exit, delivering SIGHUP.
    }
    return 0;
}
