fn main() {
    println!("cargo:rerun-if-changed=res/poni.rc");
    println!("cargo:rerun-if-changed=res/poni.manifest");
    println!("cargo:rerun-if-changed=res/poni.ico");
    // Only Windows targets get the icon, manifest (Per-Monitor-V2 DPI) and version info.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Panics with a clear message if rc.exe / x86_64-w64-mingw32-windres is missing.
        embed_resource::compile("res/poni.rc", embed_resource::NONE);
    }
}
