use cpl_app::{App, Config, Polarity};
use cpl_render::CoreText;

#[test]
fn coretext_ink_stays_inside_fitted_page_at_one_and_two_x() {
    let song = cpl_song::parse(
        "{title: A Rather Long Title}\n{artist: Ada}\n{sov: Verse 1}\n[C#m7]Hello [Gsus4]world\n[Am]Another line\n{eov}\n{soc: Chorus}\n[F]Sing it [C]again\n{eoc}",
    )
    .song;
    let app = App::new(song, Config::default(), Polarity::Light, &CoreText);
    for (width, height) in [(320.0, 500.0), (560.0, 500.0), (980.0, 720.0)] {
        let frame = app.frame(width, height);
        assert!(frame.fits, "{width}×{height} should fit after the ladder");
        let body = frame
            .text
            .iter()
            .find(|item| item.content == "Another line")
            .expect("lyric is visible")
            .size;
        for scale in [1.0, 2.0] {
            for item in frame.text.iter().filter(|item| item.group != usize::MAX) {
                let raster = CoreText
                    .rasterize(&item.content, item.size, scale, item.weight, item.face)
                    .expect("visible text should rasterize");
                let rightmost_ink = raster
                    .coverage
                    .chunks_exact(raster.width as usize)
                    .filter_map(|row| row.iter().rposition(|&alpha| alpha > 0))
                    .max()
                    .unwrap_or(0);
                let leftmost_ink = raster
                    .coverage
                    .chunks_exact(raster.width as usize)
                    .filter_map(|row| row.iter().position(|&alpha| alpha > 0))
                    .min()
                    .unwrap_or(0);
                let first_ink_row = raster
                    .coverage
                    .chunks_exact(raster.width as usize)
                    .position(|row| row.iter().any(|&alpha| alpha > 0))
                    .unwrap_or(0);
                let last_ink_row = raster
                    .coverage
                    .chunks_exact(raster.width as usize)
                    .rposition(|row| row.iter().any(|&alpha| alpha > 0))
                    .unwrap_or(0);
                let right = ((item.x * scale).round() - 1.0 + rightmost_ink as f32) / scale;
                let left = ((item.x * scale).round() - 1.0 + leftmost_ink as f32) / scale;
                let top = ((item.baseline * scale).round() - raster.baseline as f32
                    + first_ink_row as f32)
                    / scale;
                let bottom = ((item.baseline * scale).round() - raster.baseline as f32
                    + last_ink_row as f32)
                    / scale;
                assert!(
                    left >= -2.0,
                    "{width} pt page at {scale}x: {:?} starts at {left}",
                    item.content
                );
                assert!(
                    right <= width - app.config().margin_x + 2.0,
                    "{width} pt page at {scale}x: {:?} ends at {right}",
                    item.content
                );
                assert!(
                    top >= app.config().margin_top - 2.0,
                    "{height} pt page at {scale}x: {:?} starts at {top}",
                    item.content
                );
                assert!(
                    bottom
                        <= height - app.config().margin_bottom - app.config().status_height * body
                            + 2.0,
                    "{height} pt page at {scale}x: {:?} ends at {bottom}",
                    item.content
                );
            }
        }
    }
}
