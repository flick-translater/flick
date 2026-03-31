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
    println!("cargo:rerun-if-changed=native/ocr-tool.swift");

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=Vision");

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

        let out_dir = std::env::var("OUT_DIR").unwrap();
        let ocr_tool_path = std::path::Path::new("native/ocr-tool.swift");

        if ocr_tool_path.exists() {
            let output_binary = std::path::Path::new(&out_dir).join("ocr-tool");

            let sdk_path = std::process::Command::new("xcrun")
                .args(["--sdk", "macosx", "--show-sdk-path"])
                .output()
                .ok()
                .and_then(|output| {
                    if output.status.success() {
                        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
                    } else {
                        None
                    }
                });

            let target_arch =
                std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "aarch64".to_string());
            let target_triple = match target_arch.as_str() {
                "x86_64" => "x86_64-apple-macosx10.15",
                "aarch64" => "arm64-apple-macosx11.0",
                _ => "arm64-apple-macosx11.0",
            };

            let mut cmd = std::process::Command::new("swiftc");
            cmd.args([
                "-O",
                "-whole-module-optimization",
                "-target",
                target_triple,
                "-o",
                output_binary.to_str().unwrap(),
                ocr_tool_path.to_str().unwrap(),
                "-framework",
                "Vision",
                "-framework",
                "CoreGraphics",
                "-framework",
                "Foundation",
            ]);

            if let Some(ref sdk) = sdk_path {
                cmd.args(["-sdk", sdk]);
            }

            let status = cmd.status().expect("Failed to compile Swift OCR tool");

            if !status.success() {
                eprintln!("Warning: Failed to compile Swift OCR tool, falling back to swift -e");
            } else {
                println!("cargo:rustc-env=OCR_TOOL_PATH={}", output_binary.display());
            }
        }
    }

    tauri_build::build()
}
