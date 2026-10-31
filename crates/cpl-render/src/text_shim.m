// CoreText string rasterizer — the whole text stack's contact with the OS.
//
// One call in, one A8 bitmap out. Rasterizing WHOLE STRINGS (rather than
// packing glyphs and laying them out in Rust) is what buys us, for free and
// forever: real system-font shaping and font fallback for anything the
// system font lacks. None of that is code we want to own, and all of it is
// one CTLine away.
//
// Only CoreText, CoreGraphics and CoreFoundation are used, never AppKit:
// CTFont and CTLine are immutable and safe to use from any thread, so
// measuring on one thread while another rasterises needs no lock. AppKit's
// NSFont, which this shim used before, returned wrong advances under
// exactly that load (tests/coretext_threads.rs).
//
// The bitmap is alpha-only: CGBitmapContextCreate with kCGImageAlphaOnly
// gives exactly one coverage byte per pixel, which is exactly what an R8
// texture wants. Colour is applied by the compositor, so one raster serves
// both polarities.
//
// Subpixel (LCD) antialiasing is deliberately OFF: it bakes RGB fringes into
// the coverage, which is wrong the moment the text is tinted or composited.
#include <CoreFoundation/CoreFoundation.h>
#include <CoreGraphics/CoreGraphics.h>
#include <CoreText/CoreText.h>
#include <math.h>
#include <stdlib.h>

// CSS-ish weights (400 regular, 600 semibold) to CoreText's weight trait.
// These are the values of AppKit's NSFontWeight constants, so a weight
// resolves to the face `systemFontOfSize:weight:` would give.
static CGFloat sb_weight(int w) {
    if (w <= 250) return -0.8;   // ultralight
    if (w <= 350) return -0.4;   // light
    if (w <= 450) return 0.0;    // regular
    if (w <= 550) return 0.23;   // medium
    if (w <= 650) return 0.3;    // semibold
    if (w <= 750) return 0.4;    // bold
    return 0.56;                 // heavy
}

// The system UI font at `size` points and `weight`. The caller releases it.
static CTFontRef sb_weighted(double size, int weight) {
    CTFontRef base = CTFontCreateUIFontForLanguage(kCTFontUIFontSystem, size, NULL);
    if (!base) return NULL;
    CGFloat w = sb_weight(weight);
    CFNumberRef number = CFNumberCreate(NULL, kCFNumberCGFloatType, &w);
    const void *trait_keys[] = {kCTFontWeightTrait};
    const void *trait_values[] = {number};
    CFDictionaryRef traits =
        CFDictionaryCreate(NULL, trait_keys, trait_values, 1, &kCFTypeDictionaryKeyCallBacks,
                           &kCFTypeDictionaryValueCallBacks);
    const void *keys[] = {kCTFontTraitsAttribute};
    const void *values[] = {traits};
    CFDictionaryRef attributes =
        CFDictionaryCreate(NULL, keys, values, 1, &kCFTypeDictionaryKeyCallBacks,
                           &kCFTypeDictionaryValueCallBacks);
    CTFontDescriptorRef descriptor = CTFontDescriptorCreateWithAttributes(attributes);
    CTFontRef font = CTFontCreateCopyWithAttributes(base, size, NULL, descriptor);
    CFRelease(descriptor);
    CFRelease(attributes);
    CFRelease(traits);
    CFRelease(number);
    if (!font) return base;
    CFRelease(base);
    return font;
}

// The system UI font (SF Pro) or its monospaced design (SF Mono) at `size`
// points and `weight`. The weight comes first: a weight trait applied to the
// monospaced copy is ignored, while the monospace trait applied to a
// weighted system font keeps the weight. The caller releases it.
static CTFontRef sb_font(double size, int weight, int mono) {
    CTFontRef font = sb_weighted(size, weight);
    if (!font || !mono) return font;
    CTFontRef fixed = CTFontCreateCopyWithSymbolicTraits(font, size, NULL, kCTFontTraitMonoSpace,
                                                         kCTFontTraitMonoSpace);
    if (!fixed) return font;
    CFRelease(font);
    return fixed;
}

