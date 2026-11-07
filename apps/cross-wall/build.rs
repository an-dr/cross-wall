fn main() {
    // The icon Explorer, the taskbar, and shortcuts read. Other systems have
    // no such resource.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("failed to embed the Windows icon");
    }
}
