use cpl_app::{Face, Measure};
use std::ffi::{CString, c_char, c_float, c_int, c_uchar};

unsafe extern "C" {
    fn sb_text_rasterize(
        utf8: *const c_char,
        size: f64,
        scale: f64,
        weight: c_int,
        max_width: f64,
        tnum: c_int,
        mono: c_int,
        out_w: *mut c_int,
        out_h: *mut c_int,
        out_baseline: *mut c_int,
        out_adv: *mut *mut c_float,
        out_adv_n: *mut c_int,
    ) -> *mut c_uchar;
    fn sb_text_free(p: *mut c_uchar);
    fn sb_text_width(utf8: *const c_char, size: f64, weight: c_int, mono: c_int) -> f64;
}

/// Pixels of slack the shim puts on each side of a raster (`pad` in text_shim.m).
pub(crate) const RASTER_PAD: i32 = 1;

pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub baseline: u32,
    pub coverage: Vec<u8>,
}

pub struct CoreText;

impl CoreText {
    /// Draws `text` set at `size_pt` into a bitmap of `scale` pixels per point.
    pub fn rasterize(
        &self,
        text: &str,
        size_pt: f32,
        scale: f32,
        weight: u16,
        face: Face,
    ) -> Option<Raster> {
        let text = CString::new(text).ok()?;
        let (mut width, mut height, mut baseline) = (0, 0, 0);
        let pointer = unsafe {
            sb_text_rasterize(
                text.as_ptr(),
                size_pt as f64,
                scale as f64,
                weight as c_int,
                0.0,
                0,
                i32::from(face == Face::Monospace),
                &mut width,
                &mut height,
                &mut baseline,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if pointer.is_null() || width <= 0 || height <= 0 {
            return None;
        }
        let length = width as usize * height as usize;
        let coverage = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
        unsafe { sb_text_free(pointer) };
        Some(Raster {
            width: width as u32,
            height: height as u32,
            baseline: baseline.max(0) as u32,
            coverage,
        })
    }
}

impl Measure for CoreText {
    /// Typographic advance only; no bitmap is drawn to measure.
    fn width(&self, text: &str, size_pt: f32, weight: u16, face: Face) -> f32 {
        let Ok(text) = CString::new(text) else {
            return 0.0;
        };
        unsafe {
            sb_text_width(
                text.as_ptr(),
                size_pt as f64,
                weight as c_int,
                i32::from(face == Face::Monospace),
            ) as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coretext_raster_has_ink_and_transparent_margin() {
        let raster = CoreText
            .rasterize("Hello", 24.0, 1.0, 400, Face::System)
            .unwrap();
        assert!(raster.width > 20);
        assert!(raster.height > 10);
        assert_eq!(
            raster.coverage.len(),
            raster.width as usize * raster.height as usize
        );
        assert!(raster.coverage.iter().any(|&pixel| pixel > 0));
        assert!(raster.coverage.contains(&0));
    }

    #[test]
    fn width_matches_the_raster_and_counts_trailing_space() {
        let width = CoreText.width("Hello", 24.0, 400, Face::System);
        let raster = CoreText
            .rasterize("Hello", 24.0, 1.0, 400, Face::System)
            .unwrap();
        // The raster is the advance rounded up plus one pixel of slack per side.
        assert!(raster.width as f32 - 2.0 - width >= 0.0);
        assert!(raster.width as f32 - 2.0 - width < 1.0);
        assert!(CoreText.width("Hello ", 24.0, 400, Face::System) > width + 3.0);
        assert_eq!(CoreText.width("", 24.0, 400, Face::System), 0.0);
    }

    #[test]
    fn retina_raster_keeps_the_measured_point_width() {
        // SF Pro at 34 pt is ~5% narrower than twice 17 pt; drawing at 2x must
        // keep the 17 pt advance the layout measured.
        let text = "Morning light is ";
        let width = CoreText.width(text, 17.0, 400, Face::System);
        let raster = CoreText
            .rasterize(text, 17.0, 2.0, 400, Face::System)
            .unwrap();
        let drawn = (raster.width as f32 - 2.0) / 2.0;
        assert!(
            (drawn - width).abs() < 1.0,
            "drawn {drawn} pt, measured {width} pt"
        );
    }

    #[test]
    fn monospaced_face_uses_equal_advances() {
        let mono_i = CoreText.width("iiii", 20.0, 600, Face::Monospace);
        let mono_w = CoreText.width("WWWW", 20.0, 600, Face::Monospace);
        let system_i = CoreText.width("iiii", 20.0, 600, Face::System);
        let system_w = CoreText.width("WWWW", 20.0, 600, Face::System);
        assert!((mono_i - mono_w).abs() < 1.0);
        assert!(system_w - system_i > 10.0);
    }
}
