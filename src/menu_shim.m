// The Theme menu: one item per accent colour, each with a swatch of its
// colour beside it and a checkmark on the current one. Choosing an item
// calls back into Rust with its index; the host does the rest.
#import <AppKit/AppKit.h>

typedef void (*cpl_theme_callback)(long index);

@interface CPLThemeTarget : NSObject
@property(nonatomic) cpl_theme_callback callback;
@property(nonatomic, strong) NSMenu *menu;
@end

@implementation CPLThemeTarget
- (void)choose:(NSMenuItem *)sender {
    for (NSMenuItem *item in self.menu.itemArray) {
        item.state = item == sender ? NSControlStateValueOn : NSControlStateValueOff;
    }
    if (self.callback) {
        self.callback(sender.tag);
    }
}
@end

static CPLThemeTarget *target;

// A rounded square of the colour with a hairline edge, so pale colours
// still read against the menu.
static NSImage *swatch(unsigned char r, unsigned char g, unsigned char b) {
    NSColor *colour = [NSColor colorWithSRGBRed:r / 255.0 green:g / 255.0 blue:b / 255.0 alpha:1];
    return [NSImage imageWithSize:NSMakeSize(14, 14)
                          flipped:NO
                   drawingHandler:^BOOL(NSRect rect) {
                       NSRect box = NSInsetRect(rect, 1, 1);
                       NSBezierPath *path = [NSBezierPath bezierPathWithRoundedRect:box
                                                                            xRadius:3
                                                                            yRadius:3];
                       [colour setFill];
                       [path fill];
                       [[NSColor colorWithWhite:0 alpha:0.2] setStroke];
                       path.lineWidth = 1;
                       [path stroke];
                       return YES;
                   }];
}

// Adds the Theme menu after the application menu. `names` holds `count`
// C strings and `rgb` three bytes per item. Safe to call once, on the
// main thread, after the app has finished launching.
void cpl_theme_menu_install(const char *const *names, const unsigned char *rgb, long count,
                            long selected, cpl_theme_callback callback) {
    if (target) {
        return;
    }
    // Copy what the deferred block needs: the caller's arrays are gone by then.
    NSMutableArray<NSString *> *titles = [NSMutableArray array];
    NSMutableArray<NSImage *> *images = [NSMutableArray array];
    for (long i = 0; i < count; i++) {
        [titles addObject:@(names[i])];
        [images addObject:swatch(rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2])];
    }
    target = [[CPLThemeTarget alloc] init];
    target.callback = callback;
    // Added on the next turn of the main queue, once winit's own menu bar
    // is certainly in place.
    dispatch_async(dispatch_get_main_queue(), ^{
        NSMenu *bar = NSApp.mainMenu;
        if (!bar) {
            bar = [[NSMenu alloc] init];
            NSApp.mainMenu = bar;
        }
        NSMenu *menu = [[NSMenu alloc] initWithTitle:@"Theme"];
        target.menu = menu;
        for (long i = 0; i < (long)titles.count; i++) {
            NSMenuItem *item = [[NSMenuItem alloc] initWithTitle:titles[i]
                                                          action:@selector(choose:)
                                                   keyEquivalent:@""];
            item.target = target;
            item.tag = i;
            // The swatch goes into the title as an inline image: macOS 26
            // menus do not draw `image` on these items.
            NSTextAttachment *attachment = [[NSTextAttachment alloc] init];
            attachment.image = images[i];
            NSFont *font = [NSFont menuFontOfSize:0];
            attachment.bounds = NSMakeRect(0, (font.capHeight - 14) / 2, 14, 14);
            NSMutableAttributedString *title = [[NSMutableAttributedString alloc]
                initWithAttributedString:[NSAttributedString attributedStringWithAttachment:attachment]];
            [title appendAttributedString:[[NSAttributedString alloc]
                                              initWithString:[@"  " stringByAppendingString:titles[i]]
                                                  attributes:@{NSFontAttributeName : font}]];
            item.attributedTitle = title;
            item.state = i == selected ? NSControlStateValueOn : NSControlStateValueOff;
            [menu addItem:item];
        }
        NSMenuItem *holder = [[NSMenuItem alloc] initWithTitle:@"Theme" action:nil keyEquivalent:@""];
        holder.submenu = menu;
        [bar insertItem:holder atIndex:MIN((NSInteger)1, bar.numberOfItems)];
    });
}
