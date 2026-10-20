use cpl_app::Measure;
use std::ffi::{CString, c_char, c_float, c_int, c_uchar};

unsafe extern "C" {
    fn sb_text_rasterize(
        utf8: *const c_char,
        size: f64,
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
}

pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub baseline: u32,
    pub coverage: Vec<u8>,
}

pub struct CoreText;

impl CoreText {
    pub fn rasterize(&self, text: &str, size_px: f32, weight: u16) -> Option<Raster> {
        let text = CString::new(text).ok()?;
        let (mut width, mut height, mut baseline) = (0, 0, 0);
        let pointer = unsafe {
            sb_text_rasterize(
                text.as_ptr(),
                size_px as f64,
                weight as c_int,
                0.0,
                0,
                0,
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
    fn width(&self, text: &str, size_pt: f32, weight: u16) -> f32 {
        self.rasterize(text, size_pt, weight)
            .map_or(0.0, |raster| raster.width as f32 - 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coretext_raster_has_ink_and_transparent_margin() {
        let raster = CoreText.rasterize("Hello", 24.0, 400).unwrap();
        assert!(raster.width > 20);
        assert!(raster.height > 10);
        assert_eq!(
            raster.coverage.len(),
            raster.width as usize * raster.height as usize
        );
        assert!(raster.coverage.iter().any(|&pixel| pixel > 0));
        assert!(raster.coverage.contains(&0));
    }
}
