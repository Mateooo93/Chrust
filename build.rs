fn main() {
    println!("cargo:rerun-if-changed=assets/chrust/chrust.rc");
    println!("cargo:rerun-if-changed=assets/chrust/chrust.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/chrust/chrust.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("could not embed the application icon");
    }
}
