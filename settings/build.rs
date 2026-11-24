fn main() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ui = manifest.join("ui/app.slint");
    println!("cargo:rerun-if-changed={}", ui.display());
    // fluent 而不是 fluent-light / fluent-dark，深浅色留给运行时的 Palette.color-scheme。
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config(ui, config).expect("编译设置界面失败");
}
