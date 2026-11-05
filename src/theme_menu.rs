//! The macOS Theme menu: an item per accent in `THEMES`, each with a swatch
//! of its colour. AppKit calls back on the main thread; the choice is kept
//! here and the event loop woken, so the viewer picks it up in
//! `user_event` and requests a page in the new colour.

use std::sync::{
    OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use cpl_app::{Polarity, THEMES};

const NONE: usize = usize::MAX;
static CHOICE: AtomicUsize = AtomicUsize::new(NONE);
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn cpl_theme_menu_install(
        names: *const *const std::ffi::c_char,
        rgb: *const u8,
        count: std::ffi::c_long,
        selected: std::ffi::c_long,
        callback: extern "C" fn(std::ffi::c_long),
    );
}

extern "C" fn chosen(index: std::ffi::c_long) {
    CHOICE.store(index as usize, Ordering::SeqCst);
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

/// Adds the Theme menu with `selected` checked; `wake` nudges the event
/// loop when an item is chosen. Call once, on the main thread.
pub fn install(selected: usize, wake: impl Fn() + Send + Sync + 'static) {
    if WAKE.set(Box::new(wake)).is_err() {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let names: Vec<std::ffi::CString> = THEMES
            .iter()
            .map(|t| std::ffi::CString::new(t.name).unwrap())
            .collect();
        let pointers: Vec<*const std::ffi::c_char> = names.iter().map(|n| n.as_ptr()).collect();
        // The swatch shows the colour as it reads on white.
        let rgb: Vec<u8> = THEMES.iter().flat_map(|t| t.rgb(Polarity::Light)).collect();
        // SAFETY: the arrays outlive the call, which copies what it keeps.
        unsafe {
            cpl_theme_menu_install(
                pointers.as_ptr(),
                rgb.as_ptr(),
                THEMES.len() as std::ffi::c_long,
                selected as std::ffi::c_long,
                chosen,
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = selected;
}

/// The theme chosen since the last call, if any.
pub fn take_choice() -> Option<usize> {
    let index = CHOICE.swap(NONE, Ordering::SeqCst);
    (index < THEMES.len()).then_some(index)
}
