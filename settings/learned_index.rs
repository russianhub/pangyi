use std::collections::{BTreeMap, HashMap};

#[cfg(not(test))]
pub fn build(manifest: &std::path::Path) {
    use std::{env, fs, path::PathBuf};

    let dictionary = manifest.join("../ime/src/dict.tsv");
    let overrides = manifest.join("../data/gloss-overrides.tsv");
    println!("cargo:rerun-if-changed={}", dictionary.display());
    println!("cargo:rerun-if-changed={}", overrides.display());
    let dictionary = fs::read_to_string(dictionary).expect("read base dictionary");
    let overrides = fs::read_to_string(overrides).expect("read shipped gloss overrides");
    let tsv = unique_readings(&dictionary, &overrides);
    let destination = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("learned-readings.deflate");
    fs::write(destination, miniz_oxide::deflate::compress_to_vec(tsv.as_bytes(), 6))
        .expect("write learned reading index");
}

/// Only heads with exactly one known reading can bypass the editor.
pub fn unique_readings(dictionary: &str, overrides: &str) -> String {
    let glosses: HashMap<_, _> = overrides
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split('\t').map(str::trim);
            Some(((fields.next()?, fields.next()?), fields.next()?))
        })
        .collect();
    let mut words = BTreeMap::new();
    for line in dictionary.lines().skip(1) {
        let mut fields = line.split('\t');
        let Some(head) = fields.next() else { continue };
        let Some(key) = fields.nth(1) else { continue };
        let Some(display) = fields.next() else { continue };
        let russian = fields.nth(2).unwrap_or("");
        if head.is_empty() || key.is_empty() {
            continue;
        }
        let display = if display.is_empty() { key } else { display };
        let russian = glosses
            .get(&(head, display))
            .or_else(|| glosses.get(&(head, key)))
            .or_else(|| glosses.get(&(head, "")))
            .copied()
            .unwrap_or(russian);
        let reading = (key, display, russian);
        words.entry(head)
            .and_modify(|previous: &mut Option<(&str, &str, &str)>| {
                if let Some((old_key, old_display, old_russian)) = previous {
                    if (*old_key, *old_display) != (key, display) {
                        *previous = None;
                    } else if old_russian.is_empty() {
                        *old_russian = russian;
                    }
                }
            })
            .or_insert(Some(reading));
    }

    let mut tsv = String::new();
    for (head, reading) in words {
        if let Some((key, _, russian)) = reading {
            tsv.push_str(head);
            tsv.push('\t');
            tsv.push_str(key);
            tsv.push('\t');
            tsv.push_str(russian);
            tsv.push('\n');
        }
    }
    tsv
}
