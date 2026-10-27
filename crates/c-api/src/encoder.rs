//! ../../../src/encoder.rs の C API を定義するためのモジュール
use std::ffi::{CStr, CString, c_char};

use shiguredo_flac::metadata::MetadataBlock;
use shiguredo_flac::vorbis_comment::{VorbisComment, VorbisCommentField};

use crate::error::FlacError;

/// PCM サンプルの FLAC ストリームへのエンコード処理を行うための構造体
///
/// STREAMINFO の合計サンプル数・MD5・フレームサイズ統計はエンコード完了時に
/// 確定するため、出力は `flac_encoder_finalize()` の後にまとめて取得する
///
/// # 関連関数
///
/// この構造体は、以下の関数を通して操作する必要がある:
/// - `flac_encoder_new()`: `FlacEncoder` インスタンスを生成する
/// - `flac_encoder_free()`: リソースを解放する
/// - `flac_encoder_set_sample_rate()` などの設定関数: エンコード設定を変更する
/// - `flac_encoder_initialize()`: エンコード処理を初期化する
/// - `flac_encoder_push_samples()`: サンプルを投入する
/// - `flac_encoder_finalize()`: エンコード処理を完了する
/// - `flac_encoder_get_output()`: 完成した FLAC ストリームを取得する
/// - `flac_encoder_get_last_error()`: 最後に発生したエラーのメッセージを取得する
///
/// # 使用例
///
/// ```c
/// #include <stdio.h>
/// #include <stdlib.h>
/// #include "flac.h"
///
/// int main(void) {
///     // 1. FlacEncoder インスタンスを生成
///     FlacEncoder *encoder = flac_encoder_new();
///
///     // 2. エンコード設定 (初期化前に行う必要がある)
///     flac_encoder_set_sample_rate(encoder, 44100);
///     flac_encoder_set_channels(encoder, 2);
///     flac_encoder_set_bits_per_sample(encoder, 16);
///
///     // 3. エンコード処理を初期化
///     FlacError ret = flac_encoder_initialize(encoder);
///     if (ret != FLAC_ERROR_OK) {
///         fprintf(stderr, "Failed to initialize encoder: %s\n",
///                 flac_encoder_get_last_error(encoder));
///         flac_encoder_free(encoder);
///         return 1;
///     }
///
///     // 4. チャンネルインターリーブ済み (L, R, L, R, ...) のサンプルを投入
///     int32_t samples[] = {100, -100, 200, -200};
///     ret = flac_encoder_push_samples(encoder, samples, 4);
///     if (ret != FLAC_ERROR_OK) {
///         fprintf(stderr, "Failed to push samples: %s\n",
///                 flac_encoder_get_last_error(encoder));
///         flac_encoder_free(encoder);
///         return 1;
///     }
///
///     // 5. エンコード処理を完了
///     ret = flac_encoder_finalize(encoder);
///     if (ret != FLAC_ERROR_OK) {
///         fprintf(stderr, "Failed to finalize encoder: %s\n",
///                 flac_encoder_get_last_error(encoder));
///         flac_encoder_free(encoder);
///         return 1;
///     }
///
///     // 6. 完成した FLAC ストリームを取得してファイルに書き込む
///     const uint8_t *output_data;
///     uint64_t output_size;
///     flac_encoder_get_output(encoder, &output_data, &output_size);
///     FILE *fp = fopen("output.flac", "wb");
///     fwrite(output_data, 1, output_size, fp);
///     fclose(fp);
///
///     // 7. リソース解放
///     flac_encoder_free(encoder);
///     return 0;
/// }
/// ```
pub struct FlacEncoder {
    config: shiguredo_flac::encoder::StreamEncoderConfig,
    inner: Option<shiguredo_flac::encoder::StreamEncoder>,
    /// `flac_encoder_finalize()` で確定した FLAC ストリーム
    output: Option<Vec<u8>>,
    last_error_string: Option<CString>,
}

impl FlacEncoder {
    fn set_last_error(&mut self, message: &str) {
        // NUL バイトを含む極めて稀なケースではメッセージを切り詰める
        self.last_error_string = CString::new(message.split('\0').next().unwrap_or("")).ok();
    }

