fn main() {
    println!("cargo:rerun-if-changed=../../assets/pulse.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/pulse.ico")
            .set("ProductName", "Pulse")
            .set("FileDescription", "Pulse process explorer")
            .compile()
            .expect("failed to embed Windows resources");
    }
}
