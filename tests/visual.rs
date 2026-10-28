//! Visual regression: each case is rendered by the real `cpl render` (the
//! worker's page path plus the shader's placement rule) and compared with a
//! committed golden in `tests/goldens/`, a `sips`-compressed PNG you can open.
//!
//! A pixel differs when any channel moves by more than `CHANNEL_TOLERANCE`;
//! a case fails when more than `PIXEL_TOLERANCE` of its pixels differ, which
//! absorbs antialiasing noise from a macOS font update but not a moved line.
//! On failure the render and a diff (differing pixels in black) are written
//! to `target/visual/` beside the golden's name.
//!
//! After an intended visual change, look at the new renders, then bless them:
//!
//!     CPL_BLESS=1 cargo test --test visual
#![cfg(target_os = "macos")]

use cpl_render::png;
use std::{path::PathBuf, process::Command};

const CHANNEL_TOLERANCE: u8 = 24;
const PIXEL_TOLERANCE: f64 = 0.0002;

/// (golden name, `cpl render` arguments). Together they cover light and dark,
/// fit, scroll and paged pages, inline chords, wrapped lines and titles,
/// chords crowded over short syllables, tablature folded by the ladder and
/// shown in wrapped systems, the search screen with its pills and
/// dropping columns, 1x and 2x, and body sizes from the
/// 17 pt floor to the 28 pt maximum.
const CASES: &[(&str, &[&str])] = &[
    (
        "sample-light-980x720@1x",
        &["page", "--size", "980x720", "--scale", "1"],
    ),
    (
        "sample-dark-980x720@1x",
        &["page-dark", "--size", "980x720", "--scale", "1"],
    ),
    (
        "sample-inline-300x228@2x",
        &["page-inline", "--size", "300x228", "--scale", "2"],
    ),
    (
        "scroll-560x340@1x",
        &["page-scroll", "--size", "560x340", "--scale", "1"],
    ),
    (
        "scroll-560x340-at120@1x",
        &[
            "page-scroll",
            "--size",
            "560x340",
            "--scale",
            "1",
            "--scroll",
            "120",
        ],
    ),
    (
        "paged-dark-560x340-page2@1x",
        &[
            "page-scroll",
            "--size",
            "560x340",
            "--scale",
            "1",
            "--page",
            "2",
            "--dark",
        ],
    ),
    (
        "house-980x720@1x",
        &[
            "testdata/house_of_the_rising_sun.cho",
            "--size",
            "980x720",
            "--scale",
            "1",
        ],
    ),
    (
        "house-980x720@2x",
        &[
            "testdata/house_of_the_rising_sun.cho",
            "--size",
            "980x720",
            "--scale",
            "2",
        ],
    ),
    (
        "crowded-560x340@1x",
        &[
            "testdata/crowded_chords.cho",
            "--size",
            "560x340",
            "--scale",
            "1",
        ],
    ),
    (
        "scarborough-420x720@1x",
        &[
            "testdata/scarborough_fair.cho",
            "--size",
            "420x720",
            "--scale",
            "1",
        ],
    ),
    (
        "scarborough-280x840@1x",
        &[
            "testdata/scarborough_fair.cho",
            "--size",
            "280x840",
            "--scale",
            "1",
        ],
    ),
    (
        "greensleeves-980x720@1x",
        &[
            "testdata/greensleeves.cho",
            "--size",
            "980x720",
            "--scale",
            "1",
        ],
    ),
    (
        "greensleeves-tabs-980x720@1x",
        &[
            "testdata/greensleeves.cho",
            "--size",
            "980x720",
            "--scale",
            "1",
            "--tabs",
            "show",
        ],
    ),
    (
        "greensleeves-tabs-dark-420x720-page5@1x",
        &[
            "testdata/greensleeves.cho",
            "--size",
            "420x720",
            "--scale",
            "1",
            "--tabs",
            "show",
            "--dark",
            "--page",
            "5",
        ],
    ),
    (
        "search-ris-1133x744@1x",
        &[
            "testdata",
            "--search",
            "ris",
            "--store",
            "tests/fixtures/library.toml",
            "--size",
            "1133x744",
            "--scale",
            "1",
        ],
    ),
    (
        "search-filters-dark-420x600@2x",
        &[
            "testdata",
            "--search",
            "+folk *4",
            "--store",
            "tests/fixtures/library.toml",
            "--size",
            "420x600",
            "--scale",
            "2",
            "--dark",
        ],
    ),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn render(name: &str, args: &[&str]) -> PathBuf {
    let out = root().join("target/visual").join(format!("{name}.png"));
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_cpl"))
        .current_dir(root())
        .arg("render")
        .args(args)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run cpl render");
    assert!(
        status.status.success(),
        "{name}: cpl render failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    out
}

/// Re-encodes a raw render through `sips`, which compresses it ~50×.
fn bless(raw: &PathBuf, golden: &PathBuf) {
    let status = Command::new("sips")
        .args(["-s", "format", "png"])
        .arg(raw)
        .arg("--out")
        .arg(golden)
        .output()
        .expect("run sips");
    assert!(status.status.success(), "sips failed for {golden:?}");
}

#[test]
fn renders_match_their_goldens() {
    let blessing = std::env::var_os("CPL_BLESS").is_some();
    let mut failures = Vec::new();
    for (name, args) in CASES {
        let raw = render(name, args);
        let golden = root().join("tests/goldens").join(format!("{name}.png"));
        if blessing {
            bless(&raw, &golden);
            continue;
        }
        let Ok(expected) = std::fs::read(&golden) else {
            failures.push(format!(
                "{name}: no golden; run with CPL_BLESS=1 after checking {raw:?}"
            ));
            continue;
        };
        let expected = png::decode(&expected).expect("golden decodes");
        let actual = png::decode(&std::fs::read(&raw).unwrap()).expect("render decodes");
        if (actual.width, actual.height) != (expected.width, expected.height) {
            failures.push(format!(
                "{name}: {}×{} rendered, golden is {}×{}",
                actual.width, actual.height, expected.width, expected.height
            ));
            continue;
        }
        let mut diff = Vec::with_capacity(actual.rgba.len());
        let mut differing = 0usize;
        for (a, e) in actual
            .rgba
            .chunks_exact(4)
            .zip(expected.rgba.chunks_exact(4))
        {
            let moved = a
                .iter()
                .zip(e)
                .any(|(a, e)| a.abs_diff(*e) > CHANNEL_TOLERANCE);
            differing += usize::from(moved);
            diff.extend_from_slice(if moved {
                &[0, 0, 0, 255]
            } else {
                &[255, 255, 255, 255]
            });
        }
        let pixels = (actual.width * actual.height) as usize;
        eprintln!("{name}: {differing} of {pixels} pixels differ");
        if differing as f64 > pixels as f64 * PIXEL_TOLERANCE {
            let diff_path = raw.with_extension("diff.png");
            std::fs::write(&diff_path, png::encode(actual.width, actual.height, &diff)).unwrap();
            failures.push(format!(
                "{name}: {differing} of {pixels} pixels differ; see {raw:?} and {diff_path:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "visual regressions:\n{}",
        failures.join("\n")
    );
}
