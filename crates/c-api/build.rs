fn main() {
    let version = get_root_version();
    println!("cargo::rustc-env=SHIGUREDO_FLAC_VERSION={version}");

    cbindgen::Builder::new()
        .with_crate(env!("CARGO_MANIFEST_DIR"))
        .with_language(cbindgen::Language::C)
        .with_cpp_compat(true)
        .with_include_version(true)
        .with_include_guard("SHIGUREDO_FLAC_H")
        .with_no_includes()
        .with_sys_include("stdbool.h")
        .with_sys_include("stdint.h")
        .generate()
        .expect("Failed to generate C bindings")
        .write_to_file("include/flac.h");
}

/// ルートの Cargo.toml からライブラリのバージョンを取得する
fn get_root_version() -> String {
    let root_cargo_toml = include_str!("../../Cargo.toml");
    root_cargo_toml
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("version") {
                trimmed
                    .split_once('=')
                    .map(|(_, v)| v.trim().trim_matches('"'))
            } else {
                None
            }
        })
        .expect("version not found")
        .to_string()
}
