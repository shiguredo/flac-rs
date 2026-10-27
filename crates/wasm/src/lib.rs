//! shiguredo_flac の wasm バインディング
//!
//! c-api の機能に加えて、wasm 固有の機能を提供する
#![warn(missing_docs)]
#![expect(clippy::missing_safety_doc)]

pub mod decoder;

use std::alloc::Layout;

/// wasm のメモリ上で `FlacDecodedFrame` を確保するために必要なバイト数を取得する
///
/// JavaScript 側で構造体のサイズをハードコードすると、構造体の定義を
/// 変えたときに静かに壊れるため、サイズをこの関数で取得する
///
/// # 戻り値
///
/// `FlacDecodedFrame` のバイト数
#[unsafe(no_mangle)]
pub extern "C" fn flac_decoded_frame_size() -> u32 {
    size_of::<c_api::decoder::FlacDecodedFrame>() as u32
}

/// wasm のメモリ上で `FlacStreamInfo` を確保するために必要なバイト数を取得する
///
/// # 戻り値
///
/// `FlacStreamInfo` のバイト数
#[unsafe(no_mangle)]
pub extern "C" fn flac_stream_info_size() -> u32 {
    size_of::<c_api::decoder::FlacStreamInfo>() as u32
}

/// wasm のメモリ上で `FlacVorbisComment` を確保するために必要なバイト数を取得する
///
/// # 戻り値
///
/// `FlacVorbisComment` のバイト数
#[unsafe(no_mangle)]
pub extern "C" fn flac_vorbis_comment_size() -> u32 {
    size_of::<c_api::decoder::FlacVorbisComment>() as u32
}

/// wasm のメモリ上で `FlacVorbisCommentField` を確保するために必要なバイト数を取得する
///
/// # 戻り値
///
/// `FlacVorbisCommentField` のバイト数
#[unsafe(no_mangle)]
pub extern "C" fn flac_vorbis_comment_field_size() -> u32 {
    size_of::<c_api::decoder::FlacVorbisCommentField>() as u32
}

/// メモリを確保する
///
/// この関数で確保したメモリは `flac_free()` で解放すること
///
/// 返されるポインタは 8 バイト境界に整列される。`Int32Array` /
/// `Float64Array` などの型付き配列で直接読み書きできる
///
/// # 引数
///
/// - `size`: 確保するバイト数
///
/// # 戻り値
///
/// 確保したメモリの先頭アドレス。`size` が 0 の場合とメモリ確保に
/// 失敗した場合は NULL
#[unsafe(no_mangle)]
pub extern "C" fn flac_alloc(size: u32) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    // アラインメント 1 で確保すると、wasm のメモリアロケータが返す
    // アドレスが 4 の倍数になる保証がなく、JavaScript 側で
    // Int32Array のビューを作るときに RangeError になり得る。
    // 8 バイトに揃えておけば u64 / f64 まで安全に読み書きできる
    let layout = Layout::from_size_align(size as usize, 8)
        .expect("layout creation with alignment 8 should never fail");
    unsafe { std::alloc::alloc(layout) }
}

/// `flac_alloc()` で確保したメモリを解放する
///
/// # 引数
/// - `ptr`: 解放するメモリの先頭アドレス
/// - `size`: 解放するバイト数
///
/// 以下の場合には関数呼び出しは単に無視される:
/// - `ptr` が NULL
/// - `size` が 0
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_free(ptr: *mut u8, size: u32) {
    if ptr.is_null() || size == 0 {
        return;
    }
    let layout = Layout::from_size_align(size as usize, 8)
        .expect("layout creation with alignment 8 should never fail");
    unsafe { std::alloc::dealloc(ptr, layout) };
}

/// `Vec<u8>` のポインタを取得する
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_vec_ptr(v: *const Vec<u8>) -> *const u8 {
    if v.is_null() {
        return std::ptr::null();
    }
    unsafe { (*v).as_ptr() }
}

/// `Vec<u8>` の長さを取得する
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_vec_len(v: *const Vec<u8>) -> u32 {
    if v.is_null() {
        return 0;
    }
    unsafe {
        u32::try_from((*v).len()).expect("vec length should not exceed u32::MAX in wasm32 target")
    }
}

/// `Vec<u8>` を解放する
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_vec_free(v: *mut Vec<u8>) {
    if !v.is_null() {
        let _ = unsafe { Box::from_raw(v) };
    }
}
