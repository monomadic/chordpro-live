// CoreText string rasterizer — the whole text stack's contact with the OS.
//
// One call in, one A8 bitmap out. Rasterizing WHOLE STRINGS (rather than
// packing glyphs and laying them out in Rust) is what buys us, for free and
// forever: real system-font shaping, font fallback for anything the system
// font lacks, tabular figures via a font feature rather than hand-kerning,
// and end-truncation with a proper ellipsis. None of that is code we want to
// own, and all of it is one CTLine away.
//
// The bitmap is alpha-only: CGBitmapContextCreate with kCGImageAlphaOnly
// gives exactly one coverage byte per pixel, which is exactly what an R8
// glyph atlas wants — no channel swizzling, no premultiplied-colour trap.
// Colour is applied by the shader, so one raster serves every tint.
//
// Subpixel (LCD) antialiasing is deliberately OFF: it bakes RGB fringes into
// the coverage, which is wrong the moment the text is tinted or composited
// over video. Grayscale AA is the only correct choice for a texture.
#import <AppKit/AppKit.h>
#import <CoreText/CoreText.h>
#import <math.h>
#import <stdlib.h>

static NSFontWeight sb_weight(int w) {
    if (w <= 250) return NSFontWeightUltraLight;
    if (w <= 350) return NSFontWeightLight;
    if (w <= 450) return NSFontWeightRegular;
    if (w <= 550) return NSFontWeightMedium;
    if (w <= 650) return NSFontWeightSemibold;
    if (w <= 750) return NSFontWeightBold;
    return NSFontWeightHeavy;
}

static NSFont *sb_font(double size, int weight, int mono) {
    return mono ? [NSFont monospacedSystemFontOfSize:size weight:sb_weight(weight)]
                : [NSFont systemFontOfSize:size weight:sb_weight(weight)];
}

// Typographic advance of `utf8` at `size` points, without drawing anything.
// Layout measures with this; only the renderer pays for a bitmap. Trailing
// spaces count, because the next run starts after them.
double sb_text_width(const char *utf8, double size, int weight, int mono) {
    @autoreleasepool {
        if (!utf8 || size <= 0.0) return 0.0;
        NSString *s = [NSString stringWithUTF8String:utf8];
        if (!s || s.length == 0) return 0.0;
        NSFont *font = sb_font(size, weight, mono);
        if (!font) return 0.0;
        NSAttributedString *as =
            [[NSAttributedString alloc] initWithString:s
                                            attributes:@{NSFontAttributeName : font}];
        CTLineRef line = CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)as);
        if (!line) return 0.0;
        double width = CTLineGetTypographicBounds(line, NULL, NULL, NULL);
        CFRelease(line);
        return width;
    }
}

