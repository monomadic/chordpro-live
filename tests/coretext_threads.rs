//! CoreText measurement must not depend on what other threads are drawing.
//! Measuring a song while other threads rasterised once gave wrong advances
//! (a side label placed at −21 pt in 6 of 60 layouts); `cpl-render`
//! serialises the shim's calls to prevent it.

use cpl_app::{App, Config, Face, Measure, Polarity};
use cpl_render::CoreText;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn layout(song: &cpl_song::Song) -> Vec<(String, i32, i32)> {
    App::new(song.clone(), Config::default(), Polarity::Light, &CoreText)
        .frame(280.0, 840.0)
        .text
        .iter()
        .map(|t| {
            (
                t.content.clone(),
                (t.x * 100.0) as i32,
                (t.size * 100.0) as i32,
            )
        })
        .collect()
}

#[test]
fn a_layout_is_the_same_while_other_threads_rasterise() {
    let source = std::fs::read_to_string("testdata/house_of_the_rising_sun.cho").unwrap();
    let song = cpl_song::parse(&source).song;
    let reference = layout(&song);
    let stop = Arc::new(AtomicBool::new(false));
    let noise: Vec<_> = (0..3)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let words = [
                    "A Rather Long Title",
                    "Hello ",
                    "C#m7",
                    "VERSE 1",
                    "Sing it ",
                ];
                for _ in 0..40 {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    for word in words {
                        for scale in [1.0, 2.0] {
                            CoreText.rasterize(word, 17.0, scale, 600, Face::System);
                            CoreText.rasterize(word, 12.0, scale, 700, Face::Monospace);
                            let _ = CoreText.width(word, 13.0, 600, Face::Monospace);
                        }
                    }
                }
            })
        })
        .collect();
    let differing = (0..30).filter(|_| layout(&song) != reference).count();
    stop.store(true, Ordering::Relaxed);
    for thread in noise {
        thread.join().unwrap();
    }
    assert_eq!(differing, 0, "layouts changed under concurrent drawing");
}