    /// 初期化前にだけ許される設定変更の状態チェックを行う
    ///
    /// 初期化済みまたはファイナライズ済みであればエラーメッセージを設定して `false` を返す
    fn check_not_initialized(&mut self, function_name: &str) -> bool {
        if self.inner.is_some() || self.output.is_some() {
            let reason = if self.output.is_some() {
                "Encoder has already been finalized"
            } else {
                "Encoder has already been initialized"
            };
            self.set_last_error(&format!("[{function_name}] {reason}"));
            false
        } else {
            true
        }
    }
}

/// 新しい `FlacEncoder` インスタンスを作成して、それへのポインタを返す
///
/// 返されたポインタは、使用後に `flac_encoder_free()` で破棄する必要がある
///
/// エンコード設定は以下のデフォルト値で初期化される:
/// - サンプルレート: 44100 Hz
/// - チャンネル数: 2
/// - ビット深度: 16
/// - ブロックサイズ: 4096
/// - LPC の最大次数: 8
/// - Rice パーティションの最大オーダー: 4
/// - ステレオデコリレーション: 有効
///
/// # 戻り値
///
/// 新しく作成された `FlacEncoder` インスタンスへのポインタ
/// （現在の実装では NULL ポインタが返されることはない）
#[unsafe(no_mangle)]
pub extern "C" fn flac_encoder_new() -> *mut FlacEncoder {
    let encoder = Box::new(FlacEncoder {
        config: shiguredo_flac::encoder::StreamEncoderConfig::default(),
        inner: None,
        output: None,
        last_error_string: None,
    });
    Box::into_raw(encoder)
}

/// `FlacEncoder` インスタンスを破棄して、割り当てられたリソースを解放する
///
/// # 引数
///
/// - `encoder`: 破棄する `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、この関数は何もしない
///
/// # 注意
///
/// この関数の呼び出し後は、`flac_encoder_get_output()` で取得した
/// 出力データへのポインタも無効になる
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_free(encoder: *mut FlacEncoder) {
    if !encoder.is_null() {
        let _ = unsafe { Box::from_raw(encoder) };
    }
}

/// `FlacEncoder` で最後に発生したエラーのメッセージを取得する
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、NULL 終端の空文字列へのポインタを返す
///
/// # 戻り値
///
/// - メッセージが存在する場合: NULL 終端のエラーメッセージへのポインタ
/// - メッセージが存在しない場合: NULL 終端の空文字列へのポインタ
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_get_last_error(encoder: *const FlacEncoder) -> *const c_char {
    if encoder.is_null() {
        return c"".as_ptr();
    }

    let encoder = unsafe { &*encoder };
    let Some(e) = &encoder.last_error_string else {
        return c"".as_ptr();
    };
    e.as_ptr()
}

/// エンコードするストリームのサンプルレート (Hz) を設定する
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `sample_rate`: サンプルレート (Hz)。1-1048575 (RFC 9639 Section 8.2)
///   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に設定された
/// - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_set_sample_rate(
    encoder: *mut FlacEncoder,
    sample_rate: u32,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if !encoder.check_not_initialized("flac_encoder_set_sample_rate") {
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }

    encoder.config.sample_rate = sample_rate;
    FlacError::FLAC_ERROR_OK
}

/// エンコードするストリームのチャンネル数を設定する
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `channels`: チャンネル数 (1-8) (RFC 9639 Section 8.2)
///   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に設定された
/// - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_set_channels(
    encoder: *mut FlacEncoder,
    channels: u8,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if !encoder.check_not_initialized("flac_encoder_set_channels") {
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }

    encoder.config.channels = channels;
    FlacError::FLAC_ERROR_OK
}

/// エンコードするストリームのサンプルあたりのビット数を設定する
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `bits_per_sample`: サンプルあたりのビット数 (4-32) (RFC 9639 Section 8.2)
///   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に設定された
/// - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_set_bits_per_sample(
    encoder: *mut FlacEncoder,
    bits_per_sample: u8,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if !encoder.check_not_initialized("flac_encoder_set_bits_per_sample") {
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }

    encoder.config.bits_per_sample = bits_per_sample;
    FlacError::FLAC_ERROR_OK
}

