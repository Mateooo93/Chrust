fn main() {
    println!("cargo:rerun-if-changed=assets/chrust-port/chrust-port.rc");
    println!("cargo:rerun-if-changed=assets/chrust-port/chrust-port.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/chrust-port/chrust-port.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("could not embed the application icon");
    }
}