// A one-line CTLine of `utf8` in `font`, `kern` points after every character
// (0 is none), drawn in the context's fill colour. NULL for empty or invalid
// text. The caller releases it.
static CTLineRef sb_line(const char *utf8, CTFontRef font, double kern) {
    CFStringRef string = CFStringCreateWithCString(NULL, utf8, kCFStringEncodingUTF8);
    if (!string) return NULL;
    if (CFStringGetLength(string) == 0) {
        CFRelease(string);
        return NULL;
    }
    CFNumberRef kerning = CFNumberCreate(NULL, kCFNumberDoubleType, &kern);
    const void *keys[] = {kCTFontAttributeName, kCTForegroundColorFromContextAttributeName,
                          kCTKernAttributeName};
    const void *values[] = {font, kCFBooleanTrue, kerning};
    CFDictionaryRef attributes =
        CFDictionaryCreate(NULL, keys, values, kern != 0.0 ? 3 : 2,
                           &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    CFAttributedStringRef text = CFAttributedStringCreate(NULL, string, attributes);
    CTLineRef line = text ? CTLineCreateWithAttributedString(text) : NULL;
    if (text) CFRelease(text);
    CFRelease(attributes);
    CFRelease(kerning);
    CFRelease(string);
    return line;
}

// Typographic advance of `utf8` at `size` points, without drawing anything.
// Tracked text is measured by the caller (`kern` per character), so this
// stays the plain advance. Layout measures with this; only the renderer pays
// for a bitmap. Trailing spaces count, because the next run starts after
// them.
double sb_text_width(const char *utf8, double size, int weight, int mono) {
    if (!utf8 || size <= 0.0) return 0.0;
    CTFontRef font = sb_font(size, weight, mono);
    if (!font) return 0.0;
    CTLineRef line = sb_line(utf8, font, 0.0);
    CFRelease(font);
    if (!line) return 0.0;
    double width = CTLineGetTypographicBounds(line, NULL, NULL, NULL);
    CFRelease(line);
    return width;
}

// Rasterize `utf8` at `size` points and `weight` into a bitmap of `scale`
// pixels per point. `mono` picks the monospaced design; `kern` adds that many
// points after every character (letter-spacing).
//
// The font is always created at its POINT size and the context is scaled,
// never the font enlarged: SF Pro's tracking and optical size depend on the
// point size, so a 17 pt line requested as a 34 pt font is about 5% narrower
// than the layout measured, and gaps open between separately drawn runs.
//
// Returns a malloc'd A8 buffer of `*out_w * *out_h` bytes (rows are tightly
// packed, one byte per pixel) and writes the baseline's distance from the top
// edge into `*out_baseline`. Returns NULL if nothing was drawn; free a
// non-NULL result with sb_text_free.
unsigned char *sb_text_rasterize(const char *utf8, double size, double scale, int weight,
                                 int mono, double kern, int *out_w, int *out_h,
                                 int *out_baseline) {
    if (!utf8 || !out_w || !out_h || size <= 0.0) return NULL;
    if (!(scale > 0.0)) scale = 1.0;
    CTFontRef font = sb_font(size, weight, mono);
    if (!font) return NULL;
    CTLineRef line = sb_line(utf8, font, kern);
    CFRelease(font);
    if (!line) return NULL;

    CGFloat ascent = 0, descent = 0, leading = 0;
    double width = CTLineGetTypographicBounds(line, &ascent, &descent, &leading);
    // One pixel of slack on each side: glyphs routinely paint a hair outside
    // the typographic box (overshoot, sidebearing) and a clipped edge is far
    // more visible than a dead column.
    int pad = 1;
    int w = (int)ceil(width * scale) + pad * 2;
    int h = (int)ceil((ascent + descent) * scale) + pad * 2;
    if (w <= 0 || h <= 0 || w > 8192 || h > 8192) {
        CFRelease(line);
        return NULL;
    }

    unsigned char *buf = calloc((size_t)w * (size_t)h, 1);
    if (!buf) {
        CFRelease(line);
        return NULL;
    }
    CGContextRef ctx = CGBitmapContextCreate(buf, (size_t)w, (size_t)h, 8, (size_t)w, NULL,
                                             (CGBitmapInfo)kCGImageAlphaOnly);
    if (!ctx) {
        free(buf);
        CFRelease(line);
        return NULL;
    }
    CGContextSetShouldAntialias(ctx, true);
    CGContextSetShouldSmoothFonts(ctx, false);  // grayscale AA, never subpixel
    CGContextSetGrayFillColor(ctx, 1.0, 1.0);   // full coverage where ink falls
    CGContextTranslateCTM(ctx, (CGFloat)pad, (CGFloat)pad);
    CGContextScaleCTM(ctx, (CGFloat)scale, (CGFloat)scale);
    CGContextSetTextPosition(ctx, 0, descent);
    CTLineDraw(line, ctx);
    CGContextRelease(ctx);
    CFRelease(line);

    *out_w = w;
    *out_h = h;
    if (out_baseline) *out_baseline = (int)lround(ascent * scale) + pad;
    return buf;
}

void sb_text_free(unsigned char *p) {
    free(p);
}