/// エンコードのブロックサイズ (インターチャンネルサンプル数) を設定する
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `block_size`: ブロックサイズ (16-65535) (RFC 9639 Section 8.2)
///   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に設定された
/// - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_set_block_size(
    encoder: *mut FlacEncoder,
    block_size: u16,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if !encoder.check_not_initialized("flac_encoder_set_block_size") {
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }

    encoder.config.block_size = block_size;
    FlacError::FLAC_ERROR_OK
}

/// LPC の最大次数を設定する
///
/// 大きいほど圧縮率が上がる可能性があるが、エンコードは遅くなる
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `max_lpc_order`: LPC の最大次数 (0-32)。0 なら固定予測のみ使う (RFC 9639 Section 9.2.6)
///   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に設定された
/// - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_set_max_lpc_order(
    encoder: *mut FlacEncoder,
    max_lpc_order: u8,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if !encoder.check_not_initialized("flac_encoder_set_max_lpc_order") {
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }

    encoder.config.max_lpc_order = max_lpc_order;
    FlacError::FLAC_ERROR_OK
}

/// Rice パーティションの最大オーダーを設定する
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `max_partition_order`: Rice パーティションの最大オーダー (0-15) (RFC 9639 Section 9.2.7)
///   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に設定された
/// - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_set_max_partition_order(
    encoder: *mut FlacEncoder,
    max_partition_order: u8,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if !encoder.check_not_initialized("flac_encoder_set_max_partition_order") {
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }

    encoder.config.max_partition_order = max_partition_order;
    FlacError::FLAC_ERROR_OK
}

/// ステレオデコリレーション (mid-side 等) を試すかどうかを設定する
///
/// 2 チャンネルのストリームでのみ効果がある
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `stereo_decorrelation`: ステレオデコリレーションを試すかどうか
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に設定された
/// - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_set_stereo_decorrelation(
    encoder: *mut FlacEncoder,
    stereo_decorrelation: bool,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if !encoder.check_not_initialized("flac_encoder_set_stereo_decorrelation") {
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }

    encoder.config.stereo_decorrelation = stereo_decorrelation;
    FlacError::FLAC_ERROR_OK
}

/// エンコードするストリームに VORBIS_COMMENT メタデータブロックを設定する
///
/// この関数を複数回呼ぶと、最後に設定した内容で上書きされる。
/// フィールドを設定しない (vendor のみの) VORBIS_COMMENT を作ることもできる
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `vendor`: ファイルを生成したプログラム名 (NUL 終端、UTF-8)
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `field_names`: フィールド名の配列 (各要素は NUL 終端、UTF-8)
///   - `field_count` が 0 より大きいのに NULL が渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `field_values`: フィールド値の配列 (各要素は NUL 終端、UTF-8)
///   - `field_count` が 0 より大きいのに NULL が渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `field_count`: フィールド数
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に設定された
/// - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
/// - `FLAC_ERROR_INVALID_INPUT`: 文字列が UTF-8 として不正、またはフィールド名が
///   RFC 9639 Section 8.6 の制約 (U+0020-U+007E かつ `=` を含まない) に反する
///
/// エラーが発生した場合は、`flac_encoder_get_last_error()` でエラーメッセージを取得できる
///
/// # 注意
///
/// 渡された文字列はこの関数の中で複製されるため、呼び出し後に解放してよい
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_set_vorbis_comment(
    encoder: *mut FlacEncoder,
    vendor: *const c_char,
    field_names: *const *const c_char,
    field_values: *const *const c_char,
    field_count: u32,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if !encoder.check_not_initialized("flac_encoder_set_vorbis_comment") {
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }
    if vendor.is_null() {
        encoder.set_last_error("[flac_encoder_set_vorbis_comment] vendor is null");
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    if field_count > 0 && (field_names.is_null() || field_values.is_null()) {
        encoder.set_last_error(
            "[flac_encoder_set_vorbis_comment] field_names or field_values is null",
        );
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }

    let vendor = match unsafe { CStr::from_ptr(vendor) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            encoder.set_last_error("[flac_encoder_set_vorbis_comment] vendor is not valid UTF-8");
            return FlacError::FLAC_ERROR_INVALID_INPUT;
        }
    };

    let mut fields = Vec::new();
    for i in 0..field_count as usize {
        // field_count 分のポインタが有効であることは呼び出し側の責務
        let name_ptr = unsafe { *field_names.add(i) };
        let value_ptr = unsafe { *field_values.add(i) };
        if name_ptr.is_null() || value_ptr.is_null() {
            encoder.set_last_error(
                "[flac_encoder_set_vorbis_comment] field_names or field_values element is null",
            );
            return FlacError::FLAC_ERROR_NULL_POINTER;
        }
        let name = match unsafe { CStr::from_ptr(name_ptr) }.to_str() {
            Ok(s) => s,
            Err(_) => {
                encoder.set_last_error(&format!(
                    "[flac_encoder_set_vorbis_comment] field_names[{i}] is not valid UTF-8"
                ));
                return FlacError::FLAC_ERROR_INVALID_INPUT;
            }
        };
        let value = match unsafe { CStr::from_ptr(value_ptr) }.to_str() {
            Ok(s) => s,
            Err(_) => {
                encoder.set_last_error(&format!(
                    "[flac_encoder_set_vorbis_comment] field_values[{i}] is not valid UTF-8"
                ));
                return FlacError::FLAC_ERROR_INVALID_INPUT;
            }
        };
        fields.push(VorbisCommentField {
            name: String::from(name),
            value: String::from(value),
        });
    }

    let comment = VorbisComment {
        vendor: String::from(vendor),
        fields,
    };

    // 既存の VORBIS_COMMENT は取り除いてから追加する (上書き)。
    // 2 つ以上あると StreamEncoder::new() がエラーになるため
    encoder
        .config
        .metadata
        .retain(|block| !matches!(block, MetadataBlock::VorbisComment(_)));
    encoder
        .config
        .metadata
        .push(MetadataBlock::VorbisComment(comment));
    FlacError::FLAC_ERROR_OK
}

