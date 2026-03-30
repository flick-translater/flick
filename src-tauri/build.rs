fn main() {
    println!("cargo:rerun-if-changed=icons/icon.icns");
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=icons/trayTemplate.png");
    println!("cargo:rerun-if-changed=icons/trayTemplate@2x.png");
    println!("cargo:rerun-if-changed=icons/icon_32x32.png");
    println!("cargo:rerun-if-changed=icons/icon_128x128.png");
    println!("cargo:rerun-if-changed=icons/icon_128x128@2x.png");
    println!("cargo:rerun-if-changed=icons/icon_256x256.png");
    println!("cargo:rerun-if-changed=icons/icon_256x256@2x.png");
    println!("cargo:rerun-if-changed=icons/icon_512x512.png");
    println!("cargo:rerun-if-changed=icons/icon_512x512@2x.png");

    #[cfg(target_os = "macos")]
    {
        for swift_runtime_path in [
            "/usr/lib/swift",
            "/Library/Developer/CommandLineTools/usr/lib/swift/macosx",
            "/Library/Developer/CommandLineTools/usr/lib/swift-5.0/macosx",
            "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx",
            "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx",
        ] {
            if std::path::Path::new(swift_runtime_path).exists() {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_runtime_path}");
            }
        }
    }

    tauri_build::build()
}
