fn main() {
    // The launcher is what shortcuts point at, so it carries the app's icon.
    // The icon file belongs to the app; the launcher only borrows it.
    let icon = "../cross-wall/assets/icon.ico";
    println!("cargo:rerun-if-changed={icon}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon(icon)
            .compile()
            .expect("failed to embed the Windows icon");
    }
}
