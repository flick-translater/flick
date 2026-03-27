fn main() {
    println!("cargo:rerun-if-changed=icons/icon.icns");
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=icons/icon_32x32.png");
    println!("cargo:rerun-if-changed=icons/icon_128x128.png");
    println!("cargo:rerun-if-changed=icons/icon_128x128@2x.png");
    println!("cargo:rerun-if-changed=icons/icon_256x256.png");
    println!("cargo:rerun-if-changed=icons/icon_256x256@2x.png");
    println!("cargo:rerun-if-changed=icons/icon_512x512.png");
    println!("cargo:rerun-if-changed=icons/icon_512x512@2x.png");
    tauri_build::build()
}
