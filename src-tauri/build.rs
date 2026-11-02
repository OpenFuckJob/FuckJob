fn main() {
    tauri_build::build();
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("windows-msvc")) {
        let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("examples/windows.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        // Library unit-test binaries do not receive rustc-link-arg-tests.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}",manifest.display());
        // The desktop binary already embeds Tauri's manifest in resource.lib.
        // Do not let the linker create a second resource with the same ID.
        println!("cargo:rustc-link-arg-bin=offer_flow=/MANIFEST:NO");
        println!("cargo:rustc-link-arg-examples=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",manifest.display());
        println!(
            "cargo:rustc-link-arg-examples=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
