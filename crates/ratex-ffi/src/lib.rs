//! RaTeX C ABI FFI exports for native platform integration.
//!
//! Platform-specific modules:
//! - `jni` — Android JNI bridge (compiled only on `target_os = "android"`)
//!
//! ## DisplayList JSON protocol
//!
//! The primary output of this crate is a UTF-8 JSON string representing a `DisplayList`.
//! Treat this JSON as a **public protocol**: decoders should ignore unknown fields and
//! tolerate missing optional fields for forward/backward compatibility.
//!
//! See `docs/DISPLAYLIST_JSON_PROTOCOL.md` in the repository for the full schema and
//! change policy.
//!
//! # Usage (C) — layout JSON
//! ```c
//! RatexColor black = {0, 0, 0, 1};
//! RatexOptions opts = { sizeof(RatexOptions), 1, &black };  // display_mode=1 (block)
//! RatexResult result = ratex_parse_and_layout("\\frac{1}{2}", &opts);
//! if (result.error_code == 0) {
//!     // consume result.data ...
//!     ratex_free_display_list(result.data);
//! } else {
//!     const char* err = ratex_get_last_error();
//!     // handle error...
//! }
//! ```
//!
//! # Usage (C) — bitmap rasterization
//! ```c
//! RatexColor black = {0, 0, 0, 1};
//! RatexColor transparent = {0, 0, 0, 0};
//! RatexRenderOptions ropts = {
//!     sizeof(RatexRenderOptions), 1, &black,
//!     20.0f, 4.0f, 1.0f, transparent, NULL
//! };
//! RatexBitmapResult bmp = ratex_render_bitmap("\\frac{1}{2}", &ropts);
//! if (bmp.error_code == 0) {
//!     // bmp.bitmap.data is premultiplied RGBA8 (stride = width * 4)
//!     ratex_free_bitmap(bmp.bitmap);
//! }
//! ```

#[cfg(target_os = "android")]
pub mod jni;

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

use ratex_layout::{layout, to_display_list, LayoutOptions};
use ratex_parser::parse;
use ratex_render::{render_to_png, render_to_rgba, RenderOptions};
use ratex_svg::{render_to_svg, SvgOptions};
use ratex_types::display_item::DisplayList;
use ratex_types::math_style::MathStyle;
use serde_json::Value;

// Thread-local storage for the last error message.
thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn set_last_error(msg: &str) {
    let bytes: Vec<u8> = msg.bytes().filter(|&b| b != 0).collect();
    let stored = CString::new(bytes).unwrap_or_else(|_| {
        CString::new("(error message could not be encoded)").expect("static C string")
    });
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = Some(stored);
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Replace non-finite floats with 0 to produce valid JSON.
fn sanitize_json_numbers(v: Value) -> Value {
    match v {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.is_finite() {
                    Value::Number(n)
                } else {
                    Value::Number(serde_json::Number::from_f64(0.0).unwrap())
                }
            } else {
                Value::Number(n)
            }
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(sanitize_json_numbers).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, sanitize_json_numbers(v)))
                .collect(),
        ),
        other => other,
    }
}

fn do_layout(
    latex_str: &str,
    style: MathStyle,
    color: ratex_types::color::Color,
) -> Result<String, String> {
    let nodes = parse(latex_str).map_err(|e| format!("parse error: {e}"))?;
    let options = LayoutOptions::default().with_style(style).with_color(color);
    let layout_box = layout(&nodes, &options);
    let display_list = to_display_list(&layout_box);
    let value =
        serde_json::to_value(&display_list).map_err(|e| format!("serialization error: {e}"))?;
    let mut sanitized = sanitize_json_numbers(value);
    // Add a protocol version at the top level for forward-compatible decoding.
    if let Value::Object(ref mut map) = sanitized {
        map.insert("version".to_string(), Value::Number(1.into()));
    }
    serde_json::to_string(&sanitized).map_err(|e| format!("JSON stringify error: {e}"))
}

fn do_layout_display_list(
    latex_str: &str,
    style: MathStyle,
    color: ratex_types::color::Color,
) -> Result<DisplayList, String> {
    let nodes = parse(latex_str).map_err(|e| format!("parse error: {e}"))?;
    let options = LayoutOptions::default()
        .with_style(style)
        .with_color(color);
    let layout_box = layout(&nodes, &options);
    Ok(to_display_list(&layout_box))
}

fn do_render_bitmap(
    latex_str: &str,
    style: MathStyle,
    color: ratex_types::color::Color,
    render_options: &RenderOptions,
) -> Result<ratex_render::RenderedBitmap, String> {
    let display_list = do_layout_display_list(latex_str, style, color)?;
    render_to_rgba(&display_list, render_options)
}

