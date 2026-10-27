//! ../../../src/decoder.rs の C API を定義するためのモジュール
use std::ffi::{CString, c_char};

use crate::error::FlacError;

/// FLAC ストリームの STREAMINFO メタデータを表す構造体 (RFC 9639 Section 8.2)
#[repr(C)]
pub struct FlacStreamInfo {
    /// ストリーム中の最小ブロックサイズ (サンプル数、最終ブロックを除く)。16-65535
    pub min_block_size: u16,

    /// ストリーム中の最大ブロックサイズ (サンプル数)。16-65535
    pub max_block_size: u16,

    /// 最小フレームサイズ (バイト)。0 は不明を表す
    pub min_frame_size: u32,

    /// 最大フレームサイズ (バイト)。0 は不明を表す
    pub max_frame_size: u32,

    /// サンプルレート (Hz)。1-1048575
    pub sample_rate: u32,

    /// チャンネル数 (1-8)
    pub channels: u8,

    /// サンプルあたりのビット数 (4-32)
    pub bits_per_sample: u8,

    /// 総インターチャンネルサンプル数。0 は不明を表す
    pub total_samples: u64,

    /// エンコード前オーディオデータの MD5 チェックサム。全て 0 は不明を表す
    pub md5: [u8; 16],
}

/// デコードした 1 フレーム分の音声を表す構造体
///
/// `samples` が参照するバッファは `FlacDecoder` が所有しており、
/// 同じデコーダーに対して次の `flac_decoder_decode_frame()` を呼び出すか
/// `flac_decoder_free()` を呼び出すと無効になる
#[repr(C)]
pub struct FlacDecodedFrame {
    /// チャンネルインターリーブ済み (L, R, L, R, ...) のサンプル列へのポインタ
    pub samples: *const i32,

    /// `samples` の要素数 (ブロックサイズ x チャンネル数)
    pub sample_count: u32,

    /// 実効サンプルレート (Hz)
    pub sample_rate: u32,

    /// このフレームの最初のインターチャンネルサンプル番号
    pub first_sample_number: u64,

    /// チャンネル数
    pub channels: u8,

    /// 実効ビット深度
    pub bits_per_sample: u8,
}

/// VORBIS_COMMENT の 1 フィールド (name=value) を表す構造体 (RFC 9639 Section 8.6)
///
/// `name` と `value` が参照するバッファは `FlacDecoder` が所有しており、
/// 同じデコーダーに対して次の `flac_decoder_decode_frame()` を呼び出すか
/// `flac_decoder_free()` を呼び出すと無効になる
#[repr(C)]
pub struct FlacVorbisCommentField {
    /// フィールド名へのポインタ (NUL 終端、UTF-8)
    pub name: *const c_char,

    /// フィールド値へのポインタ (NUL 終端、UTF-8)
    pub value: *const c_char,
}

/// FLAC ストリームの VORBIS_COMMENT メタデータブロックを表す構造体 (RFC 9639 Section 8.6)
///
/// `vendor` と `fields` が参照するバッファは `FlacDecoder` が所有しており、
/// 同じデコーダーに対して次の `flac_decoder_decode_frame()` を呼び出すか
/// `flac_decoder_free()` を呼び出すと無効になる
#[repr(C)]
pub struct FlacVorbisComment {
    /// ファイルを生成したプログラム名へのポインタ (NUL 終端、UTF-8)
    pub vendor: *const c_char,

    /// フィールド列へのポインタ。フィールドがなければ NULL
    pub fields: *const FlacVorbisCommentField,

    /// `fields` の要素数
    pub field_count: u32,
}