// Rasterize `utf8` at `size` points and weight `weight` (CSS-ish: 400 regular,
// 600 semibold) into a bitmap of `scale` pixels per point.
//
// The font is always created at its POINT size and the context is scaled,
// never the font enlarged: SF Pro's tracking and optical size depend on the
// point size, so a 17 pt line requested as a 34 pt font is about 5% narrower
// than the layout measured, and gaps open between separately drawn runs.
//
// `max_width` > 0 truncates with a trailing ellipsis. `tnum` selects tabular
// (fixed-advance) figures so columns of digits don't shimmy as they change.
// `mono` picks the system monospaced face instead of the UI face.
//
// Returns a malloc'd A8 buffer of `*out_w * *out_h` bytes (rows are tightly
// packed, one byte per pixel) and writes the baseline's distance from the top
// edge into `*out_baseline`. Returns NULL if nothing was drawn; free a
// non-NULL result with sb_text_free.
//
// `out_adv`/`out_adv_n` (both optional) additionally return a malloc'd array
// of one x offset per UTF-16 code-unit boundary — `*out_adv_n` is the string's
// UTF-16 length plus one, so the last entry is the end of the line. These are
// what let a caller point INSIDE a rasterized string without owning any
// shaping: the search bar's caret sits at one of these offsets, and its match
// highlight redraws a clipped slice of the very same bitmap between two of
// them (so there is no second raster to misregister against the first).
// Free with sb_text_free_f.
unsigned char *sb_text_rasterize(const char *utf8, double size, double scale, int weight,
                                 double max_width, int tnum, int mono,
                                 int *out_w, int *out_h, int *out_baseline,
                                 float **out_adv, int *out_adv_n) {
    @autoreleasepool {
        if (!utf8 || !out_w || !out_h || size <= 0.0) return NULL;
        if (!(scale > 0.0)) scale = 1.0;
        if (out_adv) *out_adv = NULL;
        if (out_adv_n) *out_adv_n = 0;
        NSString *s = [NSString stringWithUTF8String:utf8];
        if (!s || s.length == 0) return NULL;

        NSFont *font = sb_font(size, weight, mono);
        if (!font) return NULL;
        if (tnum) {
            // Tabular figures as a font feature. A face without the feature
            // ignores the descriptor and we simply keep proportional digits.
            NSFontDescriptor *desc = [font.fontDescriptor fontDescriptorByAddingAttributes:@{
                NSFontFeatureSettingsAttribute : @[ @{
                    NSFontFeatureTypeIdentifierKey : @(kNumberSpacingType),
                    NSFontFeatureSelectorIdentifierKey : @(kMonospacedNumbersSelector),
                } ]
            }];
            NSFont *tab = [NSFont fontWithDescriptor:desc size:size];
            if (tab) font = tab;
        }

        NSDictionary *attrs = @{
            NSFontAttributeName : font,
            NSForegroundColorAttributeName : [NSColor whiteColor],
        };
        NSAttributedString *as = [[NSAttributedString alloc] initWithString:s
                                                                 attributes:attrs];
        CTLineRef line = CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)as);
        if (!line) return NULL;

        if (max_width > 0.0) {
            NSAttributedString *ellipsis =
                [[NSAttributedString alloc] initWithString:@"…" attributes:attrs];
            CTLineRef token =
                CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)ellipsis);
            CTLineRef truncated =
                CTLineCreateTruncatedLine(line, max_width, kCTLineTruncationEnd, token);
            if (token) CFRelease(token);
            if (!truncated) {
                // The limit can't hold even the ellipsis. Keeping the
                // untruncated line would raster at FULL width — spilling
                // the box and, for long strings, exceeding the atlas.
                // A box that small shows nothing legible anyway.
                CFRelease(line);
                return NULL;
            }
            CFRelease(line);
            line = truncated;
        }

        CGFloat ascent = 0, descent = 0, leading = 0;
        double width = CTLineGetTypographicBounds(line, &ascent, &descent, &leading);
        // One pixel of slack on each side: glyphs routinely paint a hair
        // outside the typographic box (overshoot, italic sidebearing, the
        // ellipsis) and a clipped edge is far more visible than a dead column.
        int pad = 1;
        int w = (int)ceil(width * scale) + pad * 2;
        int h = (int)ceil((ascent + descent) * scale) + pad * 2;
        if (w <= 0 || h <= 0 || w > 8192 || h > 8192) {
            CFRelease(line);
            return NULL;
        }

        size_t bytes = (size_t)w * (size_t)h;
        unsigned char *buf = calloc(bytes, 1);
        if (!buf) {
            CFRelease(line);
            return NULL;
        }
        CGContextRef ctx = CGBitmapContextCreate(buf, (size_t)w, (size_t)h, 8, (size_t)w,
                                                 NULL, (CGBitmapInfo)kCGImageAlphaOnly);
        if (!ctx) {
            free(buf);
            CFRelease(line);
            return NULL;
        }
        CGContextSetShouldAntialias(ctx, true);
        CGContextSetShouldSmoothFonts(ctx, false);  // grayscale AA, never subpixel
        CGContextTranslateCTM(ctx, (CGFloat)pad, (CGFloat)pad);
        CGContextScaleCTM(ctx, (CGFloat)scale, (CGFloat)scale);
        CGContextSetTextPosition(ctx, 0, descent);
        CTLineDraw(line, ctx);
        CGContextRelease(ctx);
        // Kept alive past the draw: the offsets below must be measured on
        // the line that was actually drawn, truncation included.
        CTLineRef line2 = line;

        *out_w = w;
        *out_h = h;
        if (out_baseline) *out_baseline = (int)lround(ascent * scale) + pad;
        // One offset per UTF-16 boundary, measured on the SAME line that was
        // just drawn — including its truncation, so an index past the
        // ellipsis simply lands on the clamped end rather than somewhere the
        // bitmap doesn't have. `pad` is added because the bitmap carries a
        // pixel of slack on the left.
        if (out_adv && out_adv_n) {
            int n = (int)s.length + 1;
            float *adv = malloc(sizeof(float) * (size_t)n);
            if (adv) {
                for (int i = 0; i < n; i++) {
                    CGFloat off = CTLineGetOffsetForStringIndex(line2, (CFIndex)i, NULL);
                    if (off < 0) off = 0;
                    if (off > width) off = width;
                    adv[i] = (float)(off * scale + pad);
                }
                *out_adv = adv;
                *out_adv_n = n;
            }
        }
        CFRelease(line2);
        return buf;
    }
}

void sb_text_free(unsigned char *p) {
    free(p);
}

void sb_text_free_f(float *p) {
    free(p);
}