fn do_render_png(
    latex_str: &str,
    style: MathStyle,
    color: ratex_types::color::Color,
    render_options: &RenderOptions,
) -> Result<Vec<u8>, String> {
    let display_list = do_layout_display_list(latex_str, style, color)?;
    render_to_png(&display_list, render_options)
}

fn do_render_svg(
    latex_str: &str,
    style: MathStyle,
    color: ratex_types::color::Color,
    svg_options: &SvgOptions,
) -> Result<String, String> {
    let display_list = do_layout_display_list(latex_str, style, color)?;
    Ok(render_to_svg(&display_list, svg_options))
}

fn parse_style_from_opts(display_mode: c_int) -> MathStyle {
    if display_mode == 0 {
        MathStyle::Text
    } else {
        MathStyle::Display
    }
}

fn parse_font_dir(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Ok(String::new());
    }
    match unsafe { CStr::from_ptr(ptr) }.to_str() {
        Ok(s) => Ok(s.to_owned()),
        Err(e) => Err(format!("invalid UTF-8 in font_dir: {e}")),
    }
}

fn validate_positive_finite(name: &str, value: f32) -> Result<f32, String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!(
            "invalid {name}: expected a finite float > 0, got {value}"
        ));
    }
    Ok(value)
}

const DEFAULT_FONT_SIZE: f32 = 20.0;
const DEFAULT_PADDING: f32 = 4.0;
const DEFAULT_DEVICE_PIXEL_RATIO: f32 = 1.0;
const DEFAULT_STROKE_WIDTH: f32 = 1.5;

struct ParsedRenderRequest {
    style: MathStyle,
    layout_color: ratex_types::color::Color,
    render: RenderOptions,
}

fn parse_layout_from_render_opts(
    opts: *const RatexRenderOptions,
) -> Result<ParsedRenderRequest, String> {
    let render = parse_render_options(opts)?;

    let style = if opts.is_null() {
        MathStyle::Display
    } else {
        let opts_ref = unsafe { &*opts };
        let min_size =
            std::mem::offset_of!(RatexRenderOptions, display_mode) + std::mem::size_of::<c_int>();
        if opts_ref.struct_size >= min_size {
            parse_style_from_opts(opts_ref.display_mode)
        } else {
            MathStyle::Display
        }
    };

    let layout_color = if opts.is_null() {
        ratex_types::color::Color::BLACK
    } else {
        let opts_ref = unsafe { &*opts };
        let color_size =
            std::mem::offset_of!(RatexRenderOptions, color) + std::mem::size_of::<*const RatexColor>();
        if opts_ref.struct_size >= color_size && !opts_ref.color.is_null() {
            validate_color(unsafe { *opts_ref.color })?
        } else {
            ratex_types::color::Color::BLACK
        }
    };

    Ok(ParsedRenderRequest {
        style,
        layout_color,
        render,
    })
}

fn parse_svg_options(opts: *const RatexRenderOptions, render: &RenderOptions) -> SvgOptions {
    let (stroke_width, embed_glyphs) = if opts.is_null() {
        (DEFAULT_STROKE_WIDTH, 1)
    } else {
        let opts_ref = unsafe { &*opts };
        let stroke_width = {
            let off = std::mem::offset_of!(RatexRenderOptions, stroke_width);
            if opts_ref.struct_size >= off + std::mem::size_of::<f32>() && opts_ref.stroke_width > 0.0
            {
                opts_ref.stroke_width
            } else {
                DEFAULT_STROKE_WIDTH
            }
        };
        let embed_glyphs = {
            let off = std::mem::offset_of!(RatexRenderOptions, embed_glyphs);
            if opts_ref.struct_size >= off + std::mem::size_of::<c_int>() {
                opts_ref.embed_glyphs
            } else {
                1
            }
        };
        (stroke_width, embed_glyphs)
    };

    SvgOptions {
        font_size: render.font_size as f64,
        padding: render.padding as f64,
        stroke_width: stroke_width as f64,
        embed_glyphs: embed_glyphs != 0,
        font_dir: render.font_dir.clone(),
    }
}

fn bytes_into_raw(mut data: Vec<u8>) -> RatexBytes {
    let ptr = data.as_mut_ptr();
    let len = data.len() as u32;
    std::mem::forget(data);
    RatexBytes { data: ptr, len }
}

fn string_into_raw(value: String) -> Result<*mut c_char, String> {
    CString::new(value).map(|cs| cs.into_raw()).map_err(|e| {
        format!("export string contains interior null byte: {e}")
    })
}

