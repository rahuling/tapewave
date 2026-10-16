fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/tapewave.rc");
    println!("cargo:rerun-if-changed=packaging/windows/tapewave.manifest");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("packaging/windows/tapewave.rc", embed_resource::NONE)
            .manifest_required()
            .expect("could not compile Windows application resources");
    }
}
