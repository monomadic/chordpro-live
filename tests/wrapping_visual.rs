//! Geometry/type guardrails for the capture fixtures; not window evidence.
use cpl_app::{App, Config, Polarity};
use cpl_render::CoreText;

#[test]
fn visual_fixtures_keep_readable_type_and_polarity_at_capture_sizes() {
    for (source, width, height) in [
        (
            include_str!("../testdata/scarborough_fair.cho"),
            420.0,
            720.0,
        ),
        (include_str!("visual/page-scroll.cho"), 360.0, 420.0),
        (include_str!("visual/wrapping.cho"), 420.0, 720.0),
        (include_str!("visual/type-ceiling.cho"), 980.0, 720.0),
    ] {
        let config = Config::default();
        let mut light_positions = None;
        for polarity in [Polarity::Light, Polarity::Dark] {
            let app = App::new(
                cpl_song::parse(source).song,
                config.clone(),
                polarity,
                &CoreText,
            );
            let page = app.page(width, height);
            let lyrics: Vec<_> = page
                .text
                .iter()
                .filter(|t| {
                    t.group > 0
                        && t.group != usize::MAX
                        && t.face == cpl_app::Face::System
                        && t.weight == 400
                })
                .collect();
            assert!(!lyrics.is_empty());
            for text in lyrics {
                assert!(
                    text.size >= config.body_size && text.size <= config.max_body_size,
                    "lyric {:?} has size {}",
                    text.content,
                    text.size
                );
            }
            let positions: Vec<_> = page
                .text
                .iter()
                .map(|t| (t.content.clone(), t.x, t.baseline, t.size, t.group))
                .collect();
            if let Some(ref light) = light_positions {
                assert_eq!(light, &positions, "polarity must preserve geometry");
            } else {
                light_positions = Some(positions);
            }
        }
    }
}

#[test]
fn type_endpoint_fixtures_reach_the_floor_and_ceiling() {
    for (source, width, height, expected) in [
        (include_str!("visual/page-scroll.cho"), 360.0, 420.0, 17.0),
        (include_str!("visual/type-ceiling.cho"), 980.0, 720.0, 28.0),
    ] {
        let app = App::new(
            cpl_song::parse(source).song,
            Config::default(),
            Polarity::Light,
            &CoreText,
        );
        let page = app.page(width, height);
        let lyrics: Vec<_> = page
            .text
            .iter()
            .filter(|t| {
                t.group > 0
                    && t.group != usize::MAX
                    && t.face == cpl_app::Face::System
                    && t.weight == 400
            })
            .collect();
        assert!(!lyrics.is_empty());
        assert!(
            lyrics.iter().all(|t| (t.size - expected).abs() < 0.01),
            "expected {expected} pt body: {lyrics:?}"
        );
    }
}
