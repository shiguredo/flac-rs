//! shiguredo_flac の C API
//!
//! Sans I/O な FLAC デコーダー / エンコーダーを C から利用するためのバインディング
#![expect(clippy::missing_safety_doc)]
pub mod decoder;
pub mod encoder;
pub mod error;

/// ライブラリのバージョンを取得する
///
/// # 戻り値
///
/// バージョン文字列へのポインタ（NULL 終端）
#[unsafe(no_mangle)]
pub extern "C" fn flac_library_version() -> *const std::ffi::c_char {
    concat!(env!("SHIGUREDO_FLAC_VERSION"), "\0")
        .as_ptr()
        .cast()
}