/// FLAC ストリームのデコード処理を行うための構造体
///
/// # 関連関数
///
/// この構造体は、以下の関数を通して操作する必要がある:
/// - `flac_decoder_new()`: `FlacDecoder` インスタンスを生成する
/// - `flac_decoder_free()`: リソースを解放する
/// - `flac_decoder_feed()`: 入力データを投入する
/// - `flac_decoder_finish()`: 入力の終端を通知する
/// - `flac_decoder_decode_frame()`: フレームをひとつデコードする
/// - `flac_decoder_get_stream_info()`: STREAMINFO を取得する
/// - `flac_decoder_get_last_error()`: 最後に発生したエラーのメッセージを取得する
///
/// # 使用例
///
/// ```c
/// #include <stdio.h>
/// #include <stdlib.h>
/// #include "flac.h"
///
/// int main(void) {
///     // 1. FlacDecoder インスタンスを生成
///     FlacDecoder *decoder = flac_decoder_new();
///
///     // 2. FLAC データを投入 (ストリーミングであれば受信のたびに呼ぶ)
///     const uint8_t *flac_data = ...;
///     uint32_t flac_size = ...;
///     flac_decoder_feed(decoder, flac_data, flac_size);
///
///     // 3. 入力の終端を通知 (これにより MD5 検証などの完全性チェックが行われる)
///     flac_decoder_finish(decoder);
///
///     // 4. フレームを順番にデコード
///     FlacDecodedFrame frame;
///     FlacError ret;
///     while ((ret = flac_decoder_decode_frame(decoder, &frame)) == FLAC_ERROR_OK) {
///         for (uint32_t i = 0; i < frame.sample_count; i++) {
///             // frame.samples[i] を処理する...
///         }
///     }
///     if (ret != FLAC_ERROR_NO_MORE_FRAMES) {
///         fprintf(stderr, "Failed to decode frame: %s\n", flac_decoder_get_last_error(decoder));
///         flac_decoder_free(decoder);
///         return 1;
///     }
///
///     // 5. リソース解放
///     flac_decoder_free(decoder);
///     return 0;
/// }
/// ```
pub struct FlacDecoder {
    inner: shiguredo_flac::decoder::StreamDecoder,
    /// 直近にデコードしたフレーム
    /// (`FlacDecodedFrame::samples` が参照するバッファの実体を保持する)
    current_frame: Option<shiguredo_flac::decoder::DecodedFrame>,
    /// `flac_decoder_finish()` が呼ばれたか
    finished: bool,
    last_error_string: Option<CString>,
    /// `flac_decoder_get_vorbis_comment()` が返すデータの実体を保持する
    ///
    /// `FlacVorbisComment` / `FlacVorbisCommentField` が参照する C 文字列と
    /// フィールド配列へのポインタを有効に保つため、呼び出しのたびに作り直す
    vorbis_comment: Option<VorbisCommentCache>,
}

/// C API で返す VORBIS_COMMENT のキャッシュ
struct VorbisCommentCache {
    /// vendor 文字列 (NUL 終端)
    vendor: CString,
    /// フィールド列 (`FlacVorbisCommentField` の配列)
    fields: Vec<FlacVorbisCommentField>,
    /// `vendor` と `fields` が指す C 文字列の実体
    ///
    /// 先頭に `_` を付けて「読み出さないが生存させ続ける」ことを明示する。
    /// このフィールドを落とすと `fields` のポインタがダングリングになる
    _entries: Vec<(CString, CString)>,
}

impl FlacDecoder {
    fn set_last_error(&mut self, message: &str) {
        // NUL バイトを含む極めて稀なケースではメッセージを切り詰める
        self.last_error_string = CString::new(message.split('\0').next().unwrap_or("")).ok();
    }
}

/// 新しい `FlacDecoder` インスタンスを作成して、それへのポインタを返す
///
/// 返されたポインタは、使用後に `flac_decoder_free()` で破棄する必要がある
///
/// # 戻り値
///
/// 新しく作成された `FlacDecoder` インスタンスへのポインタ
/// （現在の実装では NULL ポインタが返されることはない）
#[unsafe(no_mangle)]
pub extern "C" fn flac_decoder_new() -> *mut FlacDecoder {
    let decoder = Box::new(FlacDecoder {
        inner: shiguredo_flac::decoder::StreamDecoder::new(),
        current_frame: None,
        finished: false,
        last_error_string: None,
        vorbis_comment: None,
    });
    Box::into_raw(decoder)
}

