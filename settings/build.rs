mod learned_index;

fn main() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    build_busy_readings(&manifest);
    learned_index::build(&manifest);
    let ui = manifest.join("ui/app.slint");
    println!("cargo:rerun-if-changed={}", ui.display());
    // fluent 而不是 fluent-light / fluent-dark，深浅色留给运行时的 Palette.color-scheme。
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config(ui, config).expect("编译设置界面失败");
}

fn build_busy_readings(manifest: &std::path::Path) {
    use std::collections::{BTreeMap, HashSet};
    use std::io::BufRead;

    let path = manifest.join("../ime/src/dict.tsv");
    println!("cargo:rerun-if-changed={}", path.display());
    let reader = std::io::BufReader::new(std::fs::File::open(path).expect("open base lexicon"));
    let mut readings: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for line in reader.lines().skip(1) {
        let line = line.expect("read base lexicon");
        let mut fields = line.split('\t');
        let Some(head) = fields.next() else { continue };
        let Some(key) = fields.nth(1) else { continue };
        // 输入法接受字母 v 作为 ü 的键盘拼写。
        let words = readings.entry(key.replace('ü', "v")).or_default();
        if words.len() < 8 {
            words.insert(head.to_owned());
        }
    }
    let output = readings
        .into_iter()
        .filter(|(_, words)| words.len() >= 8)
        .map(|(key, _)| key)
        .collect::<Vec<_>>()
        .join("\n");
    let dest = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(dest.join("busy-readings.txt"), output).expect("write reading summary");
}