/// FLAC ストリームのエンコード処理を初期化する
///
/// この関数は、それまでに設定されたエンコード設定を検証し、
/// エンコード処理を開始するための準備を行う
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に初期化された
/// - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
/// - `FLAC_ERROR_INVALID_INPUT`: エンコード設定が不正である
///
/// エラーが発生した場合は、`flac_encoder_get_last_error()` でエラーメッセージを取得できる
///
/// # オプション指定
///
/// 以下の設定関数の呼び出しは `flac_encoder_initialize()` の前に行う必要がある:
/// - `flac_encoder_set_sample_rate()`
/// - `flac_encoder_set_channels()`
/// - `flac_encoder_set_bits_per_sample()`
/// - `flac_encoder_set_block_size()`
/// - `flac_encoder_set_max_lpc_order()`
/// - `flac_encoder_set_max_partition_order()`
/// - `flac_encoder_set_stereo_decorrelation()`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_initialize(encoder: *mut FlacEncoder) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if !encoder.check_not_initialized("flac_encoder_initialize") {
        return FlacError::FLAC_ERROR_INVALID_STATE;
    }

    match shiguredo_flac::encoder::StreamEncoder::new(encoder.config.clone()) {
        Ok(inner) => {
            encoder.inner = Some(inner);
            FlacError::FLAC_ERROR_OK
        }
        Err(e) => {
            encoder.set_last_error(&format!(
                "[flac_encoder_initialize] Failed to initialize encoder: {e}"
            ));
            e.into()
        }
    }
}

