fn main() {
    println!("cargo:rerun-if-changed=assets/aurora.rc");
    println!("cargo:rerun-if-changed=assets/aurora.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/aurora.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("could not embed the application icon");
    }
}
