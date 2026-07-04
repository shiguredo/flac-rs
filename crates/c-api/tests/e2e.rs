//! C API の E2E テスト
//!
//! 静的ライブラリ (libflac.a) をビルドして C のサンプル・テストを
//! コンパイル・実行し、C API が実際に C から利用できることを検証する
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_c_examples_compile() {
    let project_root = get_project_root();
    let lib_path = build_static_lib(&project_root);

    // examples ディレクトリから全ての .c ファイルを検索する
    let c_files: Vec<_> = std::fs::read_dir(project_root.join("crates/c-api/examples/"))
        .expect("examples ディレクトリの読み込みに失敗した")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "c") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    assert!(
        !c_files.is_empty(),
        "examples ディレクトリに .c ファイルが見つからない"
    );

    // 各 C ファイルをコンパイルする
    for c_file in c_files {
        let example_name = c_file
            .file_stem()
            .expect("ファイル名の取得に失敗した")
            .to_string_lossy();
        let output_path = lib_path
            .parent()
            .expect("ライブラリの親ディレクトリの取得に失敗した")
            .join(format!("{example_name}"));

        // C コンパイラでビルドする
        let mut cmd = Command::new("cc");
        cmd.arg(&c_file)
            .arg("-o")
            .arg(&output_path)
            .arg(&lib_path)
            .arg("-I")
            .arg(project_root.join("crates/c-api/include"));

        // sin() などの数学関数のために libm をリンクする (Windows 以外)
        #[cfg(not(target_os = "windows"))]
        cmd.arg("-lm");

        // Windows のみ追加のライブラリをリンクする
        #[cfg(target_os = "windows")]
        cmd.arg("-lws2_32").arg("-lntdll").arg("-luserenv");

        let status = cmd.status().expect("cc コマンドの実行に失敗した");

        assert!(
            status.success(),
            "サンプルのコンパイルに失敗した: {example_name}"
        );
    }
}

#[test]
fn test_simple_encode_decode() {
    let project_root = get_project_root();
    let lib_path = build_static_lib(&project_root);

    let c_file = project_root.join("crates/c-api/tests/simple_encode_decode.c");
    assert!(
        c_file.exists(),
        "simple_encode_decode.c が {} に存在しない",
        c_file.display()
    );

    let output_path = lib_path
        .parent()
        .expect("ライブラリの親ディレクトリの取得に失敗した")
        .join("simple_encode_decode");

    // C ファイルをコンパイルする
    let mut cmd = Command::new("cc");
    cmd.arg(&c_file)
        .arg("-o")
        .arg(&output_path)
        .arg(&lib_path)
        .arg("-I")
        .arg(project_root.join("crates/c-api/include"));

    // Windows のみ追加のライブラリをリンクする
    #[cfg(target_os = "windows")]
    cmd.arg("-lws2_32").arg("-lntdll").arg("-luserenv");

    let status = cmd
        .status()
        .expect("simple_encode_decode.c のコンパイルに失敗した");

    assert!(
        status.success(),
        "simple_encode_decode.c のコンパイルに失敗した"
    );

    // コンパイルされた実行ファイルを実行する
    let status = Command::new(&output_path)
        .status()
        .expect("simple_encode_decode の実行に失敗した");

    assert!(status.success(), "simple_encode_decode の実行に失敗した");
}

/// 静的ライブラリ (libflac.a) をビルドして、そのパスを返す
///
/// `cargo test` は staticlib 成果物を生成しないため、テスト内で明示的に
/// `cargo build` を実行する必要がある
fn build_static_lib(project_root: &std::path::Path) -> PathBuf {
    // カバレッジ計測 (cargo llvm-cov) の RUSTFLAGS を引き継ぐと、生成した
    // libflac.a のリンクにプロファイラランタイムが必要になり cc で
    // リンクできなくなるため、計測フラグを取り除いてビルドする
    let status = Command::new(env!("CARGO"))
        .args(["build", "--package", "c-api"])
        .current_dir(project_root)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .expect("cargo build の実行に失敗した");
    assert!(status.success(), "libflac.a のビルドに失敗した");

    // CARGO_TARGET_DIR が設定されている場合 (cargo llvm-cov 実行時など) は
    // 成果物の出力先がリポジトリ直下の target/ ではなくなる
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| project_root.join("target"));
    let lib_path = target_dir.join("debug").join("libflac.a");
    assert!(
        lib_path.exists(),
        "ビルドした libflac.a が {} に存在しない",
        lib_path.display()
    );
    lib_path
}

fn get_project_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("プロジェクトルートの取得に失敗した")
        .to_path_buf()
}