fn parse_render_options(opts: *const RatexRenderOptions) -> Result<RenderOptions, String> {
    if opts.is_null() {
        return Ok(RenderOptions {
            font_size: DEFAULT_FONT_SIZE,
            padding: DEFAULT_PADDING,
            background_color: ratex_types::color::Color::new(0.0, 0.0, 0.0, 0.0),
            font_dir: String::new(),
            device_pixel_ratio: DEFAULT_DEVICE_PIXEL_RATIO,
        });
    }

    let opts_ref = unsafe { &*opts };
    if opts_ref.struct_size < std::mem::size_of::<usize>() {
        return Err("RatexRenderOptions.struct_size is too small".to_string());
    }

    let display_mode = {
        let min_size =
            std::mem::offset_of!(RatexRenderOptions, display_mode) + std::mem::size_of::<c_int>();
        if opts_ref.struct_size >= min_size {
            opts_ref.display_mode
        } else {
            1
        }
    };
    let _ = parse_style_from_opts(display_mode);

    let font_size = {
        let off = std::mem::offset_of!(RatexRenderOptions, font_size);
        if opts_ref.struct_size >= off + std::mem::size_of::<f32>() {
            validate_positive_finite("font_size", opts_ref.font_size)?
        } else {
            DEFAULT_FONT_SIZE
        }
    };

    let padding = {
        let off = std::mem::offset_of!(RatexRenderOptions, padding);
        if opts_ref.struct_size >= off + std::mem::size_of::<f32>() {
            if !opts_ref.padding.is_finite() || opts_ref.padding < 0.0 {
                return Err(format!(
                    "invalid padding: expected a finite float >= 0, got {}",
                    opts_ref.padding
                ));
            }
            opts_ref.padding
        } else {
            DEFAULT_PADDING
        }
    };

    let device_pixel_ratio = {
        let off = std::mem::offset_of!(RatexRenderOptions, device_pixel_ratio);
        if opts_ref.struct_size >= off + std::mem::size_of::<f32>() {
            validate_positive_finite("device_pixel_ratio", opts_ref.device_pixel_ratio)?
        } else {
            DEFAULT_DEVICE_PIXEL_RATIO
        }
    };

    let background_color = {
        let off = std::mem::offset_of!(RatexRenderOptions, background_color);
        if opts_ref.struct_size >= off + std::mem::size_of::<RatexColor>() {
            validate_color(opts_ref.background_color)?
        } else {
            ratex_types::color::Color::new(0.0, 0.0, 0.0, 0.0)
        }
    };

    let font_dir = {
        let off = std::mem::offset_of!(RatexRenderOptions, font_dir);
        if opts_ref.struct_size >= off + std::mem::size_of::<*const c_char>() {
            parse_font_dir(opts_ref.font_dir)?
        } else {
            String::new()
        }
    };

    Ok(RenderOptions {
        font_size,
        padding,
        background_color,
        font_dir,
        device_pixel_ratio,
    })
}

fn bitmap_into_raw(rendered: ratex_render::RenderedBitmap) -> RatexBitmap {
    let mut data = rendered.data;
    let ptr = data.as_mut_ptr();
    let width = rendered.width;
    let height = rendered.height;
    let stride = rendered.stride;
    std::mem::forget(data);
    RatexBitmap {
        data: ptr,
        width,
        height,
        stride,
    }
}

// ---------------------------------------------------------------------------
// Public structs
// ---------------------------------------------------------------------------

/// Options for [`ratex_parse_and_layout`].
///
/// Always set `struct_size = sizeof(RatexOptions)` before passing to the function.
/// Fields beyond `struct_size` are ignored, enabling forward compatibility.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RatexColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl RatexColor {
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
}

impl From<RatexColor> for ratex_types::color::Color {
    fn from(value: RatexColor) -> Self {
        Self::new(value.r, value.g, value.b, value.a)
    }
}

fn validate_color(color: RatexColor) -> Result<ratex_types::color::Color, String> {
    fn validate_component(name: &str, value: f32) -> Result<(), String> {
        if !value.is_finite() {
            return Err(format!(
                "invalid color.{name}: expected a finite float in [0, 1], got {value}"
            ));
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(format!(
                "invalid color.{name}: expected a float in [0, 1], got {value}"
            ));
        }
        Ok(())
    }

    validate_component("r", color.r)?;
    validate_component("g", color.g)?;
    validate_component("b", color.b)?;
    validate_component("a", color.a)?;

    Ok(color.into())
}

