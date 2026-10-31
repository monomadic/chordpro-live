#[cfg(target_os = "macos")]
fn main() {
    use std::{env, path::PathBuf, process::Command};
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let object = out.join("text_shim.o");
    let archive = out.join("libcpltext.a");
    let status = Command::new("clang")
        .args(["-c", "src/text_shim.m", "-o"])
        .arg(&object)
        .status()
        .expect("Command Line Tools clang");
    assert!(status.success(), "CoreText shim compilation failed");
    let status = Command::new("ar")
        .arg("crs")
        .arg(&archive)
        .arg(&object)
        .status()
        .expect("Command Line Tools ar");
    assert!(status.success(), "CoreText shim archive failed");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=cpltext");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=CoreText");
    println!("cargo:rerun-if-changed=src/text_shim.m");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
