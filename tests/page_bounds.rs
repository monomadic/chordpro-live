//! Real CoreText ink against the page bounds the layout promised, at 1x and
//! 2x: what the fit arithmetic measured must be what the rasteriser draws.

use cpl_app::{App, Config, Frame, Polarity};
use cpl_render::CoreText;

/// Ink extents of `item` in page points: (left, right, top, bottom).
fn ink(item: &cpl_app::Text, scale: f32) -> Option<(f32, f32, f32, f32)> {
    let raster = CoreText.rasterize(&item.content, item.size, scale, item.weight, item.face)?;
    let rows = || raster.coverage.chunks_exact(raster.width as usize);
    let leftmost = rows()
        .filter_map(|row| row.iter().position(|&a| a > 0))
        .min()?;
    let rightmost = rows()
        .filter_map(|row| row.iter().rposition(|&a| a > 0))
        .max()?;
    let first_row = rows().position(|row| row.iter().any(|&a| a > 0))?;
    let last_row = rows().rposition(|row| row.iter().any(|&a| a > 0))?;
    let x0 = (item.x * scale).round() - 1.0;
    let y0 = (item.baseline * scale).round() - raster.baseline as f32;
    Some((
        (x0 + leftmost as f32) / scale,
        (x0 + rightmost as f32) / scale,
        (y0 + first_row as f32) / scale,
        (y0 + last_row as f32) / scale,
    ))
}

/// Every song item's ink lies inside the page's content box at both scales.
/// Side labels may sit in the left margin, so the left edge is the page's.
fn assert_ink_inside(app: &App, frame: &Frame, width: f32, height: f32, what: &str) {
    let c = app.config();
    let body = c.body_size;
    let content_end = height - c.margin_bottom - c.status_height * body;
    let mut checked = 0;
    for scale in [1.0, 2.0] {
        for item in frame.text.iter().filter(|item| item.group != usize::MAX) {
            let Some((left, right, top, bottom)) = ink(item, scale) else {
                continue; // whitespace draws nothing
            };
            checked += 1;
            let at = format!("{what} {width}×{height} at {scale}x: {:?}", item.content);
            assert!(left >= -2.0, "{at} starts at {left}");
            assert!(right <= width - c.margin_x + 2.0, "{at} ends at {right}");
            assert!(top >= c.margin_top - 2.0, "{at} rises to {top}");
            assert!(bottom <= content_end + 2.0, "{at} falls to {bottom}");
        }
    }
    assert!(checked > 0, "{what} {width}×{height} drew nothing");
}

fn app(source: &str) -> App {
    App::new(
        cpl_song::parse(source).song,
        Config::default(),
        Polarity::Light,
        &CoreText,
    )
}

#[test]
fn coretext_ink_stays_inside_fitted_page_at_one_and_two_x() {
    let app = app(
        "{title: A Rather Long Title}\n{artist: Ada}\n{sov: Verse 1}\n[C#m7]Hello [Gsus4]world\n[Am]Another line\n{eov}\n{soc: Chorus}\n[F]Sing it [C]again\n{eoc}",
    );
    for (width, height) in [(320.0, 500.0), (560.0, 500.0), (980.0, 720.0)] {
        let frame = app.frame(width, height);
        assert!(frame.fits, "{width}×{height} should fit after the ladder");
        assert_ink_inside(&app, &frame, width, height, "sample");
    }
}

#[test]
fn real_songs_stay_inside_at_every_size_they_fit_or_scroll() {
    for path in [
        "testdata/house_of_the_rising_sun.cho",
        "testdata/scarborough_fair.cho",
    ] {
        let source = std::fs::read_to_string(path).expect("test song");
        let app = app(&source);
        // Roomy, stand-sized, narrow enough to wrap, and short enough to scroll.
        for (width, height) in [
            (980.0, 720.0),
            (744.0, 1133.0),
            (420.0, 720.0),
            (280.0, 840.0),
            (560.0, 340.0),
        ] {
            let (frame, max_offset) = app.frame_scrolled(width, height, 0.0);
            assert_ink_inside(&app, &frame, width, height, path);
            if max_offset > 0.0 {
                let (frame, _) = app.frame_scrolled(width, height, max_offset);
                assert_ink_inside(&app, &frame, width, height, &format!("{path} (end)"));
            }
        }
    }
}

#[test]
fn wrapped_lines_and_long_chords_stay_inside() {
    let app = app(
        "{title: Wrap}\n{sov: Verse 1}\n[Cmaj7#11]Every single word of this long line [F#m7b5]has to fit inside a narrow [Bbsus2/D]window somehow\n[G]Short\n{eov}",
    );
    for width in [260.0, 340.0, 480.0] {
        let (frame, _) = app.frame_scrolled(width, 900.0, 0.0);
        assert!(
            frame.text.iter().any(|t| t.content.contains("somehow")),
            "{width}: the wrapped tail is drawn"
        );
        assert_ink_inside(&app, &frame, width, 900.0, "wrap");
    }
}