/// `FlacDecoder` インスタンスを破棄して、割り当てられたリソースを解放する
///
/// # 引数
///
/// - `decoder`: 破棄する `FlacDecoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、この関数は何もしない
///
/// # 注意
///
/// この関数の呼び出し後は、`flac_decoder_decode_frame()` で取得した
/// `FlacDecodedFrame::samples` のポインタも無効になる
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_decoder_free(decoder: *mut FlacDecoder) {
    if !decoder.is_null() {
        let _ = unsafe { Box::from_raw(decoder) };
    }
}

/// `FlacDecoder` で最後に発生したエラーのメッセージを取得する
///
/// # 引数
///
/// - `decoder`: `FlacDecoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、NULL 終端の空文字列へのポインタを返す
///
/// # 戻り値
///
/// - メッセージが存在する場合: NULL 終端のエラーメッセージへのポインタ
/// - メッセージが存在しない場合: NULL 終端の空文字列へのポインタ
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_decoder_get_last_error(decoder: *const FlacDecoder) -> *const c_char {
    if decoder.is_null() {
        return c"".as_ptr();
    }

    let decoder = unsafe { &*decoder };
    let Some(e) = &decoder.last_error_string else {
        return c"".as_ptr();
    };
    e.as_ptr()
}

/// FLAC ストリームの入力データを投入する
///
/// ストリーム全体を一括で渡しても、受信のたびに分割して渡してもよい
/// (分割の境界はフレーム境界と一致していなくてもよい)
///
/// # 引数
///
/// - `decoder`: `FlacDecoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `data`: 入力データへのポインタ
///   - `size` が 0 より大きいのに NULL が渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `size`: 入力データのサイズ（バイト単位）
///   - 0 を指定した場合は何もしない
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常にデータが投入された
/// - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
/// - `FLAC_ERROR_INVALID_STATE`: `flac_decoder_finish()` の呼び出し後に呼ばれた
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_decoder_feed(
    decoder: *mut FlacDecoder,
    data: *const u8,
    size: u32,
) -> FlacError {
    if decoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let decoder = unsafe { &mut *decoder };

    if decoder.finished {
        decoder.set_last_error("[flac_decoder_feed] Decoder has already been finished");
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }

    if size == 0 {
        return FlacError::FLAC_ERROR_OK;
    }
    if data.is_null() {
        decoder.set_last_error("[flac_decoder_feed] data is null");
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }

    let data = unsafe { std::slice::from_raw_parts(data, size as usize) };
    decoder.inner.feed(data);
    FlacError::FLAC_ERROR_OK
}

/// FLAC ストリームの入力の終端を通知する
///
/// 終端通知により、途中で切れたストリームの検出と MD5 チェックサムの検証が
/// 行われるようになる (RFC 9639 Section 8.2)
///
/// この関数の呼び出し後に `flac_decoder_decode_frame()` が残りのフレームを
/// 返し終えると、`FLAC_ERROR_NO_MORE_FRAMES` が返されるようになる
///
/// # 引数
///
/// - `decoder`: `FlacDecoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に終端が通知された
/// - `FLAC_ERROR_NULL_POINTER`: `decoder` が NULL である
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_decoder_finish(decoder: *mut FlacDecoder) -> FlacError {
    if decoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let decoder = unsafe { &mut *decoder };

    decoder.inner.finish();
    decoder.finished = true;
    FlacError::FLAC_ERROR_OK
}

