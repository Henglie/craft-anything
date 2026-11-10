//! Optional craft-fonts (fallbacks for CJK, Arabic, Thai, Hebrew, Devanagari…), following
//! craft-fonts' `docs/integration.md`. Releases and CI set it.
//!
//! Unset `CRAFT_FONTS_DIR` (the default) embeds nothing and the app uses its own DM Sans/JetBrains
//! Mono. Set it to an absolute path of a craft-fonts checkout to embed every font its manifest
//! lists. A bad path is a warning unless `CRAFT_FONTS_REQUIRED=1`.
//!
//! This also merges each language's catalogs under `src/i18n/<code>/`: every `*.tsv` file in name
//! order, joined with a newline, into `OUT_DIR/i18n/<code>.tsv` for `i18n.rs`'s `include_str!`.
//! A key defined in two files fails the build.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let mut src = String::from("pub static CRAFT_FONTS: &[CraftFont] = &[\n");
    if let Some(dir) = std::env::var_os("CRAFT_FONTS_DIR").map(PathBuf::from) {
        match craft_fonts(&dir) {
            Ok(entries) => src.push_str(&entries),
            Err(e) if std::env::var_os("CRAFT_FONTS_REQUIRED").is_some() => {
                println!("cargo::error=CRAFT_FONTS_DIR={}: {e}", dir.display());
            }
            Err(e) => println!("cargo::warning=building without craft-fonts: CRAFT_FONTS_DIR={}: {e}", dir.display()),
        }
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&out, src) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
    if let Err(e) = merge_catalogs() {
        println!("cargo::error={e}");
        std::process::exit(1);
    }
}

/// One `CraftFont { .. }` initialiser per manifest line.
fn craft_fonts(dir: &Path) -> Result<String, String> {
    let manifest = dir.join("fonts/manifest.txt");
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    const WEB_FONTS: &[(&str, &str)] = &[("BIZ UDPGothic", "Regular")];
    let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");
    let mut out = String::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, ..] = f.as_slice() else {
            return Err(format!("malformed manifest line: {line}"));
        };
        if wasm && !WEB_FONTS.contains(&(*family, *style)) {
            continue;
        }
        let path = dir.join(file).canonicalize().map_err(|e| format!("{file}: {e}"))?;
        println!("cargo::rerun-if-changed={}", path.display());
        let scripts: Vec<String> = scripts.split(',').map(|s| format!("{:?}", s.trim())).collect();
        let _ = writeln!(
            out,
            "    CraftFont {{ family: {family:?}, style: {style:?}, scripts: &[{}], bytes: include_bytes!({:?}) }},",
            scripts.join(", "),
            path.display().to_string(),
        );
    }
    Ok(out)
}

/// Concatenate every `*.tsv` in each `src/i18n/<code>/` folder, sorted by file name, into
/// `OUT_DIR/i18n/<code>.tsv`. Files are joined with a newline when one doesn't end in one.
fn merge_catalogs() -> Result<(), String> {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is not set")?);
    let root = manifest.join("src/i18n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?).join("i18n");
    println!("cargo::rerun-if-changed={}", root.display());
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|e| format!("{}: {e}", root.display()))? {
        let path = entry.map_err(|e| format!("{}: {e}", root.display()))?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();
    for dir in dirs {
        println!("cargo::rerun-if-changed={}", dir.display());
        let Some(code) = dir.file_name().and_then(|n| n.to_str()) else { continue };
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "tsv") {
                files.push(path);
            }
        }
        if files.is_empty() {
            continue;
        }
        files.sort();
        let mut merged = String::new();
        let mut seen: HashMap<String, PathBuf> = HashMap::new();
        for file in &files {
            let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
            for (i, line) in text.lines().enumerate() {
                if line.trim_start().starts_with('#') {
                    continue;
                }
                let Some((key, value)) = line.split_once('\t') else { continue };
                if key.is_empty() || value.trim().is_empty() {
                    continue;
                }
                let key = key.replace("\\n", "\n");
                if let Some(previous) = seen.insert(key.clone(), file.clone()) {
                    return Err(format!("{}:{}: duplicate key {key:?}, already in {}", file.display(), i + 1, previous.display()));
                }
            }
            if !merged.is_empty() && !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push_str(&text);
        }
        if !merged.is_empty() && !merged.ends_with('\n') {
            merged.push('\n');
        }
        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        let path = out.join(format!("{code}.tsv"));
        std::fs::write(&path, merged).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}
