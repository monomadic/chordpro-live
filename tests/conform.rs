//! The song note's conformance table must be what `chordpro-live conform` generates
//! now, so it cannot drift from the parser. Needs the reference ChordPro
//! (`cpan App::Music::ChordPro`); without it the test says so and passes.

use std::process::Command;

#[test]
fn the_conformance_table_is_current() {
    let out = Command::new(env!("CARGO_BIN_EXE_chordpro-live"))
        .args([
            "conform",
            "--table",
            "docs/architecture/chordpro.md",
            "--check",
        ])
        .output()
        .expect("run chordpro-live conform");
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("needs the reference ChordPro") {
        eprintln!("skipped: {}", stderr.trim());
        return;
    }
    assert!(out.status.success(), "{stderr}");
}