/// FLAC ストリームからフレームをひとつデコードする
///
/// # 引数
///
/// - `decoder`: `FlacDecoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `out_frame`: デコード結果を受け取る `FlacDecodedFrame` 構造体へのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常にフレームがデコードされた
/// - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
/// - `FLAC_ERROR_INPUT_REQUIRED`: フレームのデコードに必要な入力データが不足している
///   - `flac_decoder_feed()` で追加のデータを投入するか、
///     入力の終端であれば `flac_decoder_finish()` を呼び出す必要がある
/// - `FLAC_ERROR_NO_MORE_FRAMES`: すべてのフレームをデコードし終えた
///   - この時点でストリームの完全性 (MD5 / 総サンプル数) の検証も完了している
/// - `FLAC_ERROR_INVALID_DATA`: 入力データが FLAC として不正である
///
/// # 注意
///
/// `out_frame->samples` が参照するバッファは `FlacDecoder` が所有しており、
/// 同じデコーダーに対して次の `flac_decoder_decode_frame()` を呼び出すか
/// `flac_decoder_free()` を呼び出すと無効になる
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_decoder_decode_frame(
    decoder: *mut FlacDecoder,
    out_frame: *mut FlacDecodedFrame,
) -> FlacError {
    if decoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let decoder = unsafe { &mut *decoder };

    if out_frame.is_null() {
        decoder.set_last_error("[flac_decoder_decode_frame] out_frame is null");
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }

    match decoder.inner.decode_frame() {
        Ok(Some(frame)) => {
            // フレームの実体をデコーダーに保持させて、サンプル列への
            // ポインタが次の呼び出しまで有効であることを保証する
            let frame = decoder.current_frame.insert(frame);
            unsafe {
                (*out_frame).samples = frame.samples.as_ptr();
                (*out_frame).sample_count = frame.samples.len() as u32;
                (*out_frame).sample_rate = frame.sample_rate;
                (*out_frame).first_sample_number = frame.first_sample_number;
                (*out_frame).channels = frame.channels;
                (*out_frame).bits_per_sample = frame.bits_per_sample;
            }
            FlacError::FLAC_ERROR_OK
        }
        Ok(None) => {
            if decoder.finished {
                FlacError::FLAC_ERROR_NO_MORE_FRAMES
            } else {
                FlacError::FLAC_ERROR_INPUT_REQUIRED
            }
        }
        Err(e) => {
            decoder.set_last_error(&format!(
                "[flac_decoder_decode_frame] Failed to decode frame: {e}"
            ));
            e.into()
        }
    }
}

/// FLAC ストリームの STREAMINFO メタデータを取得する
///
/// STREAMINFO はストリームの先頭に位置するため、
/// 最初のフレームがデコードできる時点では必ず取得できる
///
/// # 引数
///
/// - `decoder`: `FlacDecoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `out_stream_info`: STREAMINFO を受け取る `FlacStreamInfo` 構造体へのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に STREAMINFO が取得された
/// - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
/// - `FLAC_ERROR_INPUT_REQUIRED`: STREAMINFO のデコードに必要な入力データが不足している
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_decoder_get_stream_info(
    decoder: *mut FlacDecoder,
    out_stream_info: *mut FlacStreamInfo,
) -> FlacError {
    if decoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let decoder = unsafe { &mut *decoder };

    if out_stream_info.is_null() {
        decoder.set_last_error("[flac_decoder_get_stream_info] out_stream_info is null");
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }

    let Some(info) = decoder.inner.stream_info() else {
        decoder
            .set_last_error("[flac_decoder_get_stream_info] STREAMINFO has not been decoded yet");
        return FlacError::FLAC_ERROR_INPUT_REQUIRED;
    };

    unsafe {
        (*out_stream_info).min_block_size = info.min_block_size;
        (*out_stream_info).max_block_size = info.max_block_size;
        (*out_stream_info).min_frame_size = info.min_frame_size;
        (*out_stream_info).max_frame_size = info.max_frame_size;
        (*out_stream_info).sample_rate = info.sample_rate;
        (*out_stream_info).channels = info.channels;
        (*out_stream_info).bits_per_sample = info.bits_per_sample;
        (*out_stream_info).total_samples = info.total_samples;
        (*out_stream_info).md5 = info.md5;
    }
    FlacError::FLAC_ERROR_OK
}