/// チャンネルインターリーブ済みの PCM サンプルを投入する
///
/// サンプル数はチャンネル数の倍数でなければならない。ブロックサイズ分の
/// サンプルが揃うたびに内部でフレームにエンコードされる
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `samples`: チャンネルインターリーブ済み (L, R, L, R, ...) のサンプル列へのポインタ
///   - 各サンプル値はビット深度で表現できる範囲に収まっている必要がある
///   - `sample_count` が 0 より大きいのに NULL が渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `sample_count`: `samples` の要素数
///   - チャンネル数の倍数でなければならない
///   - 0 を指定した場合は何もしない
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常にサンプルが投入された
/// - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが初期化されていないか、既にファイナライズ済み
/// - `FLAC_ERROR_INVALID_INPUT`: サンプル数がチャンネル数の倍数でないか、サンプル値が範囲外
///
/// エラーが発生した場合は、`flac_encoder_get_last_error()` でエラーメッセージを取得できる
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_push_samples(
    encoder: *mut FlacEncoder,
    samples: *const i32,
    sample_count: u32,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if sample_count > 0 && samples.is_null() {
        encoder.set_last_error("[flac_encoder_push_samples] samples is null");
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }

    let Some(inner) = &mut encoder.inner else {
        encoder.set_last_error(
            "[flac_encoder_push_samples] Encoder has not been initialized or has already been finalized",
        );
        return FlacError::FLAC_ERROR_INVALID_STATE;
    };

    if sample_count == 0 {
        return FlacError::FLAC_ERROR_OK;
    }

    let samples = unsafe { std::slice::from_raw_parts(samples, sample_count as usize) };
    if let Err(e) = inner.push_samples(samples) {
        encoder.set_last_error(&format!(
            "[flac_encoder_push_samples] Failed to push samples: {e}"
        ));
        e.into()
    } else {
        FlacError::FLAC_ERROR_OK
    }
}

/// FLAC ストリームのエンコード処理を完了する
///
/// この関数は、残りのサンプルを最終フレームとしてエンコードし、
/// STREAMINFO (合計サンプル数・MD5・フレームサイズ統計) を確定して
/// 完全な FLAC ストリームを構築する
///
/// エンコード処理が完了すると、`flac_encoder_get_output()` で
/// 完成した FLAC ストリームが取得できるようになる
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常にエンコード処理が完了した
/// - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーが初期化されていないか、既にファイナライズ済み
/// - `FLAC_ERROR_INVALID_INPUT`: 最終フレームのエンコードに失敗した
///
/// エラーが発生した場合は、`flac_encoder_get_last_error()` でエラーメッセージを取得できる
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_finalize(encoder: *mut FlacEncoder) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    let Some(inner) = encoder.inner.take() else {
        encoder.set_last_error(
            "[flac_encoder_finalize] Encoder has not been initialized or has already been finalized",
        );
        return FlacError::FLAC_ERROR_INVALID_STATE;
    };

    match inner.finish() {
        Ok(output) => {
            encoder.output = Some(output);
            FlacError::FLAC_ERROR_OK
        }
        Err(e) => {
            encoder.set_last_error(&format!(
                "[flac_encoder_finalize] Failed to finalize encoder: {e}"
            ));
            e.into()
        }
    }
}

/// 完成した FLAC ストリームを取得する
///
/// この関数は `flac_encoder_finalize()` の呼び出し後にのみ使用できる
///
/// # 引数
///
/// - `encoder`: `FlacEncoder` インスタンスへのポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
/// - `out_output_data`: 出力データのバッファへのポインタを受け取るポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
///   - 注意: このポインタの参照先は `flac_encoder_free()` を呼び出すと無効になる
/// - `out_output_size`: 出力データのサイズ（バイト単位）を受け取るポインタ
///   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
///
/// # 戻り値
///
/// - `FLAC_ERROR_OK`: 正常に出力データが取得された
/// - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
/// - `FLAC_ERROR_INVALID_STATE`: エンコーダーがまだファイナライズされていない
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_encoder_get_output(
    encoder: *mut FlacEncoder,
    out_output_data: *mut *const u8,
    out_output_size: *mut u64,
) -> FlacError {
    if encoder.is_null() {
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    let encoder = unsafe { &mut *encoder };

    if out_output_data.is_null() {
        encoder.set_last_error("[flac_encoder_get_output] out_output_data is null");
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }
    if out_output_size.is_null() {
        encoder.set_last_error("[flac_encoder_get_output] out_output_size is null");
        return FlacError::FLAC_ERROR_NULL_POINTER;
    }

    let Some(output) = &encoder.output else {
        encoder.set_last_error("[flac_encoder_get_output] Encoder has not been finalized");
        return FlacError::FLAC_ERROR_INVALID_STATE;
    };

    unsafe {
        *out_output_data = output.as_ptr();
        *out_output_size = output.len() as u64;
    }
    FlacError::FLAC_ERROR_OK
}
