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
        println!("cargo:rustc-link-lib=framework=Vision");

        // System Swift runtime (always available on macOS 10.14+)
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

        // Try to find additional Swift runtime from Xcode/CommandLineTools
        if let Ok(output) = std::process::Command::new("xcrun")
            .args(["--find", "swift"])
            .output()
        {
            if output.status.success() {
                let swift_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if let Some(swift_dir) = std::path::Path::new(&swift_path).parent() {
                    let lib_path = swift_dir.join("../lib/swift/macosx");
                    if lib_path.exists() {
                        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path.display());
                    }
                }
            }
        }
    }

    tauri_build::build()
}