/// FLAC ストリームの VORBIS_COMMENT メタデータブロックを取得する
///
/// VORBIS_COMMENT は STREAMINFO に続くメタデータブロックとして格納されるため、
/// メタデータブロックをすべて読み終えるまで取得できない。
/// `flac_decoder_decode_frame()` が最初のフレームを返した時点で確定している
///
/// # 引数
///
/// - `decoder`: `FlacDecoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `out_comment`: VORBIS_COMMENT を受け取る `FlacVorbisComment` 構造体へのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に VORBIS_COMMENT が取得された
/// - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
/// - `FLAC_ERROR_INPUT_REQUIRED`: メタデータブロックのデコードに必要な入力データが不足している
/// - `FLAC_ERROR_NOT_FOUND`: ストリームに VORBIS_COMMENT が含まれていない
///
/// # 注意
///
/// `out_comment->vendor` と `out_comment->fields` が参照するバッファは
/// `FlacDecoder` が所有しており、次のいずれかを行うと無効になる:
/// - 同じデコーダーに対して再度この関数を呼び出す
/// - `flac_decoder_free()` を呼び出す
///
/// `flac_decoder_decode_frame()` では無効にならない
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_decoder_get_vorbis_comment(
    decoder: *mut FlacDecoder,
    out_comment: *mut FlacVorbisComment,
) -> FlacError {
    if decoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let decoder = unsafe { &mut *decoder };

    if out_comment.is_null() {
        decoder.set_last_error("[flac_decoder_get_vorbis_comment] out_comment is null");
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }

    // メタデータブロックをすべて読み終えていないと VORBIS_COMMENT の有無は確定しない
    let complete = decoder.inner.metadata_complete();
    let comment = decoder
        .inner
        .metadata()
        .iter()
        .find_map(|block| match block {
            shiguredo_flac::metadata::MetadataBlock::VorbisComment(c) => Some(c),
            _ => None,
        });

    let Some(comment) = comment else {
        // メタデータを読み終えていれば、VORBIS_COMMENT は存在しないと確定する。
        // 読み終えていなければ、後続ブロックとして現れる可能性が残っている
        if complete {
            decoder.set_last_error(
                "[flac_decoder_get_vorbis_comment] VORBIS_COMMENT block is not present",
            );
            return FlacError::FLAC_ERROR_NOT_FOUND;
        }
        decoder.set_last_error(
            "[flac_decoder_get_vorbis_comment] metadata blocks have not been decoded yet",
        );
        return FlacError::FLAC_ERROR_INPUT_REQUIRED;
    };

    match VorbisCommentCache::new(comment) {
        Ok(cache) => {
            let cache = decoder.vorbis_comment.insert(cache);
            unsafe {
                (*out_comment).vendor = cache.vendor.as_ptr();
                (*out_comment).fields = if cache.fields.is_empty() {
                    std::ptr::null()
                } else {
                    cache.fields.as_ptr()
                };
                (*out_comment).field_count = cache.fields.len() as u32;
            }
            FlacError::FLAC_ERROR_OK
        }
        Err(e) => {
            decoder.set_last_error(&format!(
                "[flac_decoder_get_vorbis_comment] Failed to convert vendor or field: {e}"
            ));
            FlacError::FLAC_ERROR_INVALID_DATA
        }
    }
}

impl VorbisCommentCache {
    /// VORBIS_COMMENT を C から参照できる形に変換する
    ///
    /// 文字列に NUL バイトが含まれる場合は変換に失敗する
    /// (RFC 9639 Section 8.6 の UTF-8 文字列に NUL は含まれ得るが、
    /// C 文字列として表現できないためエラーとする)
    fn new(
        comment: &shiguredo_flac::vorbis_comment::VorbisComment,
    ) -> Result<Self, std::ffi::NulError> {
        let vendor = CString::new(comment.vendor.as_str())?;
        let mut entries = Vec::new();
        for field in &comment.fields {
            entries.push((
                CString::new(field.name.as_str())?,
                CString::new(field.value.as_str())?,
            ));
        }
        // entries の (name, value) を指すフィールド配列を作る。
        // entries は self が保持するため、ポインタは self の生存期間中有効
        let fields = entries
            .iter()
            .map(|(name, value)| FlacVorbisCommentField {
                name: name.as_ptr(),
                value: value.as_ptr(),
            })
            .collect();
        Ok(Self {
            vendor,
            fields,
            _entries: entries,
        })
    }
}
