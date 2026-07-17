/**
 * ratex.h — RaTeX C ABI public header
 *
 * Layout:
 *   ratex_parse_and_layout / ratex_free_display_list
 *
 * WPF on-screen drawing (premultiplied RGBA8):
 *   ratex_render_bitmap / ratex_free_bitmap
 *
 * File export:
 *   ratex_render_png  / ratex_free_bytes
 *   ratex_render_svg  / ratex_free_svg
 */

#ifndef RATEX_H
#define RATEX_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>
#include <stdint.h>

typedef struct {
    float r;
    float g;
    float b;
    float a;
} RatexColor;

typedef struct {
    size_t struct_size;
    int display_mode; /* 0 = inline, 1 = display */
    const RatexColor* color;
} RatexOptions;

typedef struct {
    char* data;
    int error_code;
} RatexResult;

typedef struct {
    uint8_t* data;
    uint32_t width;
    uint32_t height;
    uint32_t stride;
} RatexBitmap;

typedef struct {
    size_t struct_size;
    int display_mode;
    const RatexColor* color;
    float font_size;
    float padding;
    float device_pixel_ratio;
    RatexColor background_color;
    const char* font_dir;
    float stroke_width;   /* SVG export only */
    int embed_glyphs;     /* SVG export only: 1 = standalone paths */
} RatexRenderOptions;

typedef struct {
    RatexBitmap bitmap;
    int error_code;
} RatexBitmapResult;

typedef struct {
    uint8_t* data;
    uint32_t len;
} RatexBytes;

typedef struct {
    RatexBytes bytes;
    int error_code;
} RatexBytesResult;

RatexResult ratex_parse_and_layout(const char* latex, const RatexOptions* opts);
void ratex_free_display_list(char* json);
const char* ratex_get_last_error(void);

RatexBitmapResult ratex_render_bitmap(const char* latex, const RatexRenderOptions* opts);
void ratex_free_bitmap(RatexBitmap bitmap);

RatexBytesResult ratex_render_png(const char* latex, const RatexRenderOptions* opts);
void ratex_free_bytes(RatexBytes bytes);

RatexResult ratex_render_svg(const char* latex, const RatexRenderOptions* opts);
void ratex_free_svg(char* svg);

#ifdef __cplusplus
}
#endif

#endif /* RATEX_H */