#[repr(C)]
pub struct RatexOptions {
    /// Must be set to `sizeof(RatexOptions)` by the caller.
    pub struct_size: usize,
    /// Rendering mode:
    /// - `0` — inline (text style, equivalent to `$...$`)
    /// - `1` — display block (display style, equivalent to `$$...$$`)
    pub display_mode: c_int,
    /// Default formula color, in normalized RGBA.
    ///
    /// Explicit LaTeX color commands like `\color{...}` / `\textcolor{...}{...}`
    /// still override this per subtree.
    pub color: *const RatexColor,
}

/// Result returned by [`ratex_parse_and_layout`].
///
/// On success: `error_code == 0` and `data` is a heap-allocated JSON string;
/// free it with [`ratex_free_display_list`].
/// On error: `error_code != 0`, `data` is NULL; call [`ratex_get_last_error`] for details.
#[repr(C)]
pub struct RatexResult {
    /// JSON display list on success, NULL on error.
    pub data: *mut c_char,
    /// `0` on success, non-zero on error.
    pub error_code: c_int,
}

/// Premultiplied RGBA8 bitmap returned by [`ratex_render_bitmap`].
#[repr(C)]
pub struct RatexBitmap {
    /// Heap-allocated pixel buffer (`stride * height` bytes). NULL on error.
    pub data: *mut u8,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

/// Extended options for [`ratex_render_bitmap`].
///
/// Always set `struct_size = sizeof(RatexRenderOptions)` before passing to the function.
/// Fields beyond `struct_size` are ignored, enabling forward compatibility.
#[repr(C)]
pub struct RatexRenderOptions {
    pub struct_size: usize,
    /// `0` = inline, `1` = display block
    pub display_mode: c_int,
    /// Default formula color for layout. NULL = black.
    pub color: *const RatexColor,
    /// Font size in logical pixels (multiplied by `device_pixel_ratio` for output).
    pub font_size: f32,
    /// Padding around the formula in logical pixels.
    pub padding: f32,
    /// Device pixel ratio (1.0 = 96 DPI, 2.0 = 192 DPI, etc.).
    pub device_pixel_ratio: f32,
    /// Background fill color. Set `a = 0` for transparency.
    pub background_color: RatexColor,
    /// Optional UTF-8 path to KaTeX `.ttf` directory. Ignored when built with `embed-fonts`.
    pub font_dir: *const c_char,
    /// SVG export: stroke width in user units (`ratex_render_svg` only).
    pub stroke_width: f32,
    /// SVG export: `1` = standalone outlined glyphs, `0` = KaTeX `<text>` (`ratex_render_svg` only).
    pub embed_glyphs: c_int,
}

/// Binary buffer returned by [`ratex_render_png`].
#[repr(C)]
pub struct RatexBytes {
    pub data: *mut u8,
    pub len: u32,
}

/// Result returned by [`ratex_render_png`].
#[repr(C)]
pub struct RatexBytesResult {
    pub bytes: RatexBytes,
    pub error_code: c_int,
}

/// Result returned by [`ratex_render_bitmap`].
#[repr(C)]
pub struct RatexBitmapResult {
    pub bitmap: RatexBitmap,
    pub error_code: c_int,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a LaTeX string and compute its display list with explicit rendering options.
///
/// Pass `opts = NULL` to use display-mode defaults.
///
/// # Safety
/// - `latex` must be a valid non-null null-terminated UTF-8 C string.
/// - `opts` may be NULL. If non-null it must point to a valid `RatexOptions` whose
///   `struct_size` field is set correctly.
#[no_mangle]
pub unsafe extern "C" fn ratex_parse_and_layout(
    latex: *const c_char,
    opts: *const RatexOptions,
) -> RatexResult {
    let err_result = |msg: &str| -> RatexResult {
        set_last_error(msg);
        RatexResult {
            data: std::ptr::null_mut(),
            error_code: 1,
        }
    };

    clear_last_error();

    if latex.is_null() {
        return err_result("ratex_parse_and_layout: latex pointer is null");
    }

    let latex_str = match unsafe { CStr::from_ptr(latex) }.to_str() {
        Ok(s) => s,
        Err(e) => return err_result(&format!("invalid UTF-8 in latex string: {e}")),
    };

    let style = if opts.is_null() {
        MathStyle::Display
    } else {
        let opts_ref = unsafe { &*opts };
        let min_size =
            std::mem::offset_of!(RatexOptions, display_mode) + std::mem::size_of::<c_int>();
        if opts_ref.struct_size >= min_size && opts_ref.display_mode == 0 {
            MathStyle::Text
        } else {
            MathStyle::Display
        }
    };

    let color = if opts.is_null() {
        ratex_types::color::Color::BLACK
    } else {
        let opts_ref = unsafe { &*opts };
        let color_size =
            std::mem::offset_of!(RatexOptions, color) + std::mem::size_of::<*const RatexColor>();

        if opts_ref.struct_size >= color_size && !opts_ref.color.is_null() {
            match validate_color(unsafe { *opts_ref.color }) {
                Ok(color) => color,
                Err(msg) => return err_result(&msg),
            }
        } else {
            ratex_types::color::Color::BLACK
        }
    };

    match do_layout(latex_str, style, color) {
        Ok(json) => match CString::new(json) {
            Ok(cs) => RatexResult {
                data: cs.into_raw(),
                error_code: 0,
            },
            Err(e) => err_result(&format!("JSON contains interior null byte: {e}")),
        },
        Err(e) => err_result(&e),
    }
}

/// Free a display list JSON string returned by [`ratex_parse_and_layout`].
///
/// Passing NULL is a no-op.
///
/// # Safety
/// `ptr` must have been returned by [`ratex_parse_and_layout`] and must not be freed twice.
#[no_mangle]
pub unsafe extern "C" fn ratex_free_display_list(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// Return the last error message set by any layout function on this thread.
///
/// # Returns
/// - A pointer to a null-terminated error string, valid until the next layout call on this thread.
/// - NULL if no error has occurred on this thread.
///
/// # Safety
/// The returned pointer is only valid for the lifetime of the current thread and until the
/// next call to a layout function on this thread.
#[no_mangle]
pub extern "C" fn ratex_get_last_error() -> *const c_char {
    LAST_ERROR.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|cs| cs.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

/// Parse LaTeX and rasterize to a premultiplied RGBA8 bitmap via tiny-skia.
///
/// Pass `opts = NULL` to use defaults (display mode, 20px font, 4px padding, transparent bg).
///
/// # Safety
/// - `latex` must be a valid non-null null-terminated UTF-8 C string.
/// - `opts` may be NULL. If non-null it must point to a valid `RatexRenderOptions` whose
///   `struct_size` field is set correctly.
/// - On success, free `result.bitmap` with [`ratex_free_bitmap`].
#[no_mangle]
pub unsafe extern "C" fn ratex_render_bitmap(
    latex: *const c_char,
    opts: *const RatexRenderOptions,
) -> RatexBitmapResult {
    let empty_bitmap = RatexBitmap {
        data: std::ptr::null_mut(),
        width: 0,
        height: 0,
        stride: 0,
    };
    let err_result = |msg: &str| -> RatexBitmapResult {
        set_last_error(msg);
        RatexBitmapResult {
            bitmap: empty_bitmap,
            error_code: 1,
        }
    };

    clear_last_error();

    if latex.is_null() {
        return err_result("ratex_render_bitmap: latex pointer is null");
    }

    let latex_str = match unsafe { CStr::from_ptr(latex) }.to_str() {
        Ok(s) => s,
        Err(e) => return err_result(&format!("invalid UTF-8 in latex string: {e}")),
    };

    let parsed = match parse_layout_from_render_opts(opts) {
        Ok(parsed) => parsed,
        Err(msg) => return err_result(&msg),
    };

    match do_render_bitmap(
        latex_str,
        parsed.style,
        parsed.layout_color,
        &parsed.render,
    ) {
        Ok(rendered) => RatexBitmapResult {
            bitmap: bitmap_into_raw(rendered),
            error_code: 0,
        },
        Err(e) => err_result(&e),
    }
}

/// Free a bitmap returned by [`ratex_render_bitmap`].
///
/// Passing a bitmap with a NULL `data` pointer is a no-op.
///
/// # Safety
/// `bitmap.data` must have been returned by [`ratex_render_bitmap`] and must not be freed twice.
#[no_mangle]
pub unsafe extern "C" fn ratex_free_bitmap(bitmap: RatexBitmap) {
    if bitmap.data.is_null() || bitmap.stride == 0 || bitmap.height == 0 {
        return;
    }
    let len = (bitmap.stride as usize).saturating_mul(bitmap.height as usize);
    let _ = Vec::from_raw_parts(bitmap.data, len, len);
}

/// Parse LaTeX and export a PNG image.
///
/// Uses the same [`RatexRenderOptions`] as [`ratex_render_bitmap`]. On success, free
/// `result.bytes` with [`ratex_free_bytes`].
///
/// # Safety
/// Same requirements as [`ratex_render_bitmap`].
#[no_mangle]
pub unsafe extern "C" fn ratex_render_png(
    latex: *const c_char,
    opts: *const RatexRenderOptions,
) -> RatexBytesResult {
    let empty = RatexBytes {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let err_result = |msg: &str| -> RatexBytesResult {
        set_last_error(msg);
        RatexBytesResult {
            bytes: empty,
            error_code: 1,
        }
    };

    clear_last_error();

    if latex.is_null() {
        return err_result("ratex_render_png: latex pointer is null");
    }

    let latex_str = match unsafe { CStr::from_ptr(latex) }.to_str() {
        Ok(s) => s,
        Err(e) => return err_result(&format!("invalid UTF-8 in latex string: {e}")),
    };

    let parsed = match parse_layout_from_render_opts(opts) {
        Ok(parsed) => parsed,
        Err(msg) => return err_result(&msg),
    };

    match do_render_png(
        latex_str,
        parsed.style,
        parsed.layout_color,
        &parsed.render,
    ) {
        Ok(png) => RatexBytesResult {
            bytes: bytes_into_raw(png),
            error_code: 0,
        },
        Err(e) => err_result(&e),
    }
}

/// Parse LaTeX and export a standalone SVG document (UTF-8).
///
/// Set `embed_glyphs = 1` (default) for self-contained path outlines. On success, free
/// `result.data` with [`ratex_free_svg`].
///
/// # Safety
/// Same requirements as [`ratex_render_bitmap`].
#[no_mangle]
pub unsafe extern "C" fn ratex_render_svg(
    latex: *const c_char,
    opts: *const RatexRenderOptions,
) -> RatexResult {
    let err_result = |msg: &str| -> RatexResult {
        set_last_error(msg);
        RatexResult {
            data: std::ptr::null_mut(),
            error_code: 1,
        }
    };

    clear_last_error();

    if latex.is_null() {
        return err_result("ratex_render_svg: latex pointer is null");
    }

    let latex_str = match unsafe { CStr::from_ptr(latex) }.to_str() {
        Ok(s) => s,
        Err(e) => return err_result(&format!("invalid UTF-8 in latex string: {e}")),
    };

    let parsed = match parse_layout_from_render_opts(opts) {
        Ok(parsed) => parsed,
        Err(msg) => return err_result(&msg),
    };
    let svg_options = parse_svg_options(opts, &parsed.render);

    match do_render_svg(
        latex_str,
        parsed.style,
        parsed.layout_color,
        &svg_options,
    ) {
        Ok(svg) => match string_into_raw(svg) {
            Ok(ptr) => RatexResult {
                data: ptr,
                error_code: 0,
            },
            Err(e) => err_result(&e),
        },
        Err(e) => err_result(&e),
    }
}

/// Free a PNG buffer returned by [`ratex_render_png`].
///
/// # Safety
/// `bytes.data` must have been returned by [`ratex_render_png`] and must not be freed twice.
#[no_mangle]
pub unsafe extern "C" fn ratex_free_bytes(bytes: RatexBytes) {
    if bytes.data.is_null() || bytes.len == 0 {
        return;
    }
    let _ = Vec::from_raw_parts(bytes.data, bytes.len as usize, bytes.len as usize);
}

/// Free an SVG string returned by [`ratex_render_svg`].
///
/// # Safety
/// `ptr` must have been returned by [`ratex_render_svg`] and must not be freed twice.
#[no_mangle]
pub unsafe extern "C" fn ratex_free_svg(ptr: *mut c_char) {
    ratex_free_display_list(ptr);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::ffi::CString;

    /// Assert the default formula color applied to the first `GlyphPath` in the protocol JSON is black.
    ///
    /// We key off `type == "GlyphPath"` (see `docs/DISPLAYLIST_JSON_PROTOCOL.md`) instead of “first
    /// item with any `color`”, so fraction bars or paths cannot satisfy the assertion by accident.
    fn assert_default_glyph_path_color_is_black(json: &str) {
        let v: Value = serde_json::from_str(json).expect("valid display list JSON");
        let items = v
            .get("items")
            .and_then(|i| i.as_array())
            .expect("display list must have items array");
        let glyph = items
            .iter()
            .find(|item| {
                item.get("type")
                    .and_then(|t| t.as_str())
                    .is_some_and(|ty| ty == "GlyphPath")
            })
            .expect("expected at least one GlyphPath item");
        let color = glyph
            .get("color")
            .expect("GlyphPath must include color per DISPLAYLIST_JSON_PROTOCOL");
        let r = color.get("r").and_then(|x| x.as_f64());
        let g = color.get("g").and_then(|x| x.as_f64());
        let b = color.get("b").and_then(|x| x.as_f64());
        let a = color.get("a").and_then(|x| x.as_f64());
        assert_eq!((r, g, b, a), (Some(0.0), Some(0.0), Some(0.0), Some(1.0)));
    }

    fn call(latex: &str, display_mode: c_int) -> Option<String> {
        let input = CString::new(latex).unwrap();
        let black = RatexColor::BLACK;
        let opts = RatexOptions {
            struct_size: std::mem::size_of::<RatexOptions>(),
            display_mode,
            color: &black,
        };
        let result = unsafe { ratex_parse_and_layout(input.as_ptr(), &opts) };
        if result.error_code != 0 || result.data.is_null() {
            return None;
        }
        let json = unsafe { CStr::from_ptr(result.data) }
            .to_str()
            .unwrap()
            .to_owned();
        unsafe { ratex_free_display_list(result.data) };
        Some(json)
    }

    #[test]
    fn display_fraction() {
        let json = call(r"\frac{1}{2}", 1).expect("should not fail");
        assert!(json.starts_with('{'));
        assert!(json.contains("items"));
    }

    #[test]
    fn inline_fraction() {
        let json = call(r"\frac{1}{2}", 0).expect("should not fail");
        assert!(json.contains("items"));
    }

    #[test]
    fn display_expression() {
        let json = call("x^2 + y^2 = z^2", 1).expect("should not fail");
        assert!(json.contains("items"));
    }

    #[test]
    fn null_latex_returns_error() {
        let black = RatexColor::BLACK;
        let opts = RatexOptions {
            struct_size: std::mem::size_of::<RatexOptions>(),
            display_mode: 1,
            color: &black,
        };
        let result = unsafe { ratex_parse_and_layout(std::ptr::null(), &opts) };
        assert_ne!(result.error_code, 0);
        assert!(result.data.is_null());
        let err = ratex_get_last_error();
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("null"));
    }

    #[test]
    fn null_opts_defaults_to_display() {
        let input = CString::new(r"x^2").unwrap();
        let result = unsafe { ratex_parse_and_layout(input.as_ptr(), std::ptr::null()) };
        assert_eq!(result.error_code, 0);
        assert!(!result.data.is_null());
        unsafe { ratex_free_display_list(result.data) };
    }

    #[test]
    fn free_null_is_noop() {
        unsafe { ratex_free_display_list(std::ptr::null_mut()) };
    }

    #[test]
    fn error_on_bad_latex() {
        let result = call(r"\undefined{x}", 1);
        if result.is_none() {
            let err = ratex_get_last_error();
            assert!(!err.is_null());
        }
    }

    #[test]
    fn custom_color_applies_without_overriding_explicit_latex_color() {
        let input = CString::new(r"x + \color{red}{y}").unwrap();
        let blue = RatexColor {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        let opts = RatexOptions {
            struct_size: std::mem::size_of::<RatexOptions>(),
            display_mode: 1,
            color: &blue,
        };
        let result = unsafe { ratex_parse_and_layout(input.as_ptr(), &opts) };
        assert_eq!(result.error_code, 0);
        let json = unsafe { CStr::from_ptr(result.data) }
            .to_str()
            .unwrap()
            .to_owned();
        unsafe { ratex_free_display_list(result.data) };

        assert!(json.contains("\"b\":1.0"));
        assert!(json.contains("\"r\":1.0"));
    }

    #[repr(C)]
    struct LegacyRatexOptions {
        struct_size: usize,
        display_mode: c_int,
    }

    #[test]
    fn short_legacy_options_remain_binary_compatible() {
        let input = CString::new("x").unwrap();
        let legacy_opts = LegacyRatexOptions {
            struct_size: std::mem::size_of::<LegacyRatexOptions>(),
            display_mode: 1,
        };

        let result = unsafe {
            ratex_parse_and_layout(
                input.as_ptr(),
                &legacy_opts as *const LegacyRatexOptions as *const RatexOptions,
            )
        };
        assert_eq!(result.error_code, 0);
        assert!(!result.data.is_null());

        let json = unsafe { CStr::from_ptr(result.data) }
            .to_str()
            .unwrap()
            .to_owned();
        unsafe { ratex_free_display_list(result.data) };

        // Old callers do not provide the color tail, so layout must fall back to black.
        assert_default_glyph_path_color_is_black(&json);
    }

    #[test]
    fn invalid_color_returns_error() {
        let input = CString::new("x").unwrap();
        let invalid = RatexColor {
            r: f32::NAN,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let opts = RatexOptions {
            struct_size: std::mem::size_of::<RatexOptions>(),
            display_mode: 1,
            color: &invalid,
        };

        let result = unsafe { ratex_parse_and_layout(input.as_ptr(), &opts) };
        assert_ne!(result.error_code, 0);
        assert!(result.data.is_null());

        let err = ratex_get_last_error();
        assert!(!err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap();
        assert!(msg.contains("invalid color.r"));
    }

    #[test]
    fn null_color_pointer_defaults_to_black() {
        let input = CString::new("x").unwrap();
        let opts = RatexOptions {
            struct_size: std::mem::size_of::<RatexOptions>(),
            display_mode: 1,
            color: std::ptr::null(),
        };

        let result = unsafe { ratex_parse_and_layout(input.as_ptr(), &opts) };
        assert_eq!(result.error_code, 0);
        assert!(!result.data.is_null());

        let json = unsafe { CStr::from_ptr(result.data) }
            .to_str()
            .unwrap()
            .to_owned();
        unsafe { ratex_free_display_list(result.data) };

        assert_default_glyph_path_color_is_black(&json);
    }

    fn call_bitmap(latex: &str, display_mode: c_int) -> Option<RatexBitmap> {
        let input = CString::new(latex).unwrap();
        let black = RatexColor::BLACK;
        let transparent = RatexColor {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        let opts = RatexRenderOptions {
            struct_size: std::mem::size_of::<RatexRenderOptions>(),
            display_mode,
            color: &black,
            font_size: 20.0,
            padding: 4.0,
            device_pixel_ratio: 1.0,
            background_color: transparent,
            font_dir: std::ptr::null(),
            stroke_width: 1.5,
            embed_glyphs: 1,
        };
        let result = unsafe { ratex_render_bitmap(input.as_ptr(), &opts) };
        if result.error_code != 0 || result.bitmap.data.is_null() {
            return None;
        }
        Some(result.bitmap)
    }

    #[test]
    fn render_bitmap_fraction() {
        let bitmap = call_bitmap(r"\frac{1}{2}", 1).expect("should not fail");
        assert!(bitmap.width > 0);
        assert!(bitmap.height > 0);
        assert_eq!(bitmap.stride, bitmap.width * 4);
        unsafe { ratex_free_bitmap(bitmap) };
    }

    #[test]
    fn render_bitmap_null_latex_returns_error() {
        let black = RatexColor::BLACK;
        let opts = RatexRenderOptions {
            struct_size: std::mem::size_of::<RatexRenderOptions>(),
            display_mode: 1,
            color: &black,
            font_size: 20.0,
            padding: 4.0,
            device_pixel_ratio: 1.0,
            background_color: RatexColor::BLACK,
            font_dir: std::ptr::null(),
            stroke_width: 1.5,
            embed_glyphs: 1,
        };
        let result = unsafe { ratex_render_bitmap(std::ptr::null(), &opts) };
        assert_ne!(result.error_code, 0);
        assert!(result.bitmap.data.is_null());
    }

    #[test]
    fn free_bitmap_null_is_noop() {
        let bitmap = RatexBitmap {
            data: std::ptr::null_mut(),
            width: 0,
            height: 0,
            stride: 0,
        };
        unsafe { ratex_free_bitmap(bitmap) };
    }

    fn make_render_opts(display_mode: c_int) -> RatexRenderOptions {
        let black = RatexColor::BLACK;
        let transparent = RatexColor {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        RatexRenderOptions {
            struct_size: std::mem::size_of::<RatexRenderOptions>(),
            display_mode,
            color: &black,
            font_size: 20.0,
            padding: 4.0,
            device_pixel_ratio: 1.0,
            background_color: transparent,
            font_dir: std::ptr::null(),
            stroke_width: 1.5,
            embed_glyphs: 1,
        }
    }

    #[test]
    fn render_png_fraction() {
        let input = CString::new(r"\frac{1}{2}").unwrap();
        let opts = make_render_opts(1);
        let result = unsafe { ratex_render_png(input.as_ptr(), &opts) };
        assert_eq!(result.error_code, 0);
        assert!(!result.bytes.data.is_null());
        assert!(result.bytes.len > 8);
        let header = unsafe { std::slice::from_raw_parts(result.bytes.data, 8) };
        assert_eq!(&header[0..8], b"\x89PNG\r\n\x1a\n");
        unsafe { ratex_free_bytes(result.bytes) };
    }

    #[test]
    fn render_svg_fraction() {
        let input = CString::new(r"\frac{1}{2}").unwrap();
        let opts = make_render_opts(1);
        let result = unsafe { ratex_render_svg(input.as_ptr(), &opts) };
        assert_eq!(result.error_code, 0);
        assert!(!result.data.is_null());
        let svg = unsafe { CStr::from_ptr(result.data) }.to_str().unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
        unsafe { ratex_free_svg(result.data) };
    }
}
