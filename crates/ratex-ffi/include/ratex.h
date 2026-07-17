/**
 * ratex.h — RaTeX C ABI public header
 *
 * Provides LaTeX layout (JSON DisplayList) and bitmap rasterization for WPF / .NET.
 *
 * Layout usage:
 *   RatexColor black = {0, 0, 0, 1};
 *   RatexOptions opts = { sizeof(RatexOptions), 1, &black };
 *   RatexResult r = ratex_parse_and_layout("\\frac{1}{2}", &opts);
 *   if (r.error_code == 0) {
 *       ratex_free_display_list(r.data);
 *   }
 *
 * Bitmap usage:
 *   RatexColor black = {0, 0, 0, 1};
 *   RatexColor transparent = {0, 0, 0, 0};
 *   RatexRenderOptions ropts = {
 *       sizeof(RatexRenderOptions), 1, &black,
 *       20.0f, 4.0f, 1.0f, transparent, NULL
 *   };
 *   RatexBitmapResult br = ratex_render_bitmap("\\frac{1}{2}", &ropts);
 *   if (br.error_code == 0) {
 *       // br.bitmap.data is premultiplied RGBA8, stride = width * 4
 *       ratex_free_bitmap(br.bitmap);
 *   }
 *
 * display_mode values:
 *   1 — display (block) style, equivalent to $$...$$
 *   0 — inline (text) style,   equivalent to $...$
 *
 * Thread safety:
 *   Functions use thread-local storage for error state and are safe to call
 *   concurrently from multiple threads (each thread has its own last-error slot).
 *
 * Bitmap pixel format:
 *   Premultiplied RGBA8, row-major, top-to-bottom. stride is typically width * 4.
 *   WPF WriteableBitmap expects straight alpha — convert if needed before blitting.
 */

#ifndef RATEX_H
#define RATEX_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>
#include <stdint.h>

typedef struct {
    float r; /* normalized 0..1 */
    float g; /* normalized 0..1 */
    float b; /* normalized 0..1 */
    float a; /* normalized 0..1 */
} RatexColor;

typedef struct {
    size_t struct_size;
    int display_mode; /* 0 = inline ($...$), 1 = display block ($$...$$) */
    const RatexColor* color; /* NULL = default black */
} RatexOptions;

typedef struct {
    char* data;      /* JSON display list on success, NULL on error */
    int error_code;  /* 0 on success, non-zero on error */
} RatexResult;

typedef struct {
    uint8_t* data;   /* premultiplied RGBA8 pixels, NULL on error */
    uint32_t width;
    uint32_t height;
    uint32_t stride; /* bytes per row, typically width * 4 */
} RatexBitmap;

typedef struct {
    size_t struct_size;
    int display_mode;
    const RatexColor* color;
    float font_size;
    float padding;
    float device_pixel_ratio;
    RatexColor background_color;
    const char* font_dir; /* NULL = use embedded fonts when available */
} RatexRenderOptions;

typedef struct {
    RatexBitmap bitmap;
    int error_code;
} RatexBitmapResult;

RatexResult ratex_parse_and_layout(const char* latex, const RatexOptions* opts);
void ratex_free_display_list(char* json);
const char* ratex_get_last_error(void);

RatexBitmapResult ratex_render_bitmap(const char* latex, const RatexRenderOptions* opts);
void ratex_free_bitmap(RatexBitmap bitmap);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RATEX_H */
